extern crate noise;
extern crate nalgebra;
extern crate ncollide3d;
extern crate nphysics3d;
#[macro_use]
extern crate glium;
extern crate winit;
extern crate glium_glyph;
extern crate line_drawing;
extern crate num_traits;
extern crate frustum_query;
extern crate rand_pcg;
extern crate rand;
#[macro_use]
extern crate lazy_static;

mod map;

use map::{Map, MapChunkData, spawn_tree, CHUNKSIZE, MapBlock};
use glium::{glutin, Surface, VertexBuffer};
use nalgebra::{Vector3, Matrix4, Point3, Rotation3, Isometry3};
use num_traits::identities::Zero;
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	rusttype::Font, Section, Layout, HorizontalAlign,
};
use line_drawing::{VoxelOrigin, WalkVoxels};
use std::collections::HashMap;
use std::time::{Instant, Duration};
use std::thread;
use std::sync::mpsc::{channel, Receiver};
use frustum_query::frustum::Frustum;
use ncollide3d::shape::{Cuboid, Compound, ShapeHandle};
use ncollide3d::math::Isometry;
use nphysics3d::math::Inertia;
use nphysics3d::volumetric::Volumetric;
use nphysics3d::world::World;
use nphysics3d::object::{BodyHandle, BodyMut, ColliderHandle, Material};

fn main() {
	let mut events_loop = glutin::EventsLoop::new();
	let mut game = Game::new(&events_loop);
	game.run_loop(&mut events_loop);
}

type MeshResReceiver = Receiver<(Vector3<isize>, Option<Compound<f32>>, Vec<Vertex>)>;

fn gen_chunks_around(map :&mut Map, pos :Vector3<isize>, xyradius :isize, zradius :isize) {
	let chunk_pos = btchn(pos);
	let radius = Vector3::new(xyradius, xyradius, zradius) * CHUNKSIZE;
	let chunk_pos_min = btchn(chunk_pos - radius);
	let chunk_pos_max = btchn(chunk_pos + radius);
	map.gen_chunks_in_area(chunk_pos_min, chunk_pos_max);
}

const VERTEX_SHADER_SRC :&str = r#"
	#version 140
	in vec3 position;
	in vec4 color;

	out vec4 vcolor;

	uniform mat4 pmatrix;
	uniform mat4 vmatrix;
	void main() {
		vcolor = color;
		gl_Position = pmatrix * vmatrix * vec4(position, 1.0);
	}
"#;

const FRAGMENT_SHADER_SRC :&str = r#"
	#version 140
	in vec4 vcolor;

	out vec4 fcolor;

	void main() {
		fcolor = vcolor;
	}
"#;

const KENPIXEL :&[u8] = include_bytes!("../assets/kenney-pixel.ttf");

struct Game {

	physics_world :World<f32>,
	player_handle :BodyHandle,
	player_collider :ColliderHandle,

	meshres_r :MeshResReceiver,

	display :glium::Display,
	program :glium::Program,
	vbuffs :HashMap<Vector3<isize>, (Option<ColliderHandle>, VertexBuffer<Vertex>)>,

	selected_pos :Option<(Vector3<isize>, Vector3<isize>)>,

	last_pos :Option<winit::dpi::LogicalPosition>,

	last_frame_time :Instant,
	last_fps :f32,

	grab_cursor :bool,
	grabbing_cursor :bool,
	has_focus :bool,
	menu_enabled :bool,

	map :Map,
	camera :Camera,

	swidth :f64,
	sheight :f64,
}

