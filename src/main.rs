#![feature(euclidean_division)]

extern crate noise;
extern crate cgmath;
#[macro_use]
extern crate glium;
extern crate winit;

mod map;

use map::{Map, MapChunk, BLOCKSIZE, MapBlock};
use glium::{glutin, Surface};
use cgmath::{Vector3, Point3, InnerSpace, Matrix3, Matrix4, One, Zero};
use cgmath::{Deg, Rotation, EuclideanSpace, Matrix};

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
		in vec3 normal;

		out vec3 vnormal;

		uniform mat4 pmatrix;
		uniform mat4 vmatrix;
		void main() {
			vnormal = normal;
			gl_Position = pmatrix * vmatrix * vec4(position, 1.0);
		}
	"#;

	let fragment_shader_src = r#"
		#version 140
		in vec3 vnormal;

		out vec4 fcolor;

		void main() {
			fcolor = vec4(vnormal, 1.0);
		}
	"#;

	let program = glium::Program::from_source(&display, vertex_shader_src,
		fragment_shader_src, None).unwrap();

	let vbuffs = map.chunks.iter()
		.map(|(p, c)| mesh_for_chunk(*p, c))
		.map(|m| glium::VertexBuffer::new(&display, &m).unwrap())
		.collect::<Vec<_>>();

	let mut grab_cursor = true;

	if grab_cursor {
		display.gl_window().hide_cursor(true);
		display.gl_window().grab_cursor(true).unwrap();
	}

	let mut last_pos :Option<winit::dpi::LogicalPosition> = None;
	loop {
		// building the uniforms
		let uniforms = uniform! {
			vmatrix : camera.get_matrix(),
			pmatrix : camera.get_perspective()
		};


		let params = glium::draw_parameters::DrawParameters {
			depth : glium::Depth {
				test : glium::draw_parameters::DepthTest::IfLess,
				write : true,
				.. Default::default()
			},
			backface_culling : glium::draw_parameters::BackfaceCullingMode::CullingDisabled,
			//polygon_mode : glium::draw_parameters::PolygonMode::Line,
			.. Default::default()
		};

		// drawing a frame
		let mut target = display.draw();
		target.clear_color_and_depth((0.05, 0.01, 0.6, 0.0), 1.0);
		for buff in vbuffs.iter() {
			target.draw(buff,
				&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
				&program, &uniforms, &params).unwrap();
		}
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
	position: [f32; 3],
	normal: [f32; 3],
}

implement_vertex!(Vertex, position, normal);

fn mesh_for_chunk(offs :Vector3<usize>, chunk :&MapChunk) ->
		Vec<Vertex> {
	let mut r = Vec::new();
	for x in 0 .. BLOCKSIZE {
		for y in 0 .. BLOCKSIZE {
			for z in 0 .. BLOCKSIZE {
				if *chunk.get_blk(x, y, z) == MapBlock::Ground {

					let mut push_face = |(x, y, z), (xsd, ysd, yd, zd), normal| {
						r.push(Vertex { position: [x, y, z], normal });
						r.push(Vertex { position: [x + xsd, y + ysd, z], normal });
						r.push(Vertex { position: [x, y + yd, z + zd], normal });

						r.push(Vertex { position: [x + xsd, y + ysd, z], normal });
						r.push(Vertex { position: [x + xsd, y + yd + ysd, z + zd], normal });
						r.push(Vertex { position: [x, y + yd, z + zd], normal });
					};
					let (x, y, z) = ((offs.x + x) as f32, (offs.y + y) as f32, (offs.z + z) as f32);
					// X-Y face
					push_face((x, y, z), (1.0, 0.0, 1.0, 0.0), [0.0, 1.0, 0.0]);
					// X-Z face
					push_face((x, y, z), (1.0, 0.0, 0.0, 1.0), [0.0, 0.5, 0.0]);
					// Y-Z face
					push_face((x, y, z), (0.0, 1.0, 0.0, 1.0), [0.0, 0.5, 0.0]);
					// X-Y face (z+1)
					push_face((x, y, z + 1.0), (1.0, 0.0, 1.0, 0.0), [0.0, 1.0, 0.0]);
					// X-Z face (y+1)
					push_face((x, y + 1.0, z), (1.0, 0.0, 0.0, 1.0), [0.0, 0.5, 0.0]);
					// Y-Z face (x+1)
					push_face((x + 1.0, y, z), (0.0, 1.0, 0.0, 1.0), [0.0, 0.5, 0.0]);
				}
			}
		}
	}
	r
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
		self.yaw = (self.yaw + 180.0).mod_euc(360.0) - 180.0;

		println!("pos {} {} {} rot {} {}", self.pos.x, self.pos.y, self.pos.z, self.pitch, self.yaw);
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
}
