use mehlon_server::map::{Map, MapBackend, ClientMap, spawn_tree,
	CHUNKSIZE, MapBlock};
use glium::{glutin, Surface, VertexBuffer};
use glutin::dpi::LogicalPosition;
use glutin::KeyboardInput;
use nalgebra::{Vector3, Matrix4, Point3, Rotation3};
use num_traits::identities::Zero;
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	rusttype::Font, Section,
};
use std::collections::{HashMap, VecDeque};
use std::time::{Instant, Duration};
use std::thread;
use std::sync::mpsc::{channel, Receiver};
use frustum_query::frustum::Frustum;
use collide::collide;
use srp::client::SrpClient;
use srp::groups::G_4096;
use sha2::Sha256;
use rand::RngCore;

use mehlon_server::{btchn, ServerToClientMsg, ClientToServerMsg};
use mehlon_server::generic_net::NetworkClientConn;
use mehlon_server::local_auth::{PlayerPwHash, HashParams};
use mehlon_server::config::Config;
use mehlon_server::map_storage::{PlayerPosition, PlayerIdPair};
use mehlon_server::inventory::{SelectableInventory, Stack};

use mehlon_meshgen::{Vertex, mesh_for_chunk, push_block};

use ui::{render_menu, square_mesh, ChatWindow, ChatWindowEvent,
	InventoryMenu, IDENTITY, render_inventory_hud};

use voxel_walk::VoxelWalker;

type MeshResReceiver = Receiver<(Vector3<isize>, Vec<Vertex>)>;

fn gen_chunks_around<B :MapBackend>(map :&mut Map<B>, pos :Vector3<isize>, xyradius :isize, zradius :isize) {
	let chunk_pos = btchn(pos);
	let radius = Vector3::new(xyradius, xyradius, zradius) * CHUNKSIZE;
	let chunk_pos_min = btchn(chunk_pos - radius);
	let chunk_pos_max = btchn(chunk_pos + radius);
	map.gen_chunks_in_area(chunk_pos_min, chunk_pos_max);
}

const VERTEX_SHADER_SRC :&str = include_str!("vertex-shader.glsl");

const FRAGMENT_SHADER_SRC :&str = include_str!("fragment-shader.glsl");

const KENPIXEL :&[u8] = include_bytes!("../assets/kenney-pixel.ttf");

enum AuthState {
	WaitingForBpub(String, SrpClient<'static, Sha256>),
	Authenticated,
}

pub struct Game<C :NetworkClientConn> {
	srv_conn :C,

	config :Config,
	auth_state :AuthState,

	meshres_r :MeshResReceiver,

	display :glium::Display,
	program :glium::Program,
	vbuffs :HashMap<Vector3<isize>, VertexBuffer<Vertex>>,

	selected_pos :Option<(Vector3<isize>, Vector3<isize>)>,
	sel_inventory :SelectableInventory,
	craft_inv :SelectableInventory,

	last_pos :Option<LogicalPosition>,

	last_frame_time :Instant,
	last_fps :f32,

	player_positions :Option<(PlayerIdPair, Vec<(PlayerIdPair, Vector3<f32>)>)>,

	grab_cursor :bool,
	grabbing_cursor :bool,
	has_focus :bool,
	chat_msgs :VecDeque<String>,
	chat_window :Option<ChatWindow>,
	inventory_menu :Option<InventoryMenu>,
	menu_enabled :bool,

	map :ClientMap,
	camera :Camera,

