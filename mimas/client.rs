use mimas_server::map::{Map, MapBackend, ClientMap,
	CHUNKSIZE, MapBlock, MetadataEntry};
use glium::{glutin, Surface, VertexBuffer};
use glium::texture::SrgbTexture2dArray;
use glium::uniforms::{MagnifySamplerFilter, SamplerWrapFunction};
use glium::glutin::platform::desktop::EventLoopExtDesktop;
use glutin::dpi::PhysicalPosition;
use glutin::event_loop::{EventLoop, ControlFlow};
use glutin::event::{Event, ElementState, KeyboardInput, VirtualKeyCode,
	WindowEvent, MouseButton, MouseScrollDelta};
use nalgebra::{Vector3, Matrix4, Point3, Translation3, Rotation3};
use num_traits::identities::Zero;
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	rusttype::Font, Section,
};
use std::collections::{HashMap, VecDeque};
use std::time::{Instant, Duration};
use std::thread;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use frustum_query::frustum::Frustum;
use collide::collide;
use srp::client::SrpClient;
use srp::groups::G_4096;
use sha2::Sha256;
use rand::RngCore;

use mimas_server::{btchn, ServerToClientMsg, ClientToServerMsg};
use mimas_server::generic_net::NetworkClientConn;
use mimas_server::local_auth::{PlayerPwHash, HashParams};
use mimas_server::config::Config;
use mimas_server::map_storage::{PlayerPosition, PlayerIdPair};
use mimas_server::inventory::{SelectableInventory, InventoryPos, InventoryLocation};
use mimas_server::game_params::{GameParamsHdl, DigGroup, ToolGroup};

use mimas_meshgen::{Vertex, mesh_for_chunk, push_block,
	BlockTextureIds, TextureIdCache, ChunkMesh};

use assets::{Assets, UiColors};

use ui::{render_menu, square_mesh, ChatWindow, ChatWindowEvent,
	ChestMenu, InventoryMenu, IDENTITY, render_inventory_hud,
	SwapCommand};

use voxel_walk::VoxelWalker;

type MeshResReceiver = Receiver<(Vector3<isize>, ChunkMesh)>;

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
	params :Option<GameParamsHdl>,
	ui_colors :Option<UiColors>,
	texture_id_cache :Option<TextureIdCache>,
	texture_array :Option<SrgbTexture2dArray>,

	meshgen_spawner :Option<Box<dyn FnOnce(TextureIdCache)>>,
	meshres_r :MeshResReceiver,

	display :glium::Display,
	program :glium::Program,
	vbuffs :HashMap<Vector3<isize>, (VertexBuffer<Vertex>, Option<VertexBuffer<Vertex>>)>,

	selected_pos :Option<(Vector3<isize>, Vector3<isize>)>,
	sel_inventory :SelectableInventory,
	craft_inv :SelectableInventory,

	last_pos :Option<PhysicalPosition<f64>>,

	last_frame_time :Instant,
	last_fps :f32,

	player_positions :Option<(PlayerIdPair, Vec<(PlayerIdPair, PlayerPosition)>)>,

	grab_cursor :bool,
	grabbing_cursor :bool,
	has_focus :bool,
	chat_msgs :VecDeque<String>,
	chat_window :Option<ChatWindow>,
	inventory_menu :Option<InventoryMenu>,
	chest_menu :Option<ChestMenu>,
	menu_enabled :bool,

	map :ClientMap,
	camera :Camera,

	swidth :f64,
	sheight :f64,
}

