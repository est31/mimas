extern crate noise;
extern crate cgmath;
#[macro_use]
extern crate glium;
extern crate winit;
extern crate glium_glyph;
extern crate line_drawing;

mod map;

use map::{Map, MapChunk, BLOCKSIZE, MapBlock};
use glium::{glutin, Surface};
use cgmath::{Vector3, Point3, InnerSpace, Matrix3, Matrix4, One, Zero};
use cgmath::{Deg, Rotation, EuclideanSpace, Matrix};
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	rusttype::{self, Font}, Section,
};
use line_drawing::{VoxelOrigin, WalkVoxels};

fn main() {
	let mut events_loop = glutin::EventsLoop::new();
	let window = glutin::WindowBuilder::new()
		.with_title("Mehlon");
	let context = glutin::ContextBuilder::new().with_depth_buffer(24);
	let display = glium::Display::new(window, context, &events_loop).unwrap();

	let mut map = Map::new(77);
	map.gen_chunks();

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

	let vbuffs = map.chunks.iter()
		.map(|(p, c)| mesh_for_chunk(*p, c))
		.map(|m| glium::VertexBuffer::new(&display, &m).unwrap())
		.collect::<Vec<_>>();
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
	loop {
		// building the uniforms
		let uniforms = uniform! {
			vmatrix : camera.get_matrix(),
			pmatrix : camera.get_perspective()
		};

		let selected_pos = camera.get_selected_pos(&map);
		let mut sel_text = "sel = None".to_string();
		selbuff.clear();
		if let Some(selected_pos) = selected_pos {
			let blk = map.get_blk(selected_pos.x, selected_pos.y, selected_pos.z).unwrap();
			sel_text = format!("sel = ({}, {}, {}), {:?}", selected_pos.x, selected_pos.y, selected_pos.z, blk);

			// TODO: only update if the position actually changed from the prior one
			// this spares us needless chatter with the GPU
			let vertices = selection_mesh(selected_pos);
			let vbuff = glium::VertexBuffer::new(&display, &vertices).unwrap();
			selbuff = vec![vbuff];
		}

		let screen_dims = display.get_framebuffer_dimensions();
		// TODO turn off anti-aliasing of the font
		// https://gitlab.redox-os.org/redox-os/rusttype/issues/61
		glyph_brush.queue(Section {
			text : &format!("pos = ({}, {}, {}) pi = {}, yw = {}, {}", camera.pos.x, camera.pos.y,
				camera.pos.z, camera.pitch, camera.yaw, sel_text),
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
			backface_culling : glium::draw_parameters::BackfaceCullingMode::CullingDisabled,
			blend :glium::Blend::alpha_blending(),
			//polygon_mode : glium::draw_parameters::PolygonMode::Line,
			.. Default::default()
		};

		// drawing a frame
		let mut target = display.draw();
		target.clear_color_and_depth((0.05, 0.01, 0.6, 0.0), 1.0);

		for buff in vbuffs.iter().chain(selbuff.iter()) {
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

					_ => (),
				},
				_ => (),
			}
		});
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
	let mut push_face = |(x, y, z), (xsd, ysd, yd, zd), color| {
		r.push(Vertex { position: [x, y, z], color });
		r.push(Vertex { position: [x + xsd, y + ysd, z], color });
		r.push(Vertex { position: [x, y + yd, z + zd], color });

		r.push(Vertex { position: [x + xsd, y + ysd, z], color });
		r.push(Vertex { position: [x + xsd, y + yd + ysd, z + zd], color });
		r.push(Vertex { position: [x, y + yd, z + zd], color });
	};
	// X-Y face
	push_face((x, y, z), (siz, 0.0, siz, 0.0), color);
	// X-Z face
	push_face((x, y, z), (siz, 0.0, 0.0, siz), colorh);
	// Y-Z face
	push_face((x, y, z), (0.0, siz, 0.0, siz), colorh);
	// X-Y face (z+1)
	push_face((x, y, z + siz), (siz, 0.0, siz, 0.0), color);
	// X-Z face (y+1)
	push_face((x, y + siz, z), (siz, 0.0, 0.0, siz), colorh);
	// Y-Z face (x+1)
	push_face((x + siz, y, z), (0.0, siz, 0.0, siz), colorh);
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
				match *chunk.get_blk(x, y, z) {
					MapBlock::Air => (),
					MapBlock::Ground => {
						push_blk([0.0, 1.0, 0.0, 1.0], [0.0, 0.5, 0.0, 1.0]);
					},
					MapBlock::Water => {
						push_blk([0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 5.0, 1.0]);
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

struct Camera {
	aspect_ratio :f32,
	pitch :f32,
	yaw :f32,
	pos :Vector3<f32>,
}

impl Camera {
	fn new() -> Self {
		Camera {
			aspect_ratio : 1024.0 / 768.0,
			pitch : 0.0,
			yaw : 0.0,
			pos : Vector3::new(60.0, 40.0, 20.0),
		}
	}
	fn handle_kinput(&mut self, input :glium::glutin::KeyboardInput) {
		let key = match input.virtual_keycode {
			Some(key) => key,
			None => return,
		};
		let delta = 2.0;
		let mut delta_pos = Vector3::zero();
		match key {
			glutin::VirtualKeyCode::W => delta_pos += Vector3::unit_x(),
			glutin::VirtualKeyCode::A => delta_pos += Vector3::unit_y(),
			glutin::VirtualKeyCode::S => delta_pos -= Vector3::unit_x(),
			glutin::VirtualKeyCode::D => delta_pos -= Vector3::unit_y(),
			glutin::VirtualKeyCode::Space => delta_pos += Vector3::unit_z(),
			glutin::VirtualKeyCode::LShift => delta_pos -= Vector3::unit_z(),
		_ => (),
		}
		delta_pos *= delta;
		delta_pos = Matrix3::from_angle_z(Deg(-self.yaw)) * delta_pos;
		self.pos += delta_pos;
	}
	fn handle_mouse_move(&mut self, delta :winit::dpi::LogicalPosition) {
		let factor = 0.7;
		self.pitch = clamp(factor * delta.y as f32 + self.pitch, -89.999, 89.999);
		self.yaw += factor * delta.x as f32;
		self.yaw = mod_euc(self.yaw + 180.0, 360.0) - 180.0;
	}

	fn direction(&self) -> Vector3<f32> {
		let pitch = -self.pitch / 180.0 * std::f32::consts::PI;
		let yaw =- self.yaw / 180.0 * std::f32::consts::PI;
		Vector3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin())
	}

	fn get_matrix(&self) -> [[f32; 4]; 4] {
		let m = Matrix4::look_at_dir(Point3::origin() + self.pos, self.direction(), Vector3::unit_z());
		m.into()
	}

	pub fn get_perspective(&self) -> [[f32; 4]; 4] {
		let fov = Deg(90.0);
		let zfar = 1024.0;
		let znear = 0.1;
		cgmath::perspective(fov, self.aspect_ratio, znear, zfar).into()
	}

	pub fn get_selected_pos(&self, map :&Map) -> Option<Vector3<isize>> {
		const SELECTION_RANGE :f32 = 10.0;
		let pointing_at_distance = self.pos + self.direction() * SELECTION_RANGE;

		for (x, y, z) in WalkVoxels::<f32, isize>::new(self.pos.into(),
				pointing_at_distance.into(), &VoxelOrigin::Center) {
			let v = Vector3::new(x as isize, y as isize, z as isize);
			if let Some(blk) = map.get_blk(v.x, v.y, v.z) {
				if blk.is_pointable() {
					return Some(v);
				}
			}
		}
		None
	}
}