impl Game {
	pub fn new(events_loop :&glutin::EventsLoop) -> Self {
		let window = glutin::WindowBuilder::new()
			.with_title("Mehlon");
		let context = glutin::ContextBuilder::new().with_depth_buffer(24);
		let display = glium::Display::new(window, context, events_loop).unwrap();

		let mut map = Map::new(78);
		let camera = Camera::new();

		let program = glium::Program::from_source(&display, VERTEX_SHADER_SRC,
			FRAGMENT_SHADER_SRC, None).unwrap();

		let (meshgen_s, meshgen_r) = channel();
		let (meshres_s, meshres_r) = channel();
		thread::spawn(move || {
			while let Ok((p, chunk)) = meshgen_r.recv() {
				let v = Instant::now();
				let mut shapes = Vec::new();
				let mesh = mesh_for_chunk(p, &chunk, |p :Vector3<isize>| {
					let iso = Isometry3::new(p.map(|v| v as f32).into(), nalgebra::zero());
					let cuboid = ShapeHandle::new(Cuboid::new(Vector3::new(0.5, 0.5, 0.5)));
					shapes.push((iso, cuboid));
				});
				let compound = if shapes.len() > 0 {
					Some(Compound::new(shapes))
				} else {
					None
				};
				let _ = meshres_s.send((p, compound, mesh));
				println!("generating mesh took {:?}", Instant::now() - v);
			}
		});

		map.register_on_change(Box::new(move |chunk_pos, chunk| {
			meshgen_s.send((chunk_pos, chunk.data)).unwrap();
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
		let player_collider = physics_world.add_collider(0.01,
			player_shape, player_handle, nalgebra::one(), material);

		Game {
			physics_world,
			player_handle,
			player_collider,

			meshres_r,

			display,
			program,
			vbuffs : HashMap::new(),

			selected_pos : None,

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
	fn run_loop(&mut self, events_loop :&mut glutin::EventsLoop) {
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
		if self.camera.fast_mode {
			const DELTA :f32 = 40.0;
			delta_pos *= DELTA;
		} else {
			const FAST_DELTA :f32 = 10.0;
			delta_pos *= FAST_DELTA;
		}
		if self.camera.noclip_mode {
			self.camera.pos += delta_pos * time_delta;
			let player_body = self.physics_world.body_mut(self.player_handle);
			match player_body {
				BodyMut::RigidBody(b) => {
					let pos = self.camera.pos - Vector3::new(0.0, 0.0, 1.6);
					b.set_position(Isometry3::new(pos, nalgebra::zero()))
				},
				_ => panic!("Player is expected to be a RigidBody!"),
			}
		} else {
			let player_body = self.physics_world.body_mut(self.player_handle);
			match player_body {
				BodyMut::RigidBody(b) => {
					let pos = b.position().translation.vector;
					b.set_linear_velocity(delta_pos);
					/*let mut v = b.velocity().linear;
					v.try_normalize_mut(std::f32::EPSILON);
					b.set_linear_velocity(v);
					b.apply_force(&Force3::linear(delta_pos));*/
					// The idea is that the eyes are in the middle of the collision box
					// The collision box is 0.7 in all directions.
					let xyh = 0.35;
					self.camera.pos = pos + Vector3::new(xyh, xyh, 1.6);
				},
				_ => panic!("Player is expected to be a RigidBody!"),
			};
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
		if self.menu_enabled {
			self.render_menu(glyph_brush, &mut target);
		}

		target.finish().unwrap();
	}
	fn render_menu<'a, 'b>(&mut self, glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {
		let screen_dims = self.display.get_framebuffer_dimensions();
		const IDENTITY :[[f32; 4]; 4] = [
			[1.0, 0.0, 0.0, 0.0f32],
			[0.0, 1.0, 0.0, 0.0],
			[0.0, 0.0, 1.0, 0.0],
			[0.0, 0.0, 0.0, 1.0],
		];
		let uniforms = uniform! {
			vmatrix : IDENTITY,
			pmatrix : IDENTITY
		};
		let params = glium::draw_parameters::DrawParameters {
			/*depth : glium::Depth {
				test : glium::draw_parameters::DepthTest::IfLess,
				write : true,
				.. Default::default()
			},
			backface_culling : glium::draw_parameters::BackfaceCullingMode::CullCounterClockwise,*/
			blend :glium::Blend::alpha_blending(),
			//polygon_mode : glium::draw_parameters::PolygonMode::Line,
			.. Default::default()
		};
		let vertices = square_mesh();
		let vbuff = VertexBuffer::new(&self.display, &vertices).unwrap();
		target.draw(&vbuff,
				&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
				&self.program, &uniforms, &params).unwrap();
		let f = 1.0 / 2.0 - 0.02;
		glyph_brush.queue(Section {
			text : "Menu\nPress esc to continue Game",
			bounds : (screen_dims.0 as f32 * 0.14, screen_dims.1 as f32),
			screen_position : (screen_dims.0 as f32 * f, screen_dims.1 as f32 * f),
			layout : Layout::default()
				.h_align(HorizontalAlign::Center),
			color : [0.9, 0.9, 0.9, 1.0],
			.. Section::default()
		});
		glyph_brush.draw_queued(&self.display, target);
	}

	fn recv_vbuffs(&mut self) {
		while let Ok((p, c, m)) = self.meshres_r.try_recv() {
			let material = Material::new(0.0, 0.0);
			let collider = c.map(|c| {
				let hdl = ShapeHandle::new(c);
				self.physics_world.add_collider(0.01, hdl,
					BodyHandle::ground(), nalgebra::one(), material)
			});
			let vbuff = VertexBuffer::new(&self.display, &m).unwrap();
			let old_opt = self.vbuffs.insert(p, (collider, vbuff));
			if let Some((Some(coll), _)) = old_opt {
				self.physics_world.remove_colliders(&[coll]);
			}
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
								} else if button == glutin::MouseButton::Right {
									let mut blk = self.map.get_blk_mut(before_selected).unwrap();
									blk.set(MapBlock::Wood);
								} else if button == glutin::MouseButton::Middle {
									spawn_tree(&mut self.map, before_selected);
								}
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

#[derive(Copy, Clone)]
struct Vertex {
	position :[f32; 3],
	color :[f32; 4],
}

implement_vertex!(Vertex, position, color);

#[inline]
fn push_block<F :FnMut([isize; 3]) -> bool>(r :&mut Vec<Vertex>, [x, y, z] :[f32; 3], color :[f32; 4], colorh :[f32; 4], siz :f32, mut blocked :F) {
	macro_rules! push_face {
		(($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $color:expr) => {
		r.push(Vertex { position: [$x, $y, $z], color : $color });
		r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color });
		r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color });

		r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color });
		r.push(Vertex { position: [$x + $xsd, $y + $yd + $ysd, $z + $zd], color: $color });
		r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color });
		}
	};
	macro_rules! push_face_rev {
		(($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $color:expr) => {
		r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color });
		r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color });
		r.push(Vertex { position: [$x, $y, $z], color : $color });

		r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color });
		r.push(Vertex { position: [$x + $xsd, $y + $yd + $ysd, $z + $zd], color: $color });
		r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color });
		}
	};
	// X-Y face
	if !blocked([0, 0, -1]) {
		push_face!((x, y, z), (siz, 0.0, siz, 0.0), color);
	}
	// X-Z face
	if !blocked([0, -1, 0]) {
		push_face_rev!((x, y, z), (siz, 0.0, 0.0, siz), colorh);
	}
	// Y-Z face
	if !blocked([-1, 0, 0]) {
		push_face!((x, y, z), (0.0, siz, 0.0, siz), colorh);
	}
	// X-Y face (z+1)
	if !blocked([0, 0, 1]) {
		push_face_rev!((x, y, z + siz), (siz, 0.0, siz, 0.0), color);
	}
	// X-Z face (y+1)
	if !blocked([0, 1, 0]) {
		push_face!((x, y + siz, z), (siz, 0.0, 0.0, siz), colorh);
	}
	// Y-Z face (x+1)
	if !blocked([1, 0, 0]) {
		push_face_rev!((x + siz, y, z), (0.0, siz, 0.0, siz), colorh);
	}
}