macro_rules! maybe_inventory_change {
	($m:ident, $this:ident, $command:expr) => {
		if let Some(cmd) = $command {
			// Needed because it complains about missing type annotations otherwise
			let cmd :SwapCommand = cmd;

			fn ind_to_loc(ind :usize) -> InventoryLocation {
				match ind {
					0 => InventoryLocation::CraftInv,
					// We don't generate movement commands to/from the craft output inv
					1 => unreachable!(),
					2 => InventoryLocation::PlayerInv,
					_ => unreachable!(),
				}
			}
			let from = InventoryPos {
				stack_pos : cmd.from_pos.1,
				location : ind_to_loc(cmd.from_pos.0),
			};
			let to = InventoryPos {
				stack_pos : cmd.to_pos.1,
				location : ind_to_loc(cmd.to_pos.0),
			};
			let msg = ClientToServerMsg::InventorySwap(from, to, cmd.only_move);
			let _ = $this.srv_conn.send(msg);
		} else if $m.inventory() != &$this.sel_inventory {
			$this.sel_inventory = $m.inventory().clone();
			let msg = ClientToServerMsg::SetInventory($this.sel_inventory.clone());
			let _ = $this.srv_conn.send(msg);
		}
		if $m.craft_inv() != &$this.craft_inv {
			$this.craft_inv = $m.craft_inv().clone();
			// TODO send craft inventory to server
		}
	};
}

macro_rules! maybe_chest_inventory_change {
	($m:ident, $this:ident, $command:expr) => {
		if $m.inventory() != &$this.sel_inventory {
			$this.sel_inventory = $m.inventory().clone();
		}
		if let Some(cmd) = $command {
			// Needed because it complains about missing type annotations otherwise
			let cmd :SwapCommand = cmd;

			let ind_to_loc = |ind :usize| -> InventoryLocation {
				match ind {
					0 => InventoryLocation::WorldMeta($m.chest_pos()),
					1 => InventoryLocation::PlayerInv,
					_ => unreachable!(),
				}
			};

			let from = InventoryPos {
				stack_pos : cmd.from_pos.1,
				location : ind_to_loc(cmd.from_pos.0),
			};
			let to = InventoryPos {
				stack_pos : cmd.to_pos.1,
				location : ind_to_loc(cmd.from_pos.0),
			};
			let msg = ClientToServerMsg::InventorySwap(from, to, cmd.only_move);
			let _ = $this.srv_conn.send(msg);
		} else if $m.inventory() != &$this.sel_inventory {
			let msg = ClientToServerMsg::SetInventory($this.sel_inventory.clone());
			let _ = $this.srv_conn.send(msg);
		}

		let mut chest_meta = $this.map.get_blk_meta_mut($m.chest_pos()).unwrap();
		if Some($m.chest_inv()) != chest_meta.get().map(|v| {
			let MetadataEntry::Inventory(inv) = v;
			inv
		}) {
			chest_meta.set(MetadataEntry::Inventory($m.chest_inv().clone()));

			// TODO maybe do some checks to ensure that $command is Some?
		}
	};
}

fn title() -> String {
	let title = format!(
		"{} {}",
		env!("CARGO_PKG_NAME"),
		env!("CARGO_PKG_VERSION"));
	let first = title.chars().next().unwrap();
	first.to_uppercase().chain(title.chars().skip(1)).collect::<String>()
}

