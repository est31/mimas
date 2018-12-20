use mehlon_server::map::{Map, MapBackend, ClientMap, spawn_tree,
	CHUNKSIZE, MapBlock};
use glium::{glutin, Surface, VertexBuffer};
use nalgebra::{Vector3, Matrix4, Point3, Rotation3, Isometry3};
use num_traits::identities::Zero;
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	rusttype::Font, Section,
};
use std::collections::HashMap;
use std::time::{Instant, Duration};
use std::thread;
use std::sync::mpsc::{channel, Receiver};
use frustum_query::frustum::Frustum;
use ncollide3d::shape::{Cuboid, Compound, ShapeHandle};
use ncollide3d::math::Isometry;
use ncollide3d::query;
use nphysics3d::math::Inertia;
use nphysics3d::volumetric::Volumetric;
use nphysics3d::world::World;
use nphysics3d::object::{BodyHandle, BodyMut, ColliderHandle, Material};

use mehlon_server::{btchn, ServerToClientMsg, ClientToServerMsg};
use mehlon_server::generic_net::NetworkClientConn;

use mehlon_meshgen::{Vertex, mesh_compound_for_chunk, push_block};

use ui::{render_menu, square_mesh, IDENTITY};

use voxel_walk::VoxelWalker;

type MeshResReceiver = Receiver<(Vector3<isize>, Option<Compound<f32>>, Vec<Vertex>)>;

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

pub struct Game<C :NetworkClientConn> {
	srv_conn :C,

	physics_world :World<f32>,
	player_handle :BodyHandle,

	meshres_r :MeshResReceiver,

	display :glium::Display,
	program :glium::Program,
	vbuffs :HashMap<Vector3<isize>, (Option<Compound<f32>>, VertexBuffer<Vertex>)>,

	selected_pos :Option<(Vector3<isize>, Vector3<isize>)>,
	item_in_hand :MapBlock,

	last_pos :Option<winit::dpi::LogicalPosition>,

	last_frame_time :Instant,
	last_fps :f32,

	grab_cursor :bool,
	grabbing_cursor :bool,
	has_focus :bool,
	menu_enabled :bool,

	map :ClientMap,
	camera :Camera,

	swidth :f64,
	sheight :f64,
}