	swidth :f64,
	sheight :f64,
}

macro_rules! maybe_inventory_change {
	($m:ident, $this:ident) => {
		if $m.inventory() != &$this.sel_inventory {
			$this.sel_inventory = $m.inventory().clone();
			let msg = ClientToServerMsg::SetInventory($this.sel_inventory.clone());
			let _ = $this.srv_conn.send(msg);
		}
		if $m.craft_inv() != &$this.craft_inv {
			$this.craft_inv = $m.inventory().clone();
			// TODO send craft inventory to server
		}
	};
}

impl<C :NetworkClientConn> Game<C> {
	pub fn new(events_loop :&glutin::EventsLoop,
			srv_conn :C, config :Config, nick_pw :Option<(String, String)>) -> Self {
		let window = glutin::WindowBuilder::new()
			.with_title("Mehlon");
		let context = glutin::ContextBuilder::new().with_depth_buffer(24);
		let display = glium::Display::new(window, context, events_loop).unwrap();

		let mut map = ClientMap::new();
		let camera = Camera::new();

		let program = glium::Program::from_source(&display, VERTEX_SHADER_SRC,
			FRAGMENT_SHADER_SRC, None).unwrap();

		let (meshgen_s, meshgen_r) = channel();
		let (meshres_s, meshres_r) = channel();
		thread::spawn(move || {
			while let Ok((p, chunk)) = meshgen_r.recv() {
				let mesh = mesh_for_chunk(p, &chunk);
				let _ = meshres_s.send((p, mesh));
			}
		});

		map.register_on_change(Box::new(move |chunk_pos, chunk| {
			meshgen_s.send((chunk_pos, *chunk)).unwrap();
		}));

		let auth_state = if let Some((nick, pw)) = nick_pw {
			// Start doing the login
			let mut a = [0; 64];
			let mut rng = rand::rngs::OsRng;
			rng.fill_bytes(&mut a);
			let client = SrpClient::new(&a, &G_4096);
			let a_pub = client.get_a_pub();
			let _ = srv_conn.send(ClientToServerMsg::LogIn(nick, a_pub));
			AuthState::WaitingForBpub(pw, client)
		} else {
			AuthState::Authenticated
		};

		// This ensures that the mesh generation thread puts higher priority onto positions
		// close to the player at the beginning.
		gen_chunks_around(&mut map, camera.pos.map(|v| v as isize), 1, 1);

		let swidth = 1024.0;
		let sheight = 768.0;

		Game {
			srv_conn,

			config,
			auth_state,

			meshres_r,

			display,
			program,
			vbuffs : HashMap::new(),

			selected_pos : None,
			sel_inventory : SelectableInventory::new(),
			craft_inv : SelectableInventory::crafting_inv(),

			last_pos : None,
			last_frame_time : Instant::now(),
			last_fps : 0.0,

			player_positions : None,

			grab_cursor : true,
			grabbing_cursor : false,
			has_focus : false,
			chat_msgs : VecDeque::new(),
			chat_window : None,
			inventory_menu : None,
			menu_enabled : false,
			map,
			camera,

			swidth,
			sheight,
		}
	}
	/// Update the stored fps value and return the delta time
	fn update_fps(&mut self) -> f32 {
		let cur_time = Instant::now();
		let float_delta = durtofl(cur_time - self.last_frame_time);
		self.last_frame_time = cur_time;

		const EPS :f32 = 0.1;
		let fps_cur_term = if float_delta > 0.0 {
			1.0 / float_delta
		} else {
			// At the beginning float_delta can be zero
			// and 1/0 would fuck up the last_fps value
			900.0
		};
		let fps = self.last_fps * (1.0 - EPS) + fps_cur_term * EPS;
		self.last_fps = fps;
		float_delta
	}
	fn in_background(&self) -> bool {
		self.chat_window.is_some() ||
			self.inventory_menu.is_some() ||
			self.menu_enabled
	}
	pub fn run_loop(&mut self, events_loop :&mut glutin::EventsLoop) {
		let fonts = vec![Font::from_bytes(KENPIXEL).unwrap()];
		let mut glyph_brush = GlyphBrush::new(&self.display, fonts);
		'game_main_loop :loop {
			gen_chunks_around(&mut self.map,
				self.camera.pos.map(|v| v as isize), 4, 2);
			self.render(&mut glyph_brush);
			let float_delta = self.update_fps();
			let close = self.handle_events(events_loop);
			self.handle_mouse_buttons(float_delta);
			if !self.in_background() {
				self.movement(float_delta);
				let pos = PlayerPosition::from_pos_pitch_yaw(self.camera.pos,
					self.camera.pitch, self.camera.yaw);
				let msg = ClientToServerMsg::SetPos(pos);
				let _ = self.srv_conn.send(msg);

			}
			while let Ok(Some(msg)) = self.srv_conn.try_recv() {
				match msg {
					ServerToClientMsg::HashEnrollment => {
						if let AuthState::WaitingForBpub(ref pw, ref _srp_client) = self.auth_state {
							// choose params and send hash to server
							let params = HashParams::random();
							let pwh = PlayerPwHash::hash_password(pw, params).unwrap();
							let msg = ClientToServerMsg::SendHash(pwh);
							let _ = self.srv_conn.send(msg);
							println!("enrolling hash");
						} else {
							eprintln!("Error: received hash enrollment msg.");
						}
					},
					ServerToClientMsg::HashParamsBpub(params, b_pub) => {
						// TODO this is a hack, we unconditionally set the state to
						// authenticated because we need to put *something* there,
						// however we might not end up authenticated at all if we fail.
						// But we *need* to use mem::replace otherwise we couldn't
						// move out the client which is needed by the process_reply function.
						let state = std::mem::replace(&mut self.auth_state, AuthState::Authenticated);
						if let AuthState::WaitingForBpub(pw, srp_client) = state {
							let pwh = PlayerPwHash::hash_password(&pw, params).unwrap();
							// TODO get rid of unwrap
							let verifier = srp_client.process_reply(&pwh.hash(), &b_pub).unwrap();
							let msg = ClientToServerMsg::SendM1(verifier.get_proof().to_vec());
							let _ = self.srv_conn.send(msg);
							println!("sending hash");
						} else {
							eprintln!("Error: received hash params msg.");
						}
					},
					ServerToClientMsg::LogInFail(reason) => {
						println!("Log-In failed. Reason: {}", reason);
						break 'game_main_loop;
					},
					ServerToClientMsg::PlayerPositions(own_id, positions) => {
						self.player_positions = Some((own_id, positions));
					},
					ServerToClientMsg::SetPos(p) => {
						self.camera.pos = p.pos();
						self.camera.pitch = p.pitch();
						self.camera.yaw = p.yaw();
					},
					ServerToClientMsg::SetInventory(inv) => {
						self.sel_inventory = inv;
					},
					ServerToClientMsg::ChunkUpdated(p, c) => {
						self.map.set_chunk(p, c);
					},
					ServerToClientMsg::Chat(s) => {
						self.chat_msgs.push_back(s);
						const CHAT_MSGS_LIMIT :usize = 10;
						while self.chat_msgs.len() > CHAT_MSGS_LIMIT {
							self.chat_msgs.pop_front();
						}
					},
				}
			}

			if close {
				break;
			}
			if self.grabbing_cursor {
				self.display.gl_window().window().set_cursor_position(LogicalPosition {
					x : self.swidth / 2.0,
					y : self.sheight / 2.0,
				}).unwrap();
			}
		}
	}
	fn collide_delta_pos(&mut self, mut delta_pos :Vector3<f32>, time_delta :f32) -> Vector3<f32> {
		let pos = self.camera.pos.map(|v| v as isize);
		let new_pos = (self.camera.pos + delta_pos).map(|v| v as isize);
		let mut cubes = Vec::new();
		let d = 3;
		let cubes_min = Vector3::new(pos.x.min(new_pos.x) - d, pos.y.min(new_pos.y) - d, pos.z.min(new_pos.z) - d);
		let cubes_max = Vector3::new(pos.x.max(new_pos.x) + d, pos.y.max(new_pos.y) + d, pos.z.max(new_pos.z) + d);
		for x in cubes_min.x .. cubes_max.x {
			for y in cubes_min.y .. cubes_max.y {
				for z in cubes_min.z .. cubes_max.z {
					let p = Vector3::new(x, y, z);
					match self.map.get_blk(p) {
						Some(MapBlock::Air) => continue,
						None => (),
						Some(_) => (),
					}
					cubes.push(p);
				}
			}
		}
		let player_pos = self.camera.pos - Vector3::new(0.35, 0.35, 1.6);
		let mut touches_ground = false;
		for pos in cubes.into_iter() {
			// X coord
			let ppx = player_pos + Vector3::new(delta_pos.x, 0.0, 0.0);
			let collision = collide(ppx, pos);
			if let Some(normal) = collision {
				let d = delta_pos.dot(&normal);
				if d < 0.0 {
					delta_pos -= d * normal;
				}
			}

			// Y coord
			let ppy = player_pos + Vector3::new(0.0, delta_pos.y, 0.0);
			let collision = collide(ppy, pos);
			if let Some(normal) = collision {
				let d = delta_pos.dot(&normal);
				if d < 0.0 {
					delta_pos -= d * normal;
				}
			}

			// Z coord
			let ppz = player_pos + Vector3::new(0.0, 0.0, delta_pos.z);
			let collision = collide(ppz, pos);
			if let Some(normal) = collision {
				let d = delta_pos.dot(&normal);
				if normal.z > 0.0 {
					touches_ground = true;
				}
				if d < 0.0 {
					delta_pos -= d * normal;
				}
			}
		}
		if touches_ground || self.camera.fly_mode {
			self.camera.velocity = nalgebra::zero();
			if touches_ground && !self.camera.fly_mode && self.camera.up_pressed {
				// Jumping speed
				let jumping_speed = Vector3::new(0.0, 0.0, 60.0);
				self.camera.velocity = jumping_speed;
			}
		} else {
			let gravity = Vector3::new(0.0, 0.0, -9.81);
			self.camera.velocity += gravity * 3.0 * time_delta;
			// Maximum falling speed
			const MAX_FALLING_SPEED :f32 = 40.0;
			self.camera.velocity.z = clamp(self.camera.velocity.z, -MAX_FALLING_SPEED, 0.0);
		}
		//delta_pos.try_normalize_mut(std::f32::EPSILON);
		delta_pos
	}
	fn movement(&mut self, time_delta :f32) {
		let mut delta_pos = self.camera.delta_pos();
		if self.camera.fast_speed() {
			const FAST_DELTA :f32 = 40.0;
			delta_pos *= FAST_DELTA;
		} else {
			const DELTA :f32 = 10.0;
			delta_pos *= DELTA;
		}
		if !self.camera.fly_mode {
			delta_pos += self.camera.velocity;
		}
		delta_pos = delta_pos * time_delta;
		if !self.camera.is_noclip() {
			delta_pos = self.collide_delta_pos(delta_pos, time_delta);
		}
		self.camera.pos += delta_pos;
	}
	fn chat_string(&self) -> String {
		self.chat_msgs.iter().fold(String::new(), |v, w| v + "\n" + w)
	}
	fn render<'a, 'b>(&mut self, glyph_brush :&mut GlyphBrush<'a, 'b>) {
		self.recv_vbuffs();
		let pmatrix = self.camera.get_perspective();
		let vmatrix = self.camera.get_matrix();
		let frustum = Frustum::from_modelview_and_projection_2d(
			&vmatrix,
			&pmatrix,
		);
		// building the uniforms
		let uniforms = uniform! {
			vmatrix : vmatrix,
			pmatrix : pmatrix,
			fog_near_far : [self.config.fog_near, self.config.fog_far]
		};
		self.selected_pos = self.camera.get_selected_pos(&self.map);
		let mut sel_text = "sel = None".to_string();
		let mut selbuff = Vec::new();
		if let Some((selected_pos, _)) = self.selected_pos {
			let blk = self.map.get_blk(selected_pos).unwrap();
			sel_text = format!("sel = ({}, {}, {}), {:?}", selected_pos.x, selected_pos.y, selected_pos.z, blk);

			// TODO: only update if the position actually changed from the prior one
			// this spares us needless chatter with the GPU
			let vertices = selection_mesh(selected_pos);
			let vbuff = VertexBuffer::new(&self.display, &vertices).unwrap();
			selbuff = vec![vbuff];
		}
		let mut pl_buf = Vec::new();
		if let Some((own_id, positions)) = &self.player_positions {
			for (id, pos) in positions {
				if id == own_id {
					continue;
				}
				let v = player_mesh(*pos);
				let vbuff = VertexBuffer::new(&self.display, &v).unwrap();
				pl_buf.push(vbuff);
			}
		}
		let screen_dims = self.display.get_framebuffer_dimensions();

		let polygon_mode = if !self.config.draw_poly_lines {
			glium::draw_parameters::PolygonMode::Fill
		} else {
			glium::draw_parameters::PolygonMode::Line
		};

		let params = glium::draw_parameters::DrawParameters {
			depth : glium::Depth {
				test : glium::draw_parameters::DepthTest::IfLess,
				write : true,
				.. Default::default()
			},
			backface_culling : glium::draw_parameters::BackfaceCullingMode::CullCounterClockwise,
			blend :glium::Blend::alpha_blending(),
			polygon_mode,
			.. Default::default()
		};

		// drawing a frame
		let mut target = self.display.draw();
		target.clear_color_and_depth((0.05, 0.01, 0.6, 0.0), 1.0);

		let player_pos = self.camera.pos;
		let mut drawn_chunks_count = 0;
		for buff in self.vbuffs.iter()
				.filter_map(|(p, m)| {
					// Viewing range based culling
					let viewing_range = self.config.viewing_range;
					if (p.map(|v| v as f32) - player_pos).norm() > viewing_range {
						return None;
					}
					// Frustum culling.
					// We approximate chunks as spheres here, as the library
					// has no cube checker.
					let p = p.map(|v| (v + CHUNKSIZE / 2) as f32);
					let r = CHUNKSIZE as f32 * 3.0_f32.sqrt();
					if !frustum.sphere_intersecting(&p.x, &p.y, &p.z, &r) {
						return None;
					}
					return Some(m);
				})
				.chain(selbuff.iter())
				.chain(pl_buf.iter()) {
			drawn_chunks_count += 1;
			target.draw(buff,
				&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
				&self.program, &uniforms, &params).unwrap();
		}

		// TODO turn off anti-aliasing of the font
		// https://gitlab.redox-os.org/redox-os/rusttype/issues/61
		let text = format!("pos = ({:.2}, {:.2}, {:.2}) pi = {:.0}, yw = {:.0}, {}, FPS: {:1.2}, CL: {} CD: {}",
				self.camera.pos.x, self.camera.pos.y, self.camera.pos.z,
				self.camera.pitch, self.camera.yaw,
				sel_text, self.last_fps as u16,
				self.vbuffs.len(), drawn_chunks_count) + "\n" + &self.chat_string();
		glyph_brush.queue(Section {
			text :&text,
			bounds : (screen_dims.0 as f32, screen_dims.1 as f32),
			color : [0.9, 0.9, 0.9, 1.0],
			.. Section::default()
		});

		glyph_brush.draw_queued(&self.display, &mut target);
		// Draw the wielded item
		{
			let params = glium::draw_parameters::DrawParameters {
				backface_culling : glium::draw_parameters::BackfaceCullingMode::CullCounterClockwise,
				blend :glium::Blend::alpha_blending(),
				.. Default::default()
			};
			let vmatrix :[[f32; 4]; 4] = {
				let m = Matrix4::look_at_rh(&(Point3::origin()),
					&(Point3::origin() + Vector3::x() + Vector3::y()), &Vector3::z());
				m.into()
			};
			let pmatrix :[[f32; 4]; 4] = {
				let fov = dtr(45.0);
				let zfar = 1024.0;
				let znear = 0.1;
				Matrix4::new_perspective(self.camera.aspect_ratio, fov, znear, zfar).into()
			};
			let uniforms = uniform! {
				vmatrix : vmatrix,
				pmatrix : pmatrix,
				fog_near_far : [40.0f32, 60.0]
			};
			let hand_mesh_pos = Vector3::new(3.0, 1.0, -1.5);
			if let Some(item) = self.sel_inventory.get_selected() {
				let hand_mesh = hand_mesh(hand_mesh_pos, item);
				let vbuff = VertexBuffer::new(&self.display, &hand_mesh).unwrap();
				target.draw(&vbuff,
					&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
					&self.program, &uniforms, &params).unwrap();
			}
		}
		render_inventory_hud(
			&self.sel_inventory,
			&mut self.display,
			&self.program, glyph_brush, &mut target);
		if self.in_background() {
			if self.menu_enabled {
				render_menu(&mut self.display, &self.program, glyph_brush, &mut target);
			} else if let Some(cw) = &self.chat_window {
				cw.render(&mut self.display, &self.program, glyph_brush, &mut target);
			} else if let Some(m) = &mut self.inventory_menu {
				m.render(
					&mut self.display,
					&self.program, glyph_brush, &mut target);
				maybe_inventory_change!(m, self);
			}
		} else {
			let params = glium::draw_parameters::DrawParameters {
				blend :glium::Blend::alpha_blending(),
				.. Default::default()
			};

			let uniforms = uniform! {
				vmatrix : IDENTITY,
				pmatrix : IDENTITY,
				fog_near_far : [40.0f32, 60.0]
			};
			// Draw crosshair
			let vertices_horiz = square_mesh((20, 2), screen_dims, [0.8, 0.8, 0.8, 0.85]);
			let vertices_vert = square_mesh((2, 20), screen_dims, [0.8, 0.8, 0.8, 0.85]);
			let mut vertices = vertices_horiz;
			vertices.extend_from_slice(&vertices_vert);
			let vbuff = VertexBuffer::new(&self.display, &vertices).unwrap();
			target.draw(&vbuff,
				&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
				&self.program, &uniforms, &params).unwrap();
		}

		target.finish().unwrap();
	}