impl<C :NetworkClientConn> Game<C> {
	pub fn new(event_loop :&EventLoop<()>,
			srv_conn :C, config :Config, nick_pw :Option<(String, String)>) -> Self {
		let window = glutin::window::WindowBuilder::new()
			.with_title(&title());
		let context = glutin::ContextBuilder::new().with_depth_buffer(24);
		let display = glium::Display::new(window, context, event_loop).unwrap();

		let mut map = ClientMap::new();
		let camera = Camera::new();

		let program = glium::Program::from_source(&display, VERTEX_SHADER_SRC,
			FRAGMENT_SHADER_SRC, None).unwrap();

		let (meshgen_s, meshgen_r) = channel();
		let (meshres_s, meshres_r) = channel();


		map.register_on_change(Box::new(move |chunk_pos, chunk| {
			meshgen_s.send((chunk_pos, chunk.clone())).unwrap();
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
			params : None,
			ui_colors : None,
			texture_id_cache : None,
			texture_array : None,

			meshgen_spawner : Some(Box::new(move |cache| {
				thread::spawn(move || {
					let cache = cache;
					while let Ok((p, chunk)) = meshgen_r.recv() {
						//let start = Instant::now();
						let mesh = mesh_for_chunk(p, &chunk, &cache);
						let _ = meshres_s.send((p, mesh));
						//println!("Generated mesh in {:?}", Instant::now() - start);
					}
				});
			})),
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
			chest_menu : None,
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
		let float_delta = (cur_time - self.last_frame_time).as_secs_f32();
		self.last_frame_time = cur_time;

		const EPS :f32 = 0.1;
		const FPS_TGT :f32 = 60.0;
		let fps_cur_term = if float_delta > 0.0 {
			1.0 / float_delta
		} else {
			// At the beginning float_delta can be zero
			// and 1/0 would fuck up the last_fps value
			FPS_TGT * 30.0
		};
		let fps = self.last_fps * (1.0 - EPS) + fps_cur_term * EPS;
		self.last_fps = fps;

		// If we exceed our target FPS by a too high
		// amount, slow a little bit down
		// to avoid 100% CPU
		if fps > 1.5 * FPS_TGT {
			let smooth_delta = 1.0 / fps;
			let delta_tgt = 1.0 / FPS_TGT;
			let time_too_fast = delta_tgt - smooth_delta;
			// Don't slow down by the full time that we were too fast,
			// because then we are guaranteed to undershoot
			// the FPS target in this frame. That's not our goal!
			let micros_to_wait = (time_too_fast * 0.7 * 1_000_000.0) as u64;
			thread::sleep(Duration::from_micros(micros_to_wait));
		}

		float_delta
	}
	fn in_background(&self) -> bool {
		self.chat_window.is_some() ||
			self.inventory_menu.is_some() ||
			self.chest_menu.is_some() ||
			self.menu_enabled
	}
	pub fn run_loop(&mut self, event_loop :&mut EventLoop<()>) {
		let fonts = vec![Font::from_bytes(KENPIXEL).unwrap()];
		let mut glyph_brush = GlyphBrush::new(&self.display, fonts);
		'game_main_loop :loop {
			gen_chunks_around(&mut self.map,
				self.camera.pos.map(|v| v as isize), 4, 2);
			self.render(&mut glyph_brush);
			let float_delta = self.update_fps();
			let close = self.handle_events(event_loop);
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
					ServerToClientMsg::GameParams(params) => {
						let params_arc = Arc::new(params);

						let hash_list = crate::assets::find_uncached_hashes(&params_arc).unwrap();
						let msg = ClientToServerMsg::GetHashedBlobs(hash_list);
						let _ = self.srv_conn.send(msg);

						self.params = Some(params_arc);
					},
					ServerToClientMsg::HashedBlobs(blobs) => {
						if let Some(params) = &self.params {
							crate::assets::store_hashed_blobs(&blobs).unwrap();
							if let Some(spawner) = self.meshgen_spawner.take() {
								let mut assets = Assets::new();
								let cache = TextureIdCache::from_hdl(params, &mut assets.obtainer(params));
								spawner(cache.clone());
								self.texture_id_cache = Some(cache);
								self.ui_colors = Some(UiColors::new(&mut assets));
								let texture_array = assets.into_texture_array(&self.display).unwrap();
								self.texture_array = Some(texture_array);
							} else {
								// TODO print a warning about duplicate GameParams or sth
							}
						}
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
					ServerToClientMsg::SetCraftInventory(inv) => {
						self.craft_inv = inv;
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
				self.display.gl_window().window().set_cursor_position(PhysicalPosition {
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
		let params = if let Some(p) = &self.params {
			p
		} else {
			return Vector3::new(0.0, 0.0, 0.0);
		};
		for x in cubes_min.x .. cubes_max.x {
			for y in cubes_min.y .. cubes_max.y {
				for z in cubes_min.z .. cubes_max.z {
					let p = Vector3::new(x, y, z);
					if self.map.get_blk(p)
							.and_then(|v| params.get_block_params(v))
							.map(|v| v.solid) == Some(false) {
						continue;
					}
					cubes.push(p);
				}
			}
		}
		let player_pos = self.camera.pos - Vector3::new(0.35, 0.35, 1.4);
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
				// Start a jump
				self.camera.jump_offs = Some(0.0);
			}
		} else if self.camera.jump_offs.is_none() {
			let gravity = Vector3::new(0.0, 0.0, -9.81);
			self.camera.velocity += gravity * 3.0 * time_delta;
			// Maximum falling speed
			const MAX_FALLING_SPEED :f32 = 40.0;
			self.camera.velocity.z = clamp(self.camera.velocity.z, -MAX_FALLING_SPEED, 0.0);
		}
		//delta_pos.try_normalize_mut(std::f32::EPSILON);
		delta_pos
	}
	fn handle_jump(&mut self, time_delta :f32) -> Vector3<f32> {
		// Duration of a jump in seconds
		const JUMP_ANIM_END :f32 = 0.15;
		// The height of the jump at its highest point
		let jump_height = Vector3::new(0.0, 0.0, 1.5);
		// Handle jump end
		if self.camera.jump_offs.iter().any(|v| *v > JUMP_ANIM_END) {
			self.camera.jump_offs = None;
		}
		// Handle jump itself
		if let Some(jump_offs) = self.camera.jump_offs.as_mut()  {
			let new_jump_offs = *jump_offs + time_delta;
			// The jump curve (upwards). The function needs to
			// start at positive value <1.0 and end at 1.0.
			// We do a "smooth" jump by not immediately
			// going upwards at top speed but instead use a parabola
			fn delta_fn(offs :f32) -> f32 {
				let offs = offs / JUMP_ANIM_END + 1.0;
				offs * offs * (1.0 / 4.0)
			}
			let jump_delta = delta_fn(new_jump_offs) - delta_fn(*jump_offs);
			*jump_offs = new_jump_offs;
			return jump_height * jump_delta;
		}
		Vector3::new(0.0, 0.0, 0.0)
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
		delta_pos += self.handle_jump(time_delta);
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
		let texture_array = if let Some(texture_array) = &self.texture_array {
			texture_array
		} else {
			return;
		};
		let texture_arr = texture_array.sampled()
			.wrap_function(SamplerWrapFunction::Repeat)
			.magnify_filter(MagnifySamplerFilter::Nearest);
		// building the uniforms
		let uniforms = uniform! {
			vmatrix : vmatrix,
			pmatrix : pmatrix,
			texture_arr : texture_arr,
			fog_near_far : [self.config.fog_near, self.config.fog_far]
		};
		self.selected_pos = self.params.as_ref().and_then(|params| self.camera.get_selected_pos(&self.map, params));
		let mut sel_text = "sel = None".to_string();
		let mut selbuff = Vec::new();
		if let (Some((selected_pos, _)), Some(ui_colors)) = (self.selected_pos, &self.ui_colors) {
			let blk = self.map.get_blk(selected_pos).unwrap();
			let blk_name = if let Some(n) = self.params
					.as_ref()
					.and_then(|p| p.name_id_map.get_name(blk)) {
				n.to_owned()
			} else {
				format!("{:?}", blk)
			};
			sel_text = format!("sel = ({}, {}, {}), {}",
				selected_pos.x, selected_pos.y, selected_pos.z, blk_name);

			// TODO: only update if the position actually changed from the prior one
			// this spares us needless chatter with the GPU
			let digging = self.camera.dig_cooldown.is_some();
			let vertices = selection_mesh(selected_pos, digging, &ui_colors);
			let vbuff = VertexBuffer::new(&self.display, &vertices).unwrap();
			selbuff = vec![vbuff];
		}
		let mut pl_buf = Vec::new();
		if let (Some((own_id, positions)), Some(ui_colors)) = (&self.player_positions, &self.ui_colors) {
			for (id, pos) in positions {
				if id == own_id {
					continue;
				}
				let v = player_mesh(*pos, &ui_colors);
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
		let vbuffs_to_draw = self.vbuffs.iter()
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
			.collect::<Vec<_>>();
		let vbuffs_to_draw_iter = vbuffs_to_draw.iter()
			.map(|m| &m.0)
			.chain(vbuffs_to_draw.iter().filter_map(|m| m.1.as_ref()));
		for buff in vbuffs_to_draw_iter
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
			let hand_mesh_pos = Vector3::new(3.0, 1.0, -1.5) * 2.0;
			if let (Some(item), Some(texture_id_cache)) = (self.sel_inventory.get_selected(), &self.texture_id_cache) {
				let hand_mesh = hand_mesh(hand_mesh_pos, item, texture_id_cache);
				let vbuff = VertexBuffer::new(&self.display, &hand_mesh).unwrap();
				target.draw(&vbuff,
					&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
					&self.program, &uniforms, &params).unwrap();
			}
		}
		if let (Some(params), Some(ui_colors), Some(tid_cache)) = (&self.params, &self.ui_colors, &self.texture_id_cache) {
			render_inventory_hud(
				&self.sel_inventory,
				ui_colors,
				tid_cache,
				&mut self.display,
				&self.program, glyph_brush,
				params, &mut target);
		}
		if self.in_background() {
			if let (true, Some(ui_colors)) = (self.menu_enabled, &self.ui_colors) {
				render_menu(ui_colors, &mut self.display, &self.program, glyph_brush, &mut target);
			} else if let (Some(cw), Some(ui_colors)) = (&self.chat_window, &self.ui_colors) {
				cw.render(ui_colors, &mut self.display, &self.program, glyph_brush, &mut target);
			} else if let (Some(m), Some(ui_colors), Some(tid_cache)) = (&mut self.inventory_menu, &self.ui_colors, &self.texture_id_cache) {
				m.render(
					ui_colors,
					tid_cache,
					&mut self.display,
					&self.program, glyph_brush, &mut target);
				let command = m.check_event();
				maybe_inventory_change!(m, self, command);
			} else if let (Some(m), Some(ui_colors), Some(tid_cache)) = (&mut self.chest_menu, &self.ui_colors, &self.texture_id_cache) {
				m.render(
					ui_colors,
					tid_cache,
					&mut self.display,
					&self.program, glyph_brush, &mut target);
				let command = m.check_movement();
				maybe_chest_inventory_change!(m, self, command);
			}
		} else if let Some(ui_colors) = &self.ui_colors {
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
			let vertices_horiz = square_mesh((20, 2), screen_dims, ui_colors.crosshair_color);
			let vertices_vert = square_mesh((2, 20), screen_dims, ui_colors.crosshair_color);
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
			let vbuff = VertexBuffer::new(&self.display, &m.intransparent).unwrap();
			let vbuff_t = if m.transparent.len() > 0 {
				Some(VertexBuffer::new(&self.display, &m.transparent).unwrap())
			} else {
				None
			};
			self.vbuffs.insert(p, (vbuff, vbuff_t));
		}
	}

	fn check_grab_change(&mut self) {
		let grabbing_cursor = self.has_focus &&
			!self.in_background() && self.grab_cursor;
		if self.grabbing_cursor != grabbing_cursor {
			self.display.gl_window().window().set_cursor_visible(!grabbing_cursor);
			let _  = self.display.gl_window().window().set_cursor_grab(grabbing_cursor);
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
			Some(VirtualKeyCode::Q) if input.modifiers.ctrl() => {
				return true;
			},
			_ => (),
		}

		if let Some(ev) = (&mut self.chat_window).as_mut().map(|w| w.handle_kinput(input)) {
			self.handle_chat_win_ev(ev);
			return false;
		}
		if input.virtual_keycode == Some(VirtualKeyCode::Escape) {
			if let Some(m) = self.inventory_menu.take() {
				maybe_inventory_change!(m, self, None);

				self.check_grab_change();
				return false;
			} else if let Some(m) = self.chest_menu.take() {
				maybe_chest_inventory_change!(m, self, None);

				self.check_grab_change();
				return false;
			}
		}

		match input.virtual_keycode {
			Some(VirtualKeyCode::Escape) => {
				if input.state == ElementState::Pressed {
					self.menu_enabled = !self.menu_enabled;
					self.check_grab_change();
				}
			},
			Some(VirtualKeyCode::I) => {
				if input.state == ElementState::Pressed {
					if let Some(m) = self.inventory_menu.take() {
						maybe_inventory_change!(m, self, None);
					} else {
						// TODO unwrap below is a bit bad because players might
						// want to open inventory before the server has sent the params
						self.inventory_menu = Some(InventoryMenu::new(
							self.params.as_ref().unwrap().clone(),
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
		let params = if let Some(params) = &self.params {
			params
		} else {
			return
		};
		if !self.camera.mouse_left_down {
			self.camera.dig_cooldown = None;
		}
		const RIGHT_BUTTON_COOLDOWN :f32 = 0.2;
		self.camera.dig_cooldown.as_mut().map(|(_p, c)| *c -= float_delta);
		self.camera.mouse_right_cooldown -= float_delta;
		if let Some((selected_pos, before_selected)) = self.selected_pos {
			if self.camera.mouse_left_down {
				match &mut self.camera.dig_cooldown {
					// End the digging interaction
					Some((pos, dc)) => if *pos != selected_pos {
						self.camera.dig_cooldown = None;
					} else if *dc <= 0.0 {
						let mut blk = self.map.get_blk_mut(*pos).unwrap();
						let drops = params.get_block_params(blk.get()).unwrap().drops;
						self.sel_inventory.put(drops);
						let air_bl = params.block_roles.air;
						blk.set(air_bl);
						let msg = ClientToServerMsg::Dig(selected_pos);
						let _ = self.srv_conn.send(msg);
					},
					// Start new digging interaction
					v @ None => {
						let blk = self.map.get_blk(selected_pos).unwrap();
						let dig_group = params.get_block_params(blk).unwrap().dig_group;
						let sel = self.sel_inventory.get_selected();
						let try_tool_groups = |tool_groups :&[ToolGroup]| {
							let tgs = tool_groups.iter()
								.filter(|g| g.group == dig_group.0 || g.group == DigGroup::any());
							for tg in tgs {
								// Only start digging if hardness is below or at the tool theshold
								if dig_group.1 <= tg.hardness {
									return Some((0.01 + 1.0/tg.speed) as f32);
								}
							}
							None
						};
						// Set block specific cooldown:
						// 1. Try if the currently selected tool supports the group
						let tool_cooldown = if let Some(sel) = sel {
							let tool_groups = &params.get_block_params(sel).unwrap().tool_groups;
							try_tool_groups(tool_groups)
						} else {
							None
						};
						// 2. Try the groups of the bare hand
						let cooldown = tool_cooldown.or_else(|| {
							try_tool_groups(&params.hand_tool_groups)
						});
						// If any of the groups matched:
						if let Some(cooldown) = cooldown {
							*v = Some((selected_pos, cooldown));
						}
					},
				}
			}
			if self.camera.mouse_right_down
					&& self.camera.mouse_right_cooldown <= 0.0 {
				let blk_sel = self.map.get_blk(selected_pos).unwrap();
				let has_inv = params.get_block_params(blk_sel).unwrap().inventory;

				if let (Some(stack_num), false) = (has_inv, self.camera.down_pressed) {
					// open chest inventory
					let chest_inv = self.map.get_blk_meta(selected_pos).unwrap()
						.map(|v| {
							let MetadataEntry::Inventory(inv) = v.clone();
							inv
						})
						.unwrap_or_else(|| SelectableInventory::empty_with_size(stack_num as usize));
					self.chest_menu = Some(ChestMenu::new(
						self.params.as_ref().unwrap().clone(),
						self.sel_inventory.clone(),
						chest_inv,
						selected_pos));
					self.camera.mouse_right_cooldown = RIGHT_BUTTON_COOLDOWN;
					self.camera.mouse_right_down = false;
					self.check_grab_change();
					return;
				}

				let sel = self.sel_inventory.get_sel_idx_and_content();
				if let Some((sel_idx, sel)) = sel {
					let placeable = params.get_block_params(sel).unwrap().placeable;
					if placeable {
						let taken = self.sel_inventory.take_selected();
						assert_eq!(taken, Some(sel));
						let mut blk = self.map.get_blk_mut(before_selected).unwrap();
						blk.set(sel);
						let msg = ClientToServerMsg::PlaceBlock(before_selected, sel_idx, sel);
						let _ = self.srv_conn.send(msg);
						self.camera.mouse_right_cooldown = RIGHT_BUTTON_COOLDOWN;
					}
				}
			}
		}
	}
	fn handle_events(&mut self, event_loop :&mut EventLoop<()>) -> bool {
		let mut close = false;
		event_loop.run_return(|event, _, cflow| {
			match event {
				Event::WindowEvent { event, .. } => match event {

					WindowEvent::Focused(focus) => {
						self.has_focus = focus;
						self.check_grab_change();
					},

					WindowEvent::CloseRequested => close = true,

					WindowEvent::Resized(glutin::dpi::PhysicalSize {width, height}) => {
						self.swidth = width.into();
						self.sheight = height.into();
						self.camera.aspect_ratio = (width / height) as f32;
					},
					WindowEvent::KeyboardInput { input, .. } => {
						close |= self.handle_kinput(&input);
					},
					WindowEvent::ReceivedCharacter(ch) => {
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
					WindowEvent::CursorMoved { position, .. } => {
						if self.has_focus && !self.in_background() {
							if self.grab_cursor {
								self.last_pos = Some(PhysicalPosition {
									x : self.swidth / 2.0,
									y : self.sheight / 2.0,
								});
							}

							if let Some(last) = self.last_pos {
								let delta = PhysicalPosition {
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
							} else if let Some(m) = &mut self.chest_menu {
								m.handle_mouse_moved(position);
							}
						}
					},
					WindowEvent::MouseInput { state, button, .. } => {
						if !self.in_background() {
							let pressed = state == ElementState::Pressed;
							if button == MouseButton::Left {
								self.camera.handle_mouse_left(pressed);
							} else if button == MouseButton::Right {
								self.camera.handle_mouse_right(pressed);
							}
							if let Some((_selected_pos, before_selected))
									= self.selected_pos {
								if pressed && button == MouseButton::Middle {
									let msg = ClientToServerMsg::PlaceTree(before_selected);
									let _ = self.srv_conn.send(msg);
								}
							}
						}
						if self.has_focus {
							if let Some(m) = &mut self.inventory_menu {
								m.handle_mouse_input(state, button);
							} else if let Some(m) = &mut self.chest_menu {
								m.handle_mouse_input(state, button);
							}
						}
					},
					WindowEvent::MouseWheel { delta, .. } => {
						if !self.in_background() {
							let lines_diff = match delta {
								MouseScrollDelta::LineDelta(_x, y) => y,
								MouseScrollDelta::PixelDelta(p) => p.y as f32,
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
				Event::MainEventsCleared => {
					*cflow = ControlFlow::Exit;
				},
				_ => (),
			}
		});
		close
	}
}

fn selection_mesh(pos :Vector3<isize>, digging :bool, ui_colors :&UiColors) -> Vec<Vertex> {
	const DELTA :f32 = 0.05;
	const DELTAH :f32 = DELTA / 2.0;
	let mut vertices = Vec::new();

	let color = if digging {
		ui_colors.block_selection_color_digging
	} else {
		ui_colors.block_selection_color
	};
	let texture_ids = BlockTextureIds::uniform(color);

	push_block(&mut vertices,
		[pos.x as f32 - DELTAH, pos.y as f32 - DELTAH, pos.z as f32 - DELTAH],
		texture_ids, 1.0 + DELTA, |_| false);
	vertices
}

fn player_mesh(pos :PlayerPosition, ui_colors :&UiColors) -> Vec<Vertex> {
	let yaw = -dtr(pos.yaw());
	let pitch = dtr(pos.pitch());
	let pos = pos.pos();
	let mut vertices = Vec::new();

	let texture_ids_body = BlockTextureIds::uniform(ui_colors.color_body);
	let texture_ids_head = BlockTextureIds::uniform(ui_colors.color_head);
	let texture_ids_eyes = BlockTextureIds::uniform(ui_colors.color_eyes);

	let cx = -0.4;
	let cy = -0.4;
	push_block(&mut vertices,
		[cx, cy, -1.6 - 0.4],
		texture_ids_body, 0.8, |_| false);
	push_block(&mut vertices,
		[cx, cy, -0.8 - 0.4],
		texture_ids_body, 0.8, |_| false);

	let head_start = vertices.len();

	push_block(&mut vertices,
		[-0.5*0.78, -0.5*0.78, - 0.4],
		texture_ids_head, 0.78, |_| false);

	push_block(&mut vertices,
		[cx + 0.65, cy + 0.15, -0.1],
		texture_ids_eyes, 0.2, |_| false);
	push_block(&mut vertices,
		[cx + 0.65, cy + 0.45, -0.1],
		texture_ids_eyes, 0.2, |_| false);

	let translation = Translation3::new(pos.x, pos.y, pos.z);
	let rotation = Rotation3::from_euler_angles(0.0, 0.0, yaw);
	let pitch_rotation = Rotation3::from_euler_angles(0.0, pitch, 0.0);
	let iso = translation * rotation;
	vertices[head_start .. ].iter_mut().for_each(|v| {
		let p :Point3<f32> = v.position.into();
		let p = pitch_rotation * p;
		v.position = [p.x, p.y, p.z];
		// TODO also rotate the normal
	});
	vertices.iter_mut().for_each(|v| {
		let p :Point3<f32> = v.position.into();
		let p = iso * p;
		v.position = [p.x, p.y, p.z];
		// TODO also rotate the normal
	});
	vertices
}

fn hand_mesh(pos :Vector3<f32>, blk :MapBlock,
		texture_id_cache :&TextureIdCache) -> Vec<Vertex> {
	let mut vertices = Vec::new();
	let texture_ids = if let Some(ids) = texture_id_cache.get_bl_tex_ids(&blk) {
		ids
	} else {
		return vec![];
	};

	push_block(&mut vertices,
		[pos.x, pos.y, pos.z],
		texture_ids, 1.0, |_| false);
	vertices
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

/// Degrees to radians
fn dtr(v :f32) -> f32 {
	v.to_radians()
}

struct Camera {
	aspect_ratio :f32,
	pitch :f32,
	yaw :f32,
	pos :Vector3<f32>,
	velocity :Vector3<f32>,
	jump_offs :Option<f32>,

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
	dig_cooldown :Option<(Vector3<isize>, f32)>,
}

impl Camera {
	fn new() -> Self {
		Camera {
			aspect_ratio : 1024.0 / 768.0,
			pitch : 0.0,
			yaw : 0.0,
			pos : Vector3::new(60.0, 40.0, 20.0),
			velocity : Vector3::new(0.0, 0.0, 0.0),
			jump_offs : None,

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
			mouse_right_cooldown : 0.0,
			dig_cooldown : None,
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
			VirtualKeyCode::W => b = Some(&mut self.forward_pressed),
			VirtualKeyCode::A => b = Some(&mut self.left_pressed),
			VirtualKeyCode::S => b = Some(&mut self.backward_pressed),
			VirtualKeyCode::D => b = Some(&mut self.right_pressed),
			VirtualKeyCode::Space => b = Some(&mut self.up_pressed),
			VirtualKeyCode::LShift => b = Some(&mut self.down_pressed),
		_ => (),
		}
		if key == VirtualKeyCode::E {
			self.fast_pressed = input.state == ElementState::Pressed;
		}
		if key == VirtualKeyCode::K {
			if input.state == ElementState::Pressed {
				self.fly_mode = !self.fly_mode;
			}
		}
		if key == VirtualKeyCode::J {
			if input.state == ElementState::Pressed {
				self.fast_mode = !self.fast_mode;
			}
		}
		if key == VirtualKeyCode::H {
			if input.state == ElementState::Pressed {
				self.noclip_mode = !self.noclip_mode;
			}
		}

		if let Some(b) = b {
			*b = input.state == ElementState::Pressed;
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
	fn handle_mouse_move(&mut self, delta :PhysicalPosition<f64>) {
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
		self.yaw = (self.yaw + 180.0).rem_euclid(360.0) - 180.0;
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

	pub fn get_selected_pos<B :MapBackend>(&self, map :&Map<B>, params :&GameParamsHdl) -> Option<(Vector3<isize>, Vector3<isize>)> {
		for (vs, ve) in VoxelWalker::new(self.pos,
				self.direction().coords) {
			let vs = vs.map(|v| v.floor() as isize);
			let ve = ve.map(|v| v.floor() as isize);
			if let Some(blk) = map.get_blk(ve) {
				if params.get_pointability_for_blk(&blk) {
					return Some((ve, vs));
				}
			}
		}
		None
	}
}
