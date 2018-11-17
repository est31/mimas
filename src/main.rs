extern crate noise;
extern crate nalgebra;
#[macro_use]
extern crate glium;
extern crate winit;
extern crate glium_glyph;
extern crate line_drawing;
extern crate num_traits;

mod map;

use map::{Map, MapChunk, BLOCKSIZE, MapBlock};
use glium::{glutin, Surface};
use nalgebra::{Vector3, Matrix3, Matrix4, Point3, Rotation3};
use num_traits::identities::Zero;
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	rusttype::{self, Font}, Section,
};
use line_drawing::{VoxelOrigin, WalkVoxels};
use std::collections::HashMap;
use std::time::Instant;

fn main() {
	let mut events_loop = glutin::EventsLoop::new();
	let window = glutin::WindowBuilder::new()
		.with_title("Mehlon");
	let context = glutin::ContextBuilder::new().with_depth_buffer(24);
	let display = glium::Display::new(window, context, &events_loop).unwrap();

	let mut map = Map::new(78);
	map.gen_chunks_start();

	let mut camera = Camera::new();

	let vertex_shader_src = r#"
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

	let fragment_shader_src = r#"
		#version 140
		in vec4 vcolor;

		out vec4 fcolor;

		void main() {
			fcolor = vcolor;
		}
	"#;

	let program = glium::Program::from_source(&display, vertex_shader_src,
		fragment_shader_src, None).unwrap();

	let gen_vbuffs = |display, map :&Map| {
		let v = Instant::now();
		let ret = map.chunks.iter()
			.map(|(p, c)| (p, mesh_for_chunk(*p, c)))
			.map(|(p, m)| (*p, glium::VertexBuffer::new(display, &m).unwrap()))
			.collect::<HashMap<_, _>>();
		println!("generating the meshes took {:?}", Instant::now() - v);
		ret
	};
	let vbuffs_update = |vbuffs :&mut HashMap<Vector3<_>, glium::VertexBuffer<_>>, display, map :&Map, pos :Vector3<isize>| {
		let v = Instant::now();
		let chunk_pos = btchn(pos);
		if let Some(chunk) = map.chunks.get(&chunk_pos) {
			let mesh = mesh_for_chunk(chunk_pos, chunk);
			let vb = glium::VertexBuffer::new(display, &mesh).unwrap();
			vbuffs.insert(chunk_pos, vb);
		}
		println!("regen took {:?}", Instant::now() - v);
	};
	let gen_chunks_around = |vbuffs :&mut HashMap<Vector3<_>, glium::VertexBuffer<_>>, display, map :&mut Map, pos :Vector3<isize>| {
		let v = Instant::now();
		let chunk_pos = btchn(pos);
		let radius = 2;
		for x in -radius .. radius {
			for y in -radius .. radius {
				for z in -radius .. radius {
					let cpos = chunk_pos + Vector3::new(x, y, z) * BLOCKSIZE;
					if map.chunks.get(&cpos).is_none() {
						map.gen_chunk(cpos);
						vbuffs_update(vbuffs, display, map, cpos);
					}
				}
			}
		}
	};
	let mut vbuffs = gen_vbuffs(&display, &map);
	let mut selbuff = Vec::new();

	let grab_cursor = true;

	if grab_cursor {
		display.gl_window().hide_cursor(true);
		display.gl_window().grab_cursor(true).unwrap();
	}

	let kenpixel: &[u8] = include_bytes!("../assets/kenney-pixel.ttf");
	let fonts = vec![Font::from_bytes(kenpixel).unwrap()];
	let mut glyph_brush = GlyphBrush::new(&display, fonts);

	let mut last_pos :Option<winit::dpi::LogicalPosition> = None;
	let mut last_frame_time = Instant::now();
	let mut last_fps = 0.0;
	loop {
		// building the uniforms
		let uniforms = uniform! {
			vmatrix : camera.get_matrix(),
			pmatrix : camera.get_perspective()
		};

		let selected_pos = camera.get_selected_pos(&map);
		let mut sel_text = "sel = None".to_string();
		selbuff.clear();
		if let Some((selected_pos, _)) = selected_pos {
			let blk = map.get_blk(selected_pos).unwrap();
			sel_text = format!("sel = ({}, {}, {}), {:?}", selected_pos.x, selected_pos.y, selected_pos.z, blk);

			// TODO: only update if the position actually changed from the prior one
			// this spares us needless chatter with the GPU
			let vertices = selection_mesh(selected_pos);
			let vbuff = glium::VertexBuffer::new(&display, &vertices).unwrap();
			selbuff = vec![vbuff];
		}
		let cur_time = Instant::now();
		let time_delta = cur_time - last_frame_time;
		last_frame_time = cur_time;
		// Soon we can just convert to u128. It's already in FCP.
		// https://github.com/rust-lang/rust/issues/50202
		// Very soon...
		let float_delta = time_delta.as_secs() as f32 + time_delta.subsec_millis() as f32 / 1000.0;
		const EPS :f32 = 0.1;
		let fps_cur_term = if float_delta > 0.0 {
			1.0 / float_delta
		} else {
			// At the beginning float_delta can be zero
			// and 1/0 would fuck up the last_fps value
			0.0
		};
		let fps = last_fps * (1.0 - EPS) + fps_cur_term * EPS;
		last_fps = fps;

		gen_chunks_around(&mut vbuffs, &display, &mut map, camera.pos.map(|v| v as isize));

		let screen_dims = display.get_framebuffer_dimensions();
		// TODO turn off anti-aliasing of the font
		// https://gitlab.redox-os.org/redox-os/rusttype/issues/61
		glyph_brush.queue(Section {
			text : &format!("pos = ({}, {}, {}) pi = {}, yw = {}, {}, FPS: {:1.2}", camera.pos.x, camera.pos.y,
				camera.pos.z, camera.pitch, camera.yaw, sel_text, fps as u16),
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
		let mut target = display.draw();
		target.clear_color_and_depth((0.05, 0.01, 0.6, 0.0), 1.0);

		for buff in vbuffs.values().chain(selbuff.iter()) {
			target.draw(buff,
				&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
				&program, &uniforms, &params).unwrap();
		}
		glyph_brush.draw_queued(&display, &mut target);

		target.finish().unwrap();

		let mut swidth = 1024.0;
		let mut sheight = 768.0;

		let mut close = false;
		events_loop.poll_events(|event| {
			match event {
				glutin::Event::WindowEvent { event, .. } => match event {

					glutin::WindowEvent::CloseRequested => close = true,

					glutin::WindowEvent::Resized(glium::glutin::dpi::LogicalSize {width, height}) => {
						swidth = width;
						sheight = height;
						camera.aspect_ratio = (width / height) as f32;
					},
					glutin::WindowEvent::KeyboardInput { input, .. } => {
						match input.virtual_keycode {
							Some(glutin::VirtualKeyCode::Escape) => {
								close = true;
							}
							_ => (),
						}
						camera.handle_kinput(input);

					},
					glutin::WindowEvent::CursorMoved { position, .. } => {
						if grab_cursor {
							last_pos = Some(winit::dpi::LogicalPosition {
								x : swidth / 2.0,
								y : sheight / 2.0,
							});
						}
						if let Some(last) = last_pos {
							let delta = winit::dpi::LogicalPosition {
								x : position.x - last.x,
								y : position.y - last.y,
							};
							camera.handle_mouse_move(delta);
						}
						last_pos = Some(position);
					},
					glutin::WindowEvent::MouseInput { state, button, .. } => {
						if state == glutin::ElementState::Pressed {
							if let Some((selected_pos, before_selected)) = selected_pos {
								let mut pos_to_update = None;
								if button == glutin::MouseButton::Left {
									let blk = map.get_blk_mut(selected_pos).unwrap();
									*blk = MapBlock::Air;
									pos_to_update = Some(selected_pos);
								} else if button == glutin::MouseButton::Right {
									let blk = map.get_blk_mut(before_selected).unwrap();
									*blk = MapBlock::Wood;
									pos_to_update = Some(before_selected);
								}
								if let Some(pos) = pos_to_update {
									vbuffs_update(&mut vbuffs, &display, &map, selected_pos);
								}
							}
						}
					},

					_ => (),
				},
				_ => (),
			}
		});
		camera.tick(float_delta);
		if close {
			break;
		}
		if grab_cursor {
			display.gl_window().set_cursor_position(winit::dpi::LogicalPosition {
				x : swidth / 2.0,
				y : sheight / 2.0,
			});
		}
	}
}

#[derive(Copy, Clone)]
struct Vertex {
	position :[f32; 3],
	color :[f32; 4],
}

implement_vertex!(Vertex, position, color);

#[inline]
fn push_block(r :&mut Vec<Vertex>, [x, y, z] :[f32; 3], color :[f32; 4], colorh :[f32; 4], siz :f32) {
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
	push_face!((x, y, z), (siz, 0.0, siz, 0.0), color);
	// X-Z face
	push_face_rev!((x, y, z), (siz, 0.0, 0.0, siz), colorh);
	// Y-Z face
	push_face!((x, y, z), (0.0, siz, 0.0, siz), colorh);
	// X-Y face (z+1)
	push_face_rev!((x, y, z + siz), (siz, 0.0, siz, 0.0), color);
	// X-Z face (y+1)
	push_face!((x, y + siz, z), (siz, 0.0, 0.0, siz), colorh);
	// Y-Z face (x+1)
	push_face_rev!((x + siz, y, z), (0.0, siz, 0.0, siz), colorh);
}

fn mesh_for_chunk(offs :Vector3<isize>, chunk :&MapChunk) ->
		Vec<Vertex> {
	let mut r = Vec::new();
	for x in 0 .. BLOCKSIZE {
		for y in 0 .. BLOCKSIZE {
			for z in 0 .. BLOCKSIZE {
				let mut push_blk = |color, colorh| {
						let pos = [offs.x as f32 + x as f32, offs.y as f32 + y as f32, offs.z as f32 + z as f32];
						push_block(&mut r, pos, color, colorh, 1.0);
				};
				match *chunk.get_blk(Vector3::new(x, y, z)) {
					MapBlock::Air => (),
					MapBlock::Ground => {
						push_blk([0.0, 1.0, 0.0, 1.0], [0.0, 0.5, 0.0, 1.0]);
					},
					MapBlock::Water => {
						push_blk([0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 0.5, 1.0]);
					},
					MapBlock::Wood => {
						push_blk([0.5, 0.25, 0.0, 1.0], [0.25, 0.125, 0.0, 1.0]);
					},
					MapBlock::Stone => {
						push_blk([0.5, 0.5, 0.5, 1.0], [0.25, 0.25, 0.25, 1.0]);
					},
				}
			}
		}
	}
	r
}

fn selection_mesh(pos :Vector3<isize>) -> Vec<Vertex> {
	const DELTA :f32 = 0.05;
	const DELTAH :f32 = DELTA / 2.0;
	const COLOR :[f32; 4] = [0.0, 0.0, 0.3, 0.5];
	let mut vertices = Vec::new();

	push_block(&mut vertices,
		[pos.x as f32 - DELTAH, pos.y as f32 - DELTAH, pos.z as f32 - DELTAH],
		COLOR, COLOR, 1.0 + DELTA);
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
		let x = x as f32 / (BLOCKSIZE as f32);
		x.floor() as isize * BLOCKSIZE
	}
	v.map(r)
}

/// Block position to position inside chunk
fn btpic(v :Vector3<isize>) -> Vector3<isize> {
	v.map(|v| mod_euc(v as f32, BLOCKSIZE as f32) as isize)
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
		if let Some(b) = b {
			*b = input.state == glutin::ElementState::Pressed;
		}
	}
	fn tick(&mut self, time_delta :f32) {
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
		const DELTA :f32 = 20.0;
		delta_pos *= DELTA * time_delta;
		delta_pos = Rotation3::from_axis_angle(&Vector3::z_axis(), dtr(-self.yaw)) * delta_pos;
		self.pos += delta_pos;
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