	fn recv_vbuffs(&mut self) {
		while let Ok((p, m)) = self.meshres_r.try_recv() {
			let vbuff = VertexBuffer::new(&self.display, &m).unwrap();
			self.vbuffs.insert(p, vbuff);
		}
	}

	fn check_grab_change(&mut self) {
		let grabbing_cursor = self.has_focus &&
			!self.in_background() && self.grab_cursor;
		if self.grabbing_cursor != grabbing_cursor {
			self.display.gl_window().window().hide_cursor(grabbing_cursor);
			let _  = self.display.gl_window().window().grab_cursor(grabbing_cursor);
			self.grabbing_cursor = grabbing_cursor;
		}
	}

	fn handle_chat_win_ev(&mut self, ev :ChatWindowEvent) {
		match ev {
			ChatWindowEvent::CloseChatWindow => {
				self.chat_window = None;
				self.check_grab_change();
			},
			ChatWindowEvent::SendChat => {
				{
					let text = &self.chat_window.as_ref().unwrap().text();
					let msg = ClientToServerMsg::Chat(text.to_string());
					let _ = self.srv_conn.send(msg);
				}
				self.chat_window = None;
				self.check_grab_change();
			},
			ChatWindowEvent::None => (),
		}
	}
	fn handle_kinput(&mut self, input :&KeyboardInput) -> bool {
		match input.virtual_keycode {
			Some(glutin::VirtualKeyCode::Q) if input.modifiers.ctrl => {
				return true;
			},
			_ => (),
		}

		if let Some(ev) = (&mut self.chat_window).as_mut().map(|w| w.handle_kinput(input)) {
			self.handle_chat_win_ev(ev);
			return false;
		}
		if input.virtual_keycode == Some(glutin::VirtualKeyCode::Escape) {
			if let Some(m) = self.inventory_menu.take() {
				maybe_inventory_change!(m, self);

				self.check_grab_change();
				return false;
			}
		}

		match input.virtual_keycode {
			Some(glutin::VirtualKeyCode::Escape) => {
				if input.state == glutin::ElementState::Pressed {
					self.menu_enabled = !self.menu_enabled;
					self.check_grab_change();
				}
			},
			Some(glutin::VirtualKeyCode::I) => {
				if input.state == glutin::ElementState::Pressed {
					if let Some(m) = self.inventory_menu.take() {
						maybe_inventory_change!(m, self);
					} else {
						self.inventory_menu = Some(InventoryMenu::new(
							self.sel_inventory.clone(),
							self.craft_inv.clone()));
					}
					self.check_grab_change();
				}
			},
			_ => (),
		}
		self.camera.handle_kinput(input);
		return false;
	}
	fn handle_mouse_buttons(&mut self, float_delta :f32) {
		const BUTTON_COOLDOWN :f32 = 0.2;
		self.camera.mouse_left_cooldown -= float_delta;
		self.camera.mouse_right_cooldown -= float_delta;
		if let Some((selected_pos, before_selected)) = self.selected_pos {
			if self.camera.mouse_left_down {
				if self.camera.mouse_left_cooldown <= 0.0 {
					let mut blk = self.map.get_blk_mut(selected_pos).unwrap();
					let dug_block = blk.get();
					self.sel_inventory.put(Stack::with(dug_block, 1));
					blk.set(MapBlock::Air);
					let msg = ClientToServerMsg::SetInventory(self.sel_inventory.clone());
					let _ = self.srv_conn.send(msg);
					let msg = ClientToServerMsg::SetBlock(selected_pos, MapBlock::Air);
					let _ = self.srv_conn.send(msg);
					self.camera.mouse_left_cooldown = BUTTON_COOLDOWN;
				}
			}
			if self.camera.mouse_right_down
					&& self.camera.mouse_right_cooldown <= 0.0 {
				if let Some(ith) = self.sel_inventory.take_selected() {
					let mut blk = self.map.get_blk_mut(before_selected).unwrap();
					blk.set(ith);
					let msg = ClientToServerMsg::SetInventory(self.sel_inventory.clone());
					let _ = self.srv_conn.send(msg);
					let msg = ClientToServerMsg::SetBlock(before_selected, ith);
					let _ = self.srv_conn.send(msg);
					self.camera.mouse_right_cooldown = BUTTON_COOLDOWN;
				}
			}
		}
	}
	fn handle_events(&mut self, events_loop :&mut glutin::EventsLoop) -> bool {
		let mut close = false;
		events_loop.poll_events(|event| {
			match event {
				glutin::Event::WindowEvent { event, .. } => match event {

					glutin::WindowEvent::Focused(focus) => {
						self.has_focus = focus;
						self.check_grab_change();
					},

					glutin::WindowEvent::CloseRequested => close = true,

					glutin::WindowEvent::Resized(glium::glutin::dpi::LogicalSize {width, height}) => {
						self.swidth = width;
						self.sheight = height;
						self.camera.aspect_ratio = (width / height) as f32;
					},
					glutin::WindowEvent::KeyboardInput { input, .. } => {
						close |= self.handle_kinput(&input);
					},
					glutin::WindowEvent::ReceivedCharacter(ch) => {
						let ev = if let Some(ref mut w) = self.chat_window {
							w.handle_character(ch)
						} else {
							if ch == 't' || ch == '/' {
								let chwin = if ch == '/' {
									ChatWindow::with_text("/".to_owned())
								} else {
									ChatWindow::new()
								};
								self.chat_window = Some(chwin);
								self.check_grab_change();
							}
							ChatWindowEvent::None
						};
						self.handle_chat_win_ev(ev);
					},
					glutin::WindowEvent::CursorMoved { position, .. } => {
						if self.has_focus && !self.in_background() {
							if self.grab_cursor {
								self.last_pos = Some(LogicalPosition {
									x : self.swidth / 2.0,
									y : self.sheight / 2.0,
								});
							}

							if let Some(last) = self.last_pos {
								let delta = LogicalPosition {
									x : position.x - last.x,
									y : position.y - last.y,
								};
								self.camera.handle_mouse_move(delta);
							}
							self.last_pos = Some(position);
						}
						if self.has_focus {
							if let Some(m) = &mut self.inventory_menu {
								m.handle_mouse_moved(position);
							}
						}
					},
					glutin::WindowEvent::MouseInput { state, button, .. } => {
						if !self.in_background() {
							let pressed = state == glutin::ElementState::Pressed;
							if button == glutin::MouseButton::Left {
								self.camera.handle_mouse_left(pressed);
							} else if button == glutin::MouseButton::Right {
								self.camera.handle_mouse_right(pressed);
							}
							if let Some((_selected_pos, before_selected)) = self.selected_pos {
								if pressed && button == glutin::MouseButton::Middle {
									spawn_tree(&mut self.map, before_selected);
									let msg = ClientToServerMsg::PlaceTree(before_selected);
									let _ = self.srv_conn.send(msg);
								}
							}
						}
						if self.has_focus {
							if let Some(m) = &mut self.inventory_menu {
								m.handle_mouse_input(state, button);
							}
						}
					},
					glutin::WindowEvent::MouseWheel { delta, .. } => {
						if !self.in_background() {
							let lines_diff = match delta {
								glutin::MouseScrollDelta::LineDelta(_x, y) => y,
								glutin::MouseScrollDelta::PixelDelta(p) => p.y as f32,
							};
							if lines_diff < 0.0 {
								self.sel_inventory.rotate(true);
							} else if lines_diff > 0.0 {
								self.sel_inventory.rotate(false);
							}
							let msg = ClientToServerMsg::SetInventory(self.sel_inventory.clone());
							let _ = self.srv_conn.send(msg);
						}
					},

					_ => (),
				},
				_ => (),
			}
		});
		close
	}
}