fn mesh_for_chunk<F :FnMut(Vector3<isize>)>(offs :Vector3<isize>, chunk :&MapChunkData, mut f :F) ->
		Vec<Vertex> {
	let mut r = Vec::new();
	for x in 0 .. CHUNKSIZE {
		for y in 0 .. CHUNKSIZE {
			for z in 0 .. CHUNKSIZE {
				let mut push_blk = |color :[f32; 4]| {
						let pos = offs + Vector3::new(x, y, z);
						let fpos = [pos.x as f32, pos.y as f32, pos.z as f32];
						let colorh = [color[0]/2.0, color[1]/2.0, color[2]/2.0, color[3]];
						let mut any_non_blocked = false;
						push_block(&mut r, fpos, color, colorh, 1.0, |[xo, yo, zo]| {
							let pos = Vector3::new(x + xo, y + yo, z + zo);
							let outside = pos.map(|v| v < 0 || v >= CHUNKSIZE);
							if outside.x || outside.y || outside.z {
								any_non_blocked = true;
								return false;
							}
							match *chunk.get_blk(pos) {
								MapBlock::Air => {
									any_non_blocked = true;
									false
								},
								_ => true,
							}
						});
						// If any of the faces is unblocked, this block
						// will be reported
						if any_non_blocked {
							f(pos);
						}
				};
				match *chunk.get_blk(Vector3::new(x, y, z)) {
					MapBlock::Air => (),
					MapBlock::Ground => {
						push_blk([0.0, 1.0, 0.0, 1.0]);
					},
					MapBlock::Water => {
						push_blk([0.0, 0.0, 1.0, 1.0]);
					},
					MapBlock::Wood => {
						push_blk([0.5, 0.25, 0.0, 1.0]);
					},
					MapBlock::Stone => {
						push_blk([0.5, 0.5, 0.5, 1.0]);
					},
					MapBlock::Tree => {
						push_blk([0.38, 0.25, 0.125, 1.0]);
					},
					MapBlock::Leaves => {
						push_blk([0.0, 0.4, 0.0, 1.0]);
					},
				}
			}
		}
	}
	r
}