impl<C :NetworkClientConn> Game<C> {
	pub fn new(events_loop :&glutin::EventsLoop,
			srv_conn :C) -> Self {
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
				let (mesh, compound) = mesh_compound_for_chunk(p, &chunk);
				let _ = meshres_s.send((p, compound, mesh));
			}
		});

		map.register_on_change(Box::new(move |chunk_pos, chunk| {
			meshgen_s.send((chunk_pos, *chunk)).unwrap();
		}));

		// This ensures that the mesh generation thread puts higher priority onto positions
		// close to the player at the beginning.
		gen_chunks_around(&mut map, camera.pos.map(|v| v as isize), 1, 1);

		let swidth = 1024.0;
		let sheight = 768.0;

		let mut physics_world = World::new();

		let player_collisionbox = Cuboid::new(Vector3::new(0.35, 0.35, 0.9));
		let player_handle = physics_world.add_rigid_body(
			Isometry::new(Vector3::new(60.0, 40.0, 20.0), nalgebra::zero()),
			Inertia::new(1.0, nalgebra::zero()),
			player_collisionbox.center_of_mass());
		let material = Material::new(1.0, 1.0);
		let player_shape = ShapeHandle::new(player_collisionbox);
		let _player_collider = physics_world.add_collider(0.01,
			player_shape, player_handle, nalgebra::one(), material);

		Game {
			srv_conn,

			physics_world,
			player_handle,

			meshres_r,

			display,
			program,
			vbuffs : HashMap::new(),

			selected_pos : None,
			item_in_hand : MapBlock::Wood,

			last_pos : None,
			last_frame_time : Instant::now(),
			last_fps : 0.0,
			grab_cursor : true,
			grabbing_cursor : false,
			has_focus : false,
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
			0.0
		};
		let fps = self.last_fps * (1.0 - EPS) + fps_cur_term * EPS;
		self.last_fps = fps;
		float_delta
	}
	pub fn run_loop(&mut self, events_loop :&mut glutin::EventsLoop) {
		let fonts = vec![Font::from_bytes(KENPIXEL).unwrap()];
		let mut glyph_brush = GlyphBrush::new(&self.display, fonts);
		loop {
			gen_chunks_around(&mut self.map,
				self.camera.pos.map(|v| v as isize), 4, 2);
			self.render(&mut glyph_brush);
			let float_delta = self.update_fps();
			self.physics_world.set_timestep(float_delta);
			self.physics_world.step();
			let close = self.handle_events(events_loop);
			if !self.menu_enabled {
				self.movement(float_delta);
				let msg = ClientToServerMsg::SetPos(self.camera.pos);
				let _ = self.srv_conn.send(msg);

			}
			while let Ok(Some(msg)) = self.srv_conn.try_recv() {
				match msg {
					ServerToClientMsg::ChunkUpdated(p, c) => {
						self.map.set_chunk(p, c);
					},
				}
			}

			if close {
				break;
			}
			if self.grabbing_cursor {
				self.display.gl_window().set_cursor_position(winit::dpi::LogicalPosition {
					x : self.swidth / 2.0,
					y : self.sheight / 2.0,
				}).unwrap();
			}
		}
	}
	fn movement(&mut self, time_delta :f32) {
		let mut delta_pos = self.camera.delta_pos();
		if !self.camera.noclip_mode {
			let pos = self.camera.pos.map(|v| v as isize);
			let chunk_pos = btchn(self.camera.pos.map(|v| v as isize));
			let mut cubes = Vec::new();
			let d = 3;
			for x in -d .. d {
				for y in -d .. d {
					for z in -d .. d {
						let p = pos + Vector3::new(x, y, z);
						match self.map.get_blk(p) {
							Some(MapBlock::Air) => continue,
							None => (),
							Some(_) => (),
						}
						let iso = Isometry3::new(p.map(|v| v as f32).into(), nalgebra::zero());
						let cuboid = Cuboid::new(Vector3::new(0.5, 0.5, 0.5));
						cubes.push((iso, cuboid));
					}
				}
			}
			let player_collisionbox = Cuboid::new(Vector3::new(0.35, 0.35, 0.9));
			let player_pos = Isometry3::new(self.camera.pos, nalgebra::zero());
			print!("ccheck({}): ", cubes.len());
			for (iso, cube) in cubes.iter() {
				const PRED :f32 = 0.01;
				let collision = query::contact(&iso, cube, &player_pos, &player_collisionbox, PRED);
				if let Some(collision) = collision {
					let normal = collision.normal.as_ref();
					let v :[f32;3]  = iso.translation.vector.into();
					let nv :[f32;3]  = (*normal).into();
					print!("collision({:?}, {:?}), ", v, nv);
					//delta_pos.try_normalize_mut(std::f32::EPSILON);
					let d = delta_pos.dot(normal);
					if d < 0.0 {
						delta_pos -= d * normal;
					}
				}
			}
			println!();
		}
		if self.camera.fast_mode {
			const DELTA :f32 = 40.0;
			delta_pos *= DELTA;
		} else {
			const FAST_DELTA :f32 = 10.0;
			delta_pos *= FAST_DELTA;
		}
		self.camera.pos += delta_pos * time_delta;
		let player_body = self.physics_world.body_mut(self.player_handle);
		match player_body {
			BodyMut::RigidBody(b) => {
				let pos = self.camera.pos - Vector3::new(0.0, 0.0, 1.6);
				b.set_position(Isometry3::new(pos, nalgebra::zero()))
			},
			_ => panic!("Player is expected to be a RigidBody!"),
		}
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
			pmatrix : pmatrix
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
		let screen_dims = self.display.get_framebuffer_dimensions();
		// TODO turn off anti-aliasing of the font
		// https://gitlab.redox-os.org/redox-os/rusttype/issues/61
		glyph_brush.queue(Section {
			text : &format!("pos = ({}, {}, {}) pi = {}, yw = {}, {}, FPS: {:1.2}", self.camera.pos.x, self.camera.pos.y,
				self.camera.pos.z, self.camera.pitch,
				self.camera.yaw, sel_text, self.last_fps as u16),
			bounds : (screen_dims.0 as f32, screen_dims.1 as f32),
			//scale : glium_brush::glyph_brush::rusttype::Scale::uniform(32.0),
			color : [0.9, 0.9, 0.9, 1.0],
			.. Section::default()
		});

		let params = glium::draw_parameters::DrawParameters {
			depth : glium::Depth {
				test : glium::draw_parameters::DepthTest::IfLess,
				write : true,
				.. Default::default()
			},
			backface_culling : glium::draw_parameters::BackfaceCullingMode::CullCounterClockwise,
			blend :glium::Blend::alpha_blending(),
			//polygon_mode : glium::draw_parameters::PolygonMode::Line,
			.. Default::default()
		};

		// drawing a frame
		let mut target = self.display.draw();
		target.clear_color_and_depth((0.05, 0.01, 0.6, 0.0), 1.0);

		for buff in self.vbuffs.iter()
				.filter_map(|(p, (_c, m))| {
					// Frustum culling.
					// We approximate chunks as spheres here, as the library
					// has no cube checker.
					let p = p.map(|v| (v + CHUNKSIZE / 2) as f32);
					let r = CHUNKSIZE as f32 * 3.0_f32.sqrt();
					if frustum.sphere_intersecting(&p.x, &p.y, &p.z, &r) {
						Some(m)
					} else {
						None
					}
				})
				.chain(selbuff.iter()) {
			target.draw(buff,
				&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
				&self.program, &uniforms, &params).unwrap();
		}
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
				pmatrix : pmatrix
			};
			let hand_mesh_pos = Vector3::new(3.0, 1.0, -1.5);
			let hand_mesh = hand_mesh(hand_mesh_pos,
				self.item_in_hand);
			let vbuff = VertexBuffer::new(&self.display, &hand_mesh).unwrap();
			target.draw(&vbuff,
				&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
				&self.program, &uniforms, &params).unwrap();
		}
		if self.menu_enabled {
			render_menu(&mut self.display, &self.program, glyph_brush, &mut target);
		} else {
			let params = glium::draw_parameters::DrawParameters {
				blend :glium::Blend::alpha_blending(),
				.. Default::default()
			};

			let uniforms = uniform! {
				vmatrix : IDENTITY,
				pmatrix : IDENTITY
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
		while let Ok((p, c, m)) = self.meshres_r.try_recv() {
			let material = Material::new(0.0, 0.0);
			let vbuff = VertexBuffer::new(&self.display, &m).unwrap();
			let old_opt = self.vbuffs.insert(p, (c, vbuff));
			/*if let Some((Some(coll), _)) = old_opt {
				self.physics_world.remove_colliders(&[coll]);
			}*/
		}
	}

	fn check_grab_change(&mut self) {
		let grabbing_cursor = self.has_focus &&
			!self.menu_enabled && self.grab_cursor;
		if self.grabbing_cursor != grabbing_cursor {
			self.display.gl_window().hide_cursor(grabbing_cursor);
			let _  = self.display.gl_window().grab_cursor(grabbing_cursor);
			self.grabbing_cursor = grabbing_cursor;
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
						match input.virtual_keycode {
							Some(glutin::VirtualKeyCode::Escape) => {
								if input.state == glutin::ElementState::Pressed {
									self.menu_enabled = !self.menu_enabled;
									self.check_grab_change();
								}
							},
							Some(glutin::VirtualKeyCode::Q) if input.modifiers.ctrl => {
								close = true;
							},
							_ => (),
						}
						self.camera.handle_kinput(input);

					},
					glutin::WindowEvent::CursorMoved { position, .. } => {
						if self.has_focus && !self.menu_enabled {
							if self.grab_cursor {
								self.last_pos = Some(winit::dpi::LogicalPosition {
									x : self.swidth / 2.0,
									y : self.sheight / 2.0,
								});
							}

							if let Some(last) = self.last_pos {
								let delta = winit::dpi::LogicalPosition {
									x : position.x - last.x,
									y : position.y - last.y,
								};
								self.camera.handle_mouse_move(delta);
							}
							self.last_pos = Some(position);
						}
					},
					glutin::WindowEvent::MouseInput { state, button, .. } => {
						if state == glutin::ElementState::Pressed && !self.menu_enabled {
							if let Some((selected_pos, before_selected)) = self.selected_pos {
								if button == glutin::MouseButton::Left {
									let mut blk = self.map.get_blk_mut(selected_pos).unwrap();
									blk.set(MapBlock::Air);
									let msg = ClientToServerMsg::SetBlock(selected_pos, MapBlock::Air);
									let _ = self.srv_conn.send(msg);
								} else if button == glutin::MouseButton::Right {
									let mut blk = self.map.get_blk_mut(before_selected).unwrap();
									blk.set(self.item_in_hand);
									let msg = ClientToServerMsg::SetBlock(before_selected, self.item_in_hand);
									let _ = self.srv_conn.send(msg);
								} else if button == glutin::MouseButton::Middle {
									spawn_tree(&mut self.map, before_selected);
									let msg = ClientToServerMsg::PlaceTree(before_selected);
									let _ = self.srv_conn.send(msg);
								}
							}
						}
					},
					glutin::WindowEvent::MouseWheel { delta, .. } => {
						let lines_diff = match delta {
							glutin::MouseScrollDelta::LineDelta(_x, y) => y,
							glutin::MouseScrollDelta::PixelDelta(p) => p.y as f32,
						};
						fn rotate(mb :MapBlock) -> MapBlock {
							use mehlon_server::map::MapBlock::*;
							match mb {
								Water => Ground,
								Ground => Wood,
								Wood => Stone,
								Stone => Leaves,
								Leaves => Tree,
								Tree => Coal,
								Coal => Water,
								_ => unreachable!(),
							}
						}
						if lines_diff < 0.0 {
							self.item_in_hand = rotate(self.item_in_hand);
						} else if lines_diff > 0.0 {
							for _ in 0 .. 6 {
								self.item_in_hand = rotate(self.item_in_hand);
							}
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
	// Soon we can just convert to u128. It's already in FCP.
	// https://github.com/rust-lang/rust/issues/50202
	// Very soon...
	d.as_secs() as f32 + d.subsec_millis() as f32 / 1000.0
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

	forward_pressed :bool,
	left_pressed :bool,
	right_pressed :bool,
	backward_pressed :bool,

	fast_mode :bool,
	noclip_mode :bool,

	up_pressed :bool,
	down_pressed :bool,
}

impl Camera {
	fn new() -> Self {
		Camera {
			aspect_ratio : 1024.0 / 768.0,
			pitch : 0.0,
			yaw : 0.0,
			pos : Vector3::new(60.0, 40.0, 20.0),

			forward_pressed : false,
			left_pressed : false,
			right_pressed : false,
			backward_pressed : false,

			fast_mode : false,
			noclip_mode : false,

			up_pressed : false,
			down_pressed : false,
		}
	}
	fn handle_kinput(&mut self, input :glium::glutin::KeyboardInput) {
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
		if self.up_pressed {
			delta_pos += Vector3::z()
		}
		if self.down_pressed {
			delta_pos -= Vector3::z();
		}
		delta_pos.try_normalize_mut(std::f32::EPSILON);
		delta_pos = Rotation3::from_axis_angle(&Vector3::z_axis(), dtr(-self.yaw)) * delta_pos;

		delta_pos
	}
	fn handle_mouse_move(&mut self, delta :winit::dpi::LogicalPosition) {
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