fn selection_mesh(pos :Vector3<isize>) -> Vec<Vertex> {
	const DELTA :f32 = 0.05;
	const DELTAH :f32 = DELTA / 2.0;
	const COLOR :[f32; 4] = [0.0, 0.0, 0.3, 0.5];
	let mut vertices = Vec::new();

	push_block(&mut vertices,
		[pos.x as f32 - DELTAH, pos.y as f32 - DELTAH, pos.z as f32 - DELTAH],
		COLOR, COLOR, 1.0 + DELTA, |_| false);
	vertices
}

fn player_mesh(pos :Vector3<f32>) -> Vec<Vertex> {
	const COLORB :[f32; 4] = [0.3, 0.3, 0.5, 1.0];
	const COLOR_HEAD :[f32; 4] = [0.94, 0.76, 0.49, 1.0];
	let mut vertices = Vec::new();

	push_block(&mut vertices,
		[pos.x, pos.y, pos.z - 1.6 - 0.4],
		COLORB, COLORB, 0.8, |_| false);
	push_block(&mut vertices,
		[pos.x, pos.y, pos.z - 0.8 - 0.4],
		COLORB, COLORB, 0.8, |_| false);
	push_block(&mut vertices,
		[pos.x, pos.y, pos.z - 0.4],
		COLOR_HEAD, COLOR_HEAD, 0.8, |_| false);
	vertices
}

