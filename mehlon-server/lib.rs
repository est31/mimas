#![forbid(unsafe_code)]

extern crate noise;
extern crate nalgebra;
extern crate rand_pcg;
extern crate rand;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_big_array;
extern crate bincode;

pub mod map;
pub mod generic_net;

use map::{Map, ServerMap, MapBackend, MapChunkData, CHUNKSIZE, MapBlock};
use nalgebra::{Vector3};
use std::time::{Instant, Duration};
use std::cell::RefCell;
use std::rc::Rc;
use generic_net::{TcpServerConn, TcpServerSocket, NetworkServerConn, NetErr};

#[derive(Serialize, Deserialize)]
pub enum ClientToServerMsg {
	SetBlock(Vector3<isize>, MapBlock),
	PlaceTree(Vector3<isize>),
	SetPos(Vector3<f32>),
}

#[derive(Serialize, Deserialize, Clone)]
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

struct Player {
	conn :TcpServerConn,
	pos :Vector3<f32>,
}

impl Player {
	pub fn from_conn(conn :TcpServerConn) -> Self {
		Player {
			conn,
			pos : Vector3::new(0.0, 0.0, 0.0),
		}
	}
}

pub struct Server {
	srv_socket :TcpServerSocket,
	players :Rc<RefCell<Vec<Player>>>,

	last_frame_time :Instant,
	last_fps :f32,

	map :ServerMap,
}

impl Server {
	pub fn new(srv_socket :TcpServerSocket) -> Self {
		let mut map = ServerMap::new(78);

		let players = Rc::new(RefCell::new(Vec::<Player>::new()));
		let playersc = players.clone();
		map.register_on_change(Box::new(move |chunk_pos, chunk| {
			let mut players = playersc.borrow_mut();
			let msg = ServerToClientMsg::ChunkUpdated(chunk_pos, *chunk);
			let mut conns_to_close = Vec::new();
			for (idx, player) in players.iter().enumerate() {
				match player.conn.send(msg.clone()) {
					Ok(_) => (),
					Err(_) => conns_to_close.push(idx),
				}
			}
			close_connections(&conns_to_close, &mut *players);
		}));

		let srv = Server {
			srv_socket,
			players,

			last_frame_time : Instant::now(),
			last_fps : 0.0,
			map,
		};
		srv
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
	fn get_msgs(&mut self) -> Vec<ClientToServerMsg> {
		let mut msgs = Vec::new();
		let mut players = self.players.borrow_mut();
		let mut conns_to_close = Vec::new();
		for (idx, player) in players.iter_mut().enumerate() {
			loop {
				let msg = player.conn.try_recv();
				match msg {
					Ok(Some(ClientToServerMsg::SetPos(p))) => {
						player.pos = p;
					},
					Ok(Some(msg)) => {
						msgs.push(msg);
					},
					Ok(None) => break,
					Err(NetErr::ConnectionClosed) => {
						println!("Client connection closed.");
						conns_to_close.push(idx);
						break;
					},
					Err(_) => {
						println!("Client connection error.");
						conns_to_close.push(idx);
						break;
					},
				}
			}
		}
		close_connections(&conns_to_close, &mut *players);
		msgs
	}
	pub fn run_loop(&mut self) {
		loop {
			let positions = self.players.borrow().iter()
				.map(|player| player.pos).collect::<Vec<_>>();
			for pos in positions {
				gen_chunks_around(&mut self.map,
					pos.map(|v| v as isize), 4, 2);
			}
			let _float_delta = self.update_fps();
			let exit = false;
			while let Some(conn) = self.srv_socket.try_open_conn() {
				self.players.borrow_mut().push(Player::from_conn(conn));
			}
			let msgs = self.get_msgs();

			for msg in msgs {
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
					SetPos(_p) => unreachable!(),
				}
			}

			if exit {
				break;
			}
		}
	}
}

fn close_connections(conns_to_close :&[usize], connections :&mut Vec<impl Sized>) {
	for (skew, idx) in conns_to_close.iter().enumerate() {
		println!("closing connection");
		connections.remove(idx - skew);
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
