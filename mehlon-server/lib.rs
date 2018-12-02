extern crate noise;
extern crate nalgebra;
extern crate ncollide3d;
extern crate nphysics3d;
extern crate num_traits;
extern crate rand_pcg;
extern crate rand;
#[macro_use]
extern crate lazy_static;

pub mod map;

use map::{Map, ServerMap, MapBackend, MapChunkData, CHUNKSIZE, MapBlock};
//use glium::{glutin, Surface, VertexBuffer};
use nalgebra::{Vector3, Matrix4, Point3, Rotation3, Isometry3};
use num_traits::identities::Zero;
//use glium_glyph::GlyphBrush;
//use glium_glyph::glyph_brush::{
//	rusttype::Font, Section, Layout, HorizontalAlign,
//};
use std::collections::HashMap;
use std::time::{Instant, Duration};
use std::thread;
use std::sync::mpsc::{channel, Receiver, Sender};
use ncollide3d::shape::{Cuboid, Compound, ShapeHandle};
use ncollide3d::math::Isometry;
use nphysics3d::math::Inertia;
use nphysics3d::volumetric::Volumetric;
use nphysics3d::world::World;
use nphysics3d::object::{BodyHandle, BodyMut, ColliderHandle, Material};

pub enum ClientToServerMsg {
	SetBlock(Vector3<isize>, MapBlock),
	PlaceTree(Vector3<isize>),
	SetPos(Vector3<f32>),
}

pub enum ServerToClientMsg {
	ChunkUpdated(Vector3<isize>, MapChunkData),
}

type MeshResReceiver = Receiver<(Vector3<isize>, Option<Compound<f32>>)>;

fn gen_chunks_around<B :MapBackend>(map :&mut Map<B>, pos :Vector3<isize>, xyradius :isize, zradius :isize) {
	let chunk_pos = btchn(pos);
	let radius = Vector3::new(xyradius, xyradius, zradius) * CHUNKSIZE;
	let chunk_pos_min = btchn(chunk_pos - radius);
	let chunk_pos_max = btchn(chunk_pos + radius);
	map.gen_chunks_in_area(chunk_pos_min, chunk_pos_max);
}

pub struct Server {

	physics_world :World<f32>,
	player_handle :BodyHandle,

	meshres_r :MeshResReceiver,

	stc_s :Sender<ServerToClientMsg>,
	cts_r :Receiver<ClientToServerMsg>,

	vbuffs :HashMap<Vector3<isize>, (Option<ColliderHandle>)>,

	selected_pos :Option<(Vector3<isize>, Vector3<isize>)>,

	last_frame_time :Instant,
	last_fps :f32,

	map :ServerMap,
	camera :Camera,
}

pub struct ServerConnection {
	pub stc_r :Receiver<ServerToClientMsg>,
	pub cts_s :Sender<ClientToServerMsg>,
}

impl Server {
	pub fn new() -> (Self, ServerConnection) {
		let mut map = ServerMap::new(78);
		let camera = Camera::new();

		let (stc_s, stc_r) = channel();
		let (cts_s, cts_r) = channel();

		let (meshgen_s, meshgen_r) = channel();
		let (meshres_s, meshres_r) = channel();
		thread::spawn(move || {
			while let Ok((p, chunk)) = meshgen_r.recv() {
				let v = Instant::now();
				let mut shapes = Vec::new();
				let compound = if shapes.len() > 0 {
					Some(Compound::new(shapes))
				} else {
					None
				};
				let _ = meshres_s.send((p, compound));
				println!("generating mesh took {:?}", Instant::now() - v);
			}
		});

		let stc_sc = stc_s.clone();
		map.register_on_change(Box::new(move |chunk_pos, chunk| {
			meshgen_s.send((chunk_pos, *chunk)).unwrap();
			let _ = stc_sc.send(ServerToClientMsg::ChunkUpdated(chunk_pos, *chunk));
		}));

		// This ensures that the mesh generation thread puts higher priority onto positions
		// close to the player at the beginning.
		gen_chunks_around(&mut map, camera.pos.map(|v| v as isize), 1, 1);

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

		let srv = Server {
			physics_world,
			player_handle,

			meshres_r,

			stc_s,
			cts_r,

			vbuffs : HashMap::new(),

			selected_pos : None,

			last_frame_time : Instant::now(),
			last_fps : 0.0,
			map,
			camera,
		};
		let conn = ServerConnection {
			stc_r,
			cts_s,
		};
		(srv, conn)
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
	pub fn run_loop(&mut self) {
		loop {
			gen_chunks_around(&mut self.map,
				self.camera.pos.map(|v| v as isize), 4, 2);
			self.recv_vbuffs();
			let float_delta = self.update_fps();
			self.physics_world.set_timestep(float_delta);
			self.physics_world.step();
			let exit = false;
			while let Ok(msg) = self.cts_r.try_recv() {
				use ClientToServerMsg::*;
				match msg {
					SetBlock(p, b) => {
						if let Some(mut hdl) = self.map.get_blk_mut(p) {
							hdl.set(b);
						} else {
							// TODO log something about an attempted action in an unloaded chunk
						}
					},
					PlaceTree(p) => {
					},
					SetPos(p) => {
						self.camera.pos = p;
					},
				}
			}

			if exit {
				break;
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

	fn recv_vbuffs(&mut self) {
		while let Ok((p, c)) = self.meshres_r.try_recv() {
			let material = Material::new(0.0, 0.0);
			let collider = c.map(|c| {
				let hdl = ShapeHandle::new(c);
				self.physics_world.add_collider(0.01, hdl,
					BodyHandle::ground(), nalgebra::one(), material)
			});
			let old_opt = self.vbuffs.insert(p, (collider));
			if let Some((Some(coll))) = old_opt {
				self.physics_world.remove_colliders(&[coll]);
			}
		}
	}
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

	fast_mode :bool,
	noclip_mode :bool,

}

impl Camera {
	fn new() -> Self {
		Camera {
			aspect_ratio : 1024.0 / 768.0,
			pitch : 0.0,
			yaw : 0.0,
			pos : Vector3::new(60.0, 40.0, 20.0),

			fast_mode : false,
			noclip_mode : false,
		}
	}
	fn delta_pos(&mut self) -> Vector3<f32> {
		Vector3::new(0.0, 0.0, 0.0)
	}

	fn direction(&self) -> Point3<f32> {
		let pitch = dtr(-self.pitch);
		let yaw = dtr(-self.yaw);
		Point3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin())
	}
}