fn hand_mesh(pos :Vector3<f32>, blk :MapBlock) -> Vec<Vertex> {
	let mut vertices = Vec::new();
	let color = if let Some(c) = mehlon_meshgen::get_color_for_blk(blk) {
		c
	} else {
		return vec![];
	};
	let colorh = mehlon_meshgen::colorh(color);

	push_block(&mut vertices,
		[pos.x, pos.y, pos.z],
		color, colorh, 0.5, |_| false);
	vertices
}

fn durtofl(d :Duration) -> f32 {
	d.as_millis() as f32 / 1_000.0
}

fn clamp(a :f32, min :f32, max :f32) -> f32 {
	if a > min {
		if a < max {
			a
		} else {
			max
		}
	} else {
		min
	}
}

// TODO: once euclidean division stabilizes,
// use it: https://github.com/rust-lang/rust/issues/49048
fn mod_euc(a :f32, b :f32) -> f32 {
	((a % b) + b) % b
}

/// Degrees to radians
fn dtr(v :f32) -> f32 {
	v / 180.0 * std::f32::consts::PI
}

struct Camera {
	aspect_ratio :f32,
	pitch :f32,
	yaw :f32,
	pos :Vector3<f32>,
	velocity :Vector3<f32>,

	forward_pressed :bool,
	left_pressed :bool,
	right_pressed :bool,
	backward_pressed :bool,

