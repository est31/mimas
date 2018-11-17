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
use glium::{glutin, Surface, VertexBuffer};
use glium::backend::Facade;
use nalgebra::{Vector3, Matrix4, Point3, Rotation3};
use num_traits::identities::Zero;
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	rusttype::Font, Section,
};
use line_drawing::{VoxelOrigin, WalkVoxels};
use std::collections::HashMap;
use std::time::Instant;

fn main() {
	let mut events_loop = glutin::EventsLoop::new();
	let mut game = Game::new(&events_loop);
	game.run_loop(&mut events_loop);
}

fn gen_vbuffs<F :Facade>(display :&F, map :&Map) ->
		HashMap<Vector3<isize>, VertexBuffer<Vertex>> {
	let v = Instant::now();
	let ret = map.chunks.iter()
		.map(|(p, c)| (p, mesh_for_chunk(*p, c)))
		.map(|(p, m)| (*p, VertexBuffer::new(display, &m).unwrap()))
		.collect::<HashMap<_, _>>();
	println!("generating the meshes took {:?}", Instant::now() - v);
	ret
}

fn vbuffs_update<F :Facade>(vbuffs :&mut HashMap<Vector3<isize>, VertexBuffer<Vertex>>,
		display :&F, map :&Map, pos :Vector3<isize>) {
	let v = Instant::now();
	let chunk_pos = btchn(pos);
	if let Some(chunk) = map.chunks.get(&chunk_pos) {
		let mesh = mesh_for_chunk(chunk_pos, chunk);
		let vb = VertexBuffer::new(display, &mesh).unwrap();
		vbuffs.insert(chunk_pos, vb);
	}
	println!("regen took {:?}", Instant::now() - v);
}


fn gen_chunks_around<F :Facade>(vbuffs :&mut HashMap<Vector3<isize>, VertexBuffer<Vertex>>,
		display :&F, map :&mut Map, pos :Vector3<isize>) {
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

	display :glium::Display,
	program :glium::Program,
	vbuffs :HashMap<Vector3<isize>, VertexBuffer<Vertex>>,

	selected_pos :Option<(Vector3<isize>, Vector3<isize>)>,

	last_pos :Option<winit::dpi::LogicalPosition>,

	last_frame_time :Instant,
	last_fps :f32,

	grab_cursor :bool,
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
		map.gen_chunks_start();
		let camera = Camera::new();

		let program = glium::Program::from_source(&display, VERTEX_SHADER_SRC,
			FRAGMENT_SHADER_SRC, None).unwrap();

		let vbuffs = gen_vbuffs(&display, &map);

		let grab_cursor = true;

		if grab_cursor {
			display.gl_window().hide_cursor(true);
			display.gl_window().grab_cursor(true).unwrap();
		}

		let swidth = 1024.0;
		let sheight = 768.0;

		Game {
			display,
			program,
			vbuffs,

			selected_pos : None,

			last_pos : None,
			last_frame_time : Instant::now(),
			last_fps : 0.0,
			grab_cursor,
			map,
			camera,

			swidth,
			sheight,
		}
	}
	/// Update the stored fps value and return the delta time
	fn update_fps(&mut self) -> f32 {
		let cur_time = Instant::now();
		let time_delta = cur_time - self.last_frame_time;
		self.last_frame_time = cur_time;
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
		let fps = self.last_fps * (1.0 - EPS) + fps_cur_term * EPS;
		self.last_fps = fps;
		float_delta
	}
	fn run_loop(&mut self, events_loop :&mut glutin::EventsLoop) {
		let fonts = vec![Font::from_bytes(KENPIXEL).unwrap()];
		let mut glyph_brush = GlyphBrush::new(&self.display, fonts);
		loop {
			gen_chunks_around(&mut self.vbuffs, &self.display, &mut self.map, self.camera.pos.map(|v| v as isize));
			self.render(&mut glyph_brush);
			let float_delta = self.update_fps();
			let close = self.handle_events(events_loop);
			self.camera.tick(float_delta);
			if close {
				break;
			}
			if self.grab_cursor {
				self.display.gl_window().set_cursor_position(winit::dpi::LogicalPosition {
					x : self.swidth / 2.0,
					y : self.sheight / 2.0,
				}).unwrap();
			}
		}
	}
	fn render<'a, 'b>(&mut self, glyph_brush :&mut GlyphBrush<'a, 'b>) {
		// building the uniforms
		let uniforms = uniform! {
			vmatrix : self.camera.get_matrix(),
			pmatrix : self.camera.get_perspective()
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

		for buff in self.vbuffs.values().chain(selbuff.iter()) {
			target.draw(buff,
				&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
				&self.program, &uniforms, &params).unwrap();
		}
		glyph_brush.draw_queued(&self.display, &mut target);

		target.finish().unwrap();
	}
	fn handle_events(&mut self, events_loop :&mut glutin::EventsLoop) -> bool {
		let mut close = false;
		events_loop.poll_events(|event| {
			match event {
				glutin::Event::WindowEvent { event, .. } => match event {

					glutin::WindowEvent::CloseRequested => close = true,

					glutin::WindowEvent::Resized(glium::glutin::dpi::LogicalSize {width, height}) => {
						self.swidth = width;
						self.sheight = height;
						self.camera.aspect_ratio = (width / height) as f32;
					},
					glutin::WindowEvent::KeyboardInput { input, .. } => {
						match input.virtual_keycode {
							Some(glutin::VirtualKeyCode::Escape) => {
								close = true;
							}
							_ => (),
						}
						self.camera.handle_kinput(input);

					},
					glutin::WindowEvent::CursorMoved { position, .. } => {
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
					},
					glutin::WindowEvent::MouseInput { state, button, .. } => {
						if state == glutin::ElementState::Pressed {
							if let Some((selected_pos, before_selected)) = self.selected_pos {
								let mut pos_to_update = None;
								if button == glutin::MouseButton::Left {
									let blk = self.map.get_blk_mut(selected_pos).unwrap();
									*blk = MapBlock::Air;
									pos_to_update = Some(selected_pos);
								} else if button == glutin::MouseButton::Right {
									let blk = self.map.get_blk_mut(before_selected).unwrap();
									*blk = MapBlock::Wood;
									pos_to_update = Some(before_selected);
								}
								if let Some(pos) = pos_to_update {
									vbuffs_update(&mut self.vbuffs, &self.display,
										&self.map, pos);
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
fn push_block<F :Fn([isize; 3]) -> bool>(r :&mut Vec<Vertex>, [x, y, z] :[f32; 3], color :[f32; 4], colorh :[f32; 4], siz :f32, blocked :F) {
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

fn mesh_for_chunk(offs :Vector3<isize>, chunk :&MapChunk) ->
		Vec<Vertex> {
	let mut r = Vec::new();
	for x in 0 .. BLOCKSIZE {
		for y in 0 .. BLOCKSIZE {
			for z in 0 .. BLOCKSIZE {
				let mut push_blk = |color, colorh| {
						let pos = [offs.x as f32 + x as f32, offs.y as f32 + y as f32, offs.z as f32 + z as f32];
						push_block(&mut r, pos, color, colorh, 1.0, |[xo, yo, zo]| {
							let pos = Vector3::new(x + xo, y + yo, z + zo);
							let outside = pos.map(|v| v < 0 || v >= BLOCKSIZE);
							if outside.x || outside.y || outside.z {
								return false;
							}
							match *chunk.get_blk(pos) {
								MapBlock::Air => false,
								_ => true,
							}
						});
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
		COLOR, COLOR, 1.0 + DELTA, |_| false);
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