fn square_mesh() -> Vec<Vertex> {
	const COLOR :[f32; 4] = [0.4, 0.4, 0.4, 0.85];
	let mut vertices = Vec::new();

	let size = 0.15;
	let x_min = -size;
	let y_min = -size;
	let x_max = size;
	let y_max = size;
	let z = 0.2;

	vertices.push(Vertex {
		position : [x_min, x_min, z],
		color : COLOR,
	});
	vertices.push(Vertex {
		position : [x_max, y_min, z],
		color : COLOR,
	});
	vertices.push(Vertex {
		position : [x_max, y_max, z],
		color : COLOR,
	});
	vertices.push(Vertex {
		position : [x_max, y_max, z],
		color : COLOR,
	});
	vertices.push(Vertex {
		position : [x_min, y_max, z],
		color : COLOR,
	});
	vertices.push(Vertex {
		position : [x_min, x_min, z],
		color : COLOR,
	});
	vertices
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

/// Block position to chunk position
fn btchn(v :Vector3<isize>) -> Vector3<isize> {
	fn r(x :isize) -> isize {
		let x = x as f32 / (CHUNKSIZE as f32);
		x.floor() as isize * CHUNKSIZE
	}
	v.map(r)
}

/// Block position to position inside chunk
fn btpic(v :Vector3<isize>) -> Vector3<isize> {
	v.map(|v| mod_euc(v as f32, CHUNKSIZE as f32) as isize)
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
		self.pitch = clamp(factor * delta.y as f32 + self.pitch, -89.999, 89.999);
		self.yaw += factor * delta.x as f32;
		self.yaw = mod_euc(self.yaw + 180.0, 360.0) - 180.0;
	}

	fn direction(&self) -> Point3<f32> {
		let pitch = dtr(-self.pitch);
		let yaw = dtr(-self.yaw);
		Point3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin())
	}

	fn get_matrix(&self) -> [[f32; 4]; 4] {
		let m = Matrix4::look_at_rh(&(Point3::origin() + self.pos),
			&(self.direction() + self.pos), &Vector3::z());
		m.into()
	}

	pub fn get_perspective(&self) -> [[f32; 4]; 4] {
		let fov = dtr(90.0);
		let zfar = 1024.0;
		let znear = 0.1;
		Matrix4::new_perspective(self.aspect_ratio, fov, znear, zfar).into()
	}

	pub fn get_selected_pos(&self, map :&Map) -> Option<(Vector3<isize>, Vector3<isize>)> {
		const SELECTION_RANGE :f32 = 10.0;
		let pointing_at_distance = self.pos + self.direction().coords * SELECTION_RANGE;
		let (dx, dy, dz) = (pointing_at_distance.x, pointing_at_distance.y, pointing_at_distance.z);
		let (px, py, pz) = (self.pos.x, self.pos.y, self.pos.z);
		for ((xs, ys, zs), (xe, ye, ze)) in WalkVoxels::<f32, isize>::new((px, py, pz),
				(dx, dy, dz), &VoxelOrigin::Corner).steps() {
			let vs = Vector3::new(xs as isize, ys as isize, zs as isize);
			let ve = Vector3::new(xe as isize, ye as isize, ze as isize);
			if let Some(blk) = map.get_blk(ve) {
				if blk.is_pointable() {
					return Some((ve, vs));
				}
			}
		}
		None
	}
}