	fast_pressed :bool,
	fast_mode :bool,
	noclip_mode :bool,
	fly_mode :bool,

	up_pressed :bool,
	down_pressed :bool,

	mouse_left_down :bool,
	mouse_right_down :bool,
	mouse_right_cooldown :f32,
	mouse_left_cooldown :f32,
}

impl Camera {
	fn new() -> Self {
		Camera {
			aspect_ratio : 1024.0 / 768.0,
			pitch : 0.0,
			yaw : 0.0,
			pos : Vector3::new(60.0, 40.0, 20.0),
			velocity : Vector3::new(0.0, 0.0, 0.0),

			forward_pressed : false,
			left_pressed : false,
			right_pressed : false,
			backward_pressed : false,

			fast_pressed : false,
			fast_mode : false,
			noclip_mode : false,
			fly_mode : true,

			up_pressed : false,
			down_pressed : false,

			mouse_left_down : false,
			mouse_right_down : false,
			mouse_left_cooldown : 0.0,
			mouse_right_cooldown : 0.0,
		}
	}
	fn handle_mouse_left(&mut self, down :bool) {
		self.mouse_left_down = down;
	}
	fn handle_mouse_right(&mut self, down :bool) {
		self.mouse_right_down = down;
	}
	fn handle_kinput(&mut self, input :&KeyboardInput) {
		let key = match input.virtual_keycode {
			Some(key) => key,
			None => return,
		};
		let mut b = None;
		match key {
			glutin::VirtualKeyCode::W => b = Some(&mut self.forward_pressed),
			glutin::VirtualKeyCode::A => b = Some(&mut self.left_pressed),
			glutin::VirtualKeyCode::S => b = Some(&mut self.backward_pressed),
			glutin::VirtualKeyCode::D => b = Some(&mut self.right_pressed),
			glutin::VirtualKeyCode::Space => b = Some(&mut self.up_pressed),
			glutin::VirtualKeyCode::LShift => b = Some(&mut self.down_pressed),
		_ => (),
		}
		if key == glutin::VirtualKeyCode::E {
			self.fast_pressed = input.state == glutin::ElementState::Pressed;
		}
		if key == glutin::VirtualKeyCode::K {
			if input.state == glutin::ElementState::Pressed {
				self.fly_mode = !self.fly_mode;
			}
		}
		if key == glutin::VirtualKeyCode::J {
			if input.state == glutin::ElementState::Pressed {
				self.fast_mode = !self.fast_mode;
			}
		}
		if key == glutin::VirtualKeyCode::H {
			if input.state == glutin::ElementState::Pressed {
				self.noclip_mode = !self.noclip_mode;
			}
		}

		if let Some(b) = b {
			*b = input.state == glutin::ElementState::Pressed;
		}
	}
	fn delta_pos(&mut self) -> Vector3<f32> {
		let mut delta_pos = Vector3::zero();
		if self.forward_pressed {
			delta_pos += Vector3::x();
		}
		if self.backward_pressed {
			delta_pos -= Vector3::x();
		}
		if self.left_pressed {
			delta_pos += Vector3::y();
		}
		if self.right_pressed {
			delta_pos -= Vector3::y();
		}
		if self.fly_mode {
			if self.up_pressed {
				delta_pos += Vector3::z()
			}
			if self.down_pressed {
				delta_pos -= Vector3::z();
			}
		}
		delta_pos.try_normalize_mut(std::f32::EPSILON);
		delta_pos = Rotation3::from_axis_angle(&Vector3::z_axis(), dtr(-self.yaw)) * delta_pos;

		delta_pos
	}
	fn handle_mouse_move(&mut self, delta :LogicalPosition) {
		let factor = 0.7;
		// Limit the pitch by this value so that we never look 100%
		// straight down. Otherwise the Matrix4::look_at_rh function
		// will return NaN values.
		// The further we are from the center, the stricter this limit
		// has to be, probably due to float precision reasons.
		// The value we set works for coordinates tens of thousands
		// of blocks away from the center (60_000.0, 40_000.0, 20.0).
		const MAX_PITCH :f32 = 89.0;
		self.pitch = clamp(factor * delta.y as f32 + self.pitch, -MAX_PITCH, MAX_PITCH);
		self.yaw += factor * delta.x as f32;
		self.yaw = mod_euc(self.yaw + 180.0, 360.0) - 180.0;
	}
	fn fast_speed(&self) -> bool {
		self.fast_mode || self.fast_pressed
	}
	fn is_noclip(&self) -> bool {
		self.noclip_mode && self.fly_mode
	}

	fn direction(&self) -> Point3<f32> {
		let pitch = dtr(-self.pitch);
		let yaw = dtr(-self.yaw);
		Point3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin())
	}

	fn get_matrix(&self) -> [[f32; 4]; 4] {
		let looking_at = self.direction() + self.pos;
		let m = Matrix4::look_at_rh(&(Point3::origin() + self.pos),
			&looking_at, &Vector3::z());
		m.into()
	}

	pub fn get_perspective(&self) -> [[f32; 4]; 4] {
		let fov = dtr(90.0);
		let zfar = 1024.0;
		let znear = 0.1;
		Matrix4::new_perspective(self.aspect_ratio, fov, znear, zfar).into()
	}

	pub fn get_selected_pos<B :MapBackend>(&self, map :&Map<B>) -> Option<(Vector3<isize>, Vector3<isize>)> {
		for (vs, ve) in VoxelWalker::new(self.pos,
				self.direction().coords) {
			let vs = vs.map(|v| v.floor() as isize);
			let ve = ve.map(|v| v.floor() as isize);
			if let Some(blk) = map.get_blk(ve) {
				if blk.is_pointable() {
					return Some((ve, vs));
				}
			}
		}
		None
	}
}
