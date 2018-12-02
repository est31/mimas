extern crate noise;
extern crate nalgebra;
extern crate rand_pcg;
extern crate rand;
#[macro_use]
extern crate lazy_static;

pub mod map;
pub mod generic_net;

use map::{Map, ServerMap, MapBackend, MapChunkData, CHUNKSIZE, MapBlock};
use nalgebra::{Vector3};
use std::time::{Instant, Duration};
use generic_net::{MpscServerSocket, NetworkServerSocket, ServerConnection};

pub enum ClientToServerMsg {
	SetBlock(Vector3<isize>, MapBlock),
	PlaceTree(Vector3<isize>),
	SetPos(Vector3<f32>),
}

pub enum ServerToClientMsg {
	ChunkUpdated(Vector3<isize>, MapChunkData),
}

fn gen_chunks_around<B :MapBackend>(map :&mut Map<B>, pos :Vector3<isize>, xyradius :isize, zradius :isize) {
	let chunk_pos = btchn(pos);
	let radius = Vector3::new(xyradius, xyradius, zradius) * CHUNKSIZE;
	let chunk_pos_min = btchn(chunk_pos - radius);
	let chunk_pos_max = btchn(chunk_pos + radius);
	map.gen_chunks_in_area(chunk_pos_min, chunk_pos_max);
}

pub struct Server {
	srv_socket :MpscServerSocket,

	last_frame_time :Instant,
	last_fps :f32,

	map :ServerMap,
	player_pos :Vector3<f32>,
}

impl Server {
	pub fn new() -> (Self, ServerConnection) {
		let (srv_socket, conn) = MpscServerSocket::new();
		let mut map = ServerMap::new(78);
		let player_pos = Vector3::new(60.0, 40.0, 20.0);

		let stc_sc = srv_socket.stc_s.clone();
		map.register_on_change(Box::new(move |chunk_pos, chunk| {
			let _ = stc_sc.send(ServerToClientMsg::ChunkUpdated(chunk_pos, *chunk));
		}));

		// This ensures that the mesh generation thread puts higher priority onto positions
		// close to the player at the beginning.
		gen_chunks_around(&mut map, player_pos.map(|v| v as isize), 1, 1);

		let srv = Server {
			srv_socket,

			last_frame_time : Instant::now(),
			last_fps : 0.0,
			map,
			player_pos,
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
				self.player_pos.map(|v| v as isize), 4, 2);
			let _float_delta = self.update_fps();
			let exit = false;
			while let Some(msg) = self.srv_socket.try_recv() {
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
						map::spawn_tree(&mut self.map, p);
					},
					SetPos(p) => {
						self.player_pos = p;
					},
				}
			}

			if exit {
				break;
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

// TODO: once euclidean division stabilizes,
// use it: https://github.com/rust-lang/rust/issues/49048
fn mod_euc(a :f32, b :f32) -> f32 {
	((a % b) + b) % b
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
