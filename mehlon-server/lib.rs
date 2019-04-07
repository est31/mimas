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
extern crate fasthash;
extern crate toml;
extern crate rustls;
extern crate argon2;

extern crate webpki;
extern crate quinn;
extern crate slog_term;
extern crate slog;
extern crate tokio;
extern crate tokio_current_thread;
extern crate futures;
extern crate rcgen;

extern crate rusqlite;
extern crate libsqlite3_sys;
extern crate byteorder;
extern crate flate2;
extern crate base64;

pub mod map;
pub mod mapgen;
pub mod map_storage;
pub mod generic_net;
pub mod quic_net;
pub mod config;
pub mod sqlite_generic;
pub mod local_auth;

use map::{Map, ServerMap, MapBackend, MapChunkData, CHUNKSIZE, MapBlock};
use nalgebra::{Vector3};
use std::time::{Instant, Duration};
use std::thread;
use std::cell::RefCell;
use std::collections::{HashSet, HashMap};
use std::rc::Rc;
use std::fmt::Display;
use generic_net::{NetworkServerSocket, NetworkServerConn, NetErr};
use config::Config;
use map_storage::{PlayerIdPair, PlayerPosition};
use local_auth::{SqliteLocalAuth, AuthBackend, PlayerPwHash, HashParams};

#[derive(Serialize, Deserialize)]
pub enum ClientToServerMsg {
	LogIn(String),
	SendHash(PlayerPwHash),

	SetBlock(Vector3<isize>, MapBlock),
	PlaceTree(Vector3<isize>),
	SetPos(Vector3<f32>),
	Chat(String),
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ServerToClientMsg {
	HashEnrollment,
	HashParams(HashParams),
	LogInFail(String),

	SetPos(Vector3<f32>),
	ChunkUpdated(Vector3<isize>, MapChunkData),
	Chat(String),
}

#[derive(Clone, PartialEq, Eq)]
enum AuthState {
	Unauthenticated,
	NewUser(String),
	WaitingForHash(String, PlayerIdPair, PlayerPwHash),
}

#[derive(Debug)]
pub struct StrErr(String);

impl<T :Display> From<T> for StrErr {
	fn from(v :T) -> Self {
		StrErr(format!("{}", v))
	}
}

fn chunk_positions_around(pos :Vector3<isize>, xyradius :isize, zradius :isize) -> (Vector3<isize>, Vector3<isize>) {
	let chunk_pos = btchn(pos);
	let radius = Vector3::new(xyradius, xyradius, zradius) * CHUNKSIZE;
	let chunk_pos_min = btchn(chunk_pos - radius);
	let chunk_pos_max = btchn(chunk_pos + radius);
	(chunk_pos_min, chunk_pos_max)
}

fn gen_chunks_around<B :MapBackend>(map :&mut Map<B>, pos :Vector3<isize>, xyradius :isize, zradius :isize) {
	let (chunk_pos_min, chunk_pos_max) = chunk_positions_around(pos, xyradius, zradius);
	map.gen_chunks_in_area(chunk_pos_min, chunk_pos_max);
}

struct Player<C: NetworkServerConn> {
	conn :C,
	ids :PlayerIdPair,
	nick :String,
	pos :Vector3<f32>,
	sent_chunks :HashSet<Vector3<isize>>,
	last_chunk_pos :Vector3<isize>,
}

impl<C: NetworkServerConn> Player<C> {
	pub fn from_conn_id_nick(conn :C, ids :PlayerIdPair, nick :String) -> Self {
		Player {
			conn,
			ids,
			nick,
			pos : Vector3::new(0.0, 0.0, 0.0),
			sent_chunks : HashSet::new(),
			last_chunk_pos : Vector3::new(0, 0, 0),
		}
	}
}

pub struct Server<S :NetworkServerSocket> {
	srv_socket :S,
	is_singleplayer :bool,
	config :Config,
	auth_back :Option<SqliteLocalAuth>,
	unauthenticated_players :Vec<(S::Conn, AuthState)>,
	players_waiting_for_kv :HashMap<PlayerIdPair, (S::Conn, String)>,
	players :Rc<RefCell<HashMap<PlayerIdPair, Player<S::Conn>>>>,

	last_frame_time :Instant,
	last_pos_storage_time :Instant,
	last_fps :f32,

	map :ServerMap,
}

impl<S :NetworkServerSocket> Server<S> {
	pub fn new(srv_socket :S, singleplayer :bool, mut config :Config) -> Self {
		let backends = map_storage::backends_from_config(&mut config, !singleplayer);
		let (storage_back, auth_back) = backends;
		let mut map = ServerMap::new(config.mapgen_seed, storage_back);

		let unauthenticated_players = Vec::<_>::new();
		let players = Rc::new(RefCell::new(HashMap::<_, Player<S::Conn>>::new()));
		let playersc = players.clone();
		map.register_on_change(Box::new(move |chunk_pos, chunk| {
			let mut players = playersc.borrow_mut();
			let msg = ServerToClientMsg::ChunkUpdated(chunk_pos, *chunk);
			let mut conns_to_close = Vec::new();
			for (id, player) in players.iter_mut() {
				player.sent_chunks.insert(chunk_pos);
				match player.conn.send(msg.clone()) {
					Ok(_) => (),
					Err(_) => conns_to_close.push(*id),
				}
			}
			close_connections(&conns_to_close, &mut *players);
		}));

		let srv = Server {
			srv_socket,
			is_singleplayer : singleplayer,
			config,
			auth_back,
			unauthenticated_players,
			players_waiting_for_kv : HashMap::new(),
			players,

			last_frame_time : Instant::now(),
			last_pos_storage_time : Instant::now(),
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
		const FPS_TGT :f32 = 60.0;
		let fps_cur_term = if float_delta > 0.0 {
			1.0 / float_delta
		} else {
			// At the beginning float_delta can be zero
			// and 1/0 would fuck up the last_fps value.
			// Also, a value of 0.0 would mean that FPS
			// limiting couldn't kick in. Thus, set
			// it to something highly above the target.
			FPS_TGT * 100.0
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
	fn handle_auth_msgs(&mut self) {
		// TODO do SRP based auth or spake2 or sth
		let mut players_to_add = Vec::new();
		let mut conns_to_remove = Vec::new();
		enum Verdict {
			AddAsPlayer(String, PlayerIdPair),
			LogInFail(String),
			Close,
		}
		for (idx, (conn, state)) in
				self.unauthenticated_players.iter_mut().enumerate() {
			loop {
				macro_rules! verdict {
					($e:expr) => {
						conns_to_remove.push((idx, $e));
						break;
					};
				}
				let msg = conn.try_recv();
				match msg {
					Ok(Some(ClientToServerMsg::LogIn(nick))) => {
						// Check that the nick uses valid characters
						let nick_has_valid_chars = nick
							.bytes()
							.all(|b|
								(b'0' ..= b'9').contains(&b) ||
								(b'a' ..= b'z').contains(&b) ||
								(b'A' ..= b'Z').contains(&b) ||
								(b == b'-' || b == b'_'));

						if !nick_has_valid_chars {
							verdict!(Verdict::LogInFail("Invalid characters in nick".to_string()));
						}

						let la = self.auth_back.as_mut().unwrap();
						let id_opt = la.get_player_id(&nick, 1).unwrap();
						if let Some(id) = id_opt {
							if let Some(pwh) = la.get_player_pwh(id).unwrap() {
								let params = pwh.params().clone();
								conn.send(ServerToClientMsg::HashParams(params));
								*state = AuthState::WaitingForHash(nick, id, pwh);
							} else {
								verdict!(Verdict::LogInFail("No password hash stored on server".to_string()));
							}
						} else {
							// New user
							*state = AuthState::NewUser(nick);
							conn.send(ServerToClientMsg::HashEnrollment);
						}
					},
					Ok(Some(ClientToServerMsg::SendHash(pwh))) => {
						match state {
							AuthState::NewUser(nick) => {
								let la = self.auth_back.as_mut().unwrap();
								let id = la.add_player(&nick, pwh, 1).unwrap();
								// Check whether the same nick is already present on the server
								let verdict = if !self.players.borrow().get(&id).is_some() {
									Verdict::AddAsPlayer(nick.to_string(), id)
								} else {
									Verdict::LogInFail("Player already logged in".to_string())
								};
								verdict!(verdict);
							},
							AuthState::WaitingForHash(nick, id, pwh_wanted) => {
								let verdict = if pwh_wanted == &pwh {
									// Check whether the same nick is already present on the server
									if !self.players.borrow().get(&id).is_some() {
										Verdict::AddAsPlayer(nick.to_string(), *id)
									} else {
										Verdict::LogInFail("Player already logged in".to_string())
									}
								} else {
									Verdict::LogInFail("Wrong password".to_string())
								};
								verdict!(verdict);
							},
							_ => {
								verdict!(Verdict::LogInFail("Wrong auth state".to_string()));
							},
						}
					},
					Ok(Some(_msg)) => {
						// Ignore all other msgs
						// TODO Maybe in the future, the client can be made
						// to not send messages to the server
						// while auth is still ongoing
					},
					Ok(None) => break,
					Err(NetErr::ConnectionClosed) => {
						println!("Client connection closed.");
						verdict!(Verdict::Close);
					},
					Err(_) => {
						println!("Client connection error.");
						verdict!(Verdict::Close);
					},
				}
			}
		}
		for (skew, (idx, verd)) in conns_to_remove.into_iter().enumerate() {
			println!("closing connection");
			let (conn, _state) = self.unauthenticated_players.remove(idx - skew);
			match verd {
				Verdict::AddAsPlayer(nick, id) => {
					players_to_add.push((conn, id, nick));
				},
				Verdict::LogInFail(reason) => {
					let _ = conn.send(ServerToClientMsg::LogInFail(reason));
				},
				Verdict::Close => (),
			}
		}
		for (conn, id, nick) in players_to_add.into_iter() {
			self.add_player_waiting(conn, id, nick);
		}
	}
	fn handle_players_waiting_for_kv(&mut self) {
		let mut players_to_add = Vec::new();
		// TODO with NLL, these {} become unneccessary
		// See: https://github.com/rust-lang/rust/issues/57804
		{
			let pwfk = &mut self.players_waiting_for_kv;
			self.map.run_for_kv_results(&mut |id, _payload, key, value| {
				if key != "position" {
					return;
				}
				if let Some((conn, nick)) = pwfk.remove(&id) {
					let pos = if let Some(buf) = value {
						PlayerPosition::deserialize(&buf)
							.ok()
							.unwrap_or_else(PlayerPosition::default)
					} else {
						// No value could be found
						PlayerPosition::default()
					};
					players_to_add.push((conn, id, nick, pos));
				}
			});
		}
		for (conn, id, nick, pos) in players_to_add {
			self.add_player(conn, id, nick, pos);
		}
	}
	fn get_msgs(&mut self) -> Vec<(PlayerIdPair, ClientToServerMsg)> {
		let mut msgs = Vec::new();
		let mut players = self.players.borrow_mut();
		let mut conns_to_close = Vec::new();
		for (id, player) in players.iter_mut() {
			loop {
				let msg = player.conn.try_recv();
				match msg {
					Ok(Some(ClientToServerMsg::SetPos(p))) => {
						player.pos = p;
					},
					Ok(Some(msg)) => {
						msgs.push((*id, msg));
					},
					Ok(None) => break,
					Err(NetErr::ConnectionClosed) => {
						println!("Client connection closed.");
						conns_to_close.push(*id);
						break;
					},
					Err(_) => {
						println!("Client connection error.");
						conns_to_close.push(*id);
						break;
					},
				}
			}
		}
		close_connections(&conns_to_close, &mut *players);
		msgs
	}
	fn send_chunks_to_player(&mut self, player :&mut Player<S::Conn>) -> Result<(), NetErr> {
		let isize_pos = player.pos.map(|v| v as isize);
		let (pmin, pmax) = chunk_positions_around(isize_pos, 6, 3);
		let pmin = pmin / CHUNKSIZE;
		let pmax = pmax / CHUNKSIZE;
		for x in pmin.x .. pmax.x {
			for y in pmin.y .. pmax.y {
				for z in pmin.z .. pmax.z {
					let p = Vector3::new(x, y, z) * CHUNKSIZE;
					if let Some(c) = self.map.get_chunk(p) {
						if !player.sent_chunks.contains(&p) {
							let msg = ServerToClientMsg::ChunkUpdated(p, *c);
							player.conn.send(msg)?;
							player.sent_chunks.insert(p);
						}
					}
				}
			}
		}
		Ok(())
	}
	fn store_player_positions(&mut self) -> Result<(), StrErr> {
		// This limiting makes sure we don't store the player positions
		// too often as that would mean too much wear on the hdd
		// and cause massive mapgen thread lag.
		const INTERVAL_MILLIS :u128 = 1_500;
		let now = Instant::now();
		if (now - self.last_pos_storage_time).as_millis() < INTERVAL_MILLIS {
			return Ok(());
		}
		self.last_pos_storage_time = now;
		let players = self.players.clone();
		for (_, player) in players.borrow().iter() {
			let serialized_str = toml::to_string(&PlayerPosition::from_pos(player.pos))?;
			self.map.set_player_kv(player.ids, "position", serialized_str.into());
		}
		Ok(())
	}
	fn send_chunks_to_players(&mut self) {
		let players = self.players.clone();
		let mut players_to_remove = Vec::new();
		for (id, player) in players.borrow_mut().iter_mut() {
			let isize_pos = player.pos.map(|v| v as isize);
			let player_pos_chn = btchn(isize_pos);
			if player.last_chunk_pos == player_pos_chn {
				continue;
			}
			player.last_chunk_pos = player_pos_chn;
			if self.send_chunks_to_player(player).is_err() {
				players_to_remove.push(*id);
			}
		}
		close_connections(&players_to_remove, &mut *players.borrow_mut());
	}
	fn add_player_waiting(&mut self, conn :S::Conn, id :PlayerIdPair, nick :String) {
		const PAYLOAD :u32 = 0;
		self.map.get_player_kv(id, "position", PAYLOAD);
		self.players_waiting_for_kv.insert(id, (conn, nick));
	}
	fn add_player(&mut self, conn :S::Conn, id :PlayerIdPair, nick :String, pos :PlayerPosition) {
		let player_count = {
			let msg = ServerToClientMsg::SetPos(pos.pos());
			// TODO get rid of unwrap
			conn.send(msg).unwrap();
			let mut players = self.players.borrow_mut();
			players.insert(id, Player::from_conn_id_nick(conn, id, nick.clone()));
			players.len()
		};
		// In singleplayer, don't spam messages about players joining
		if !self.is_singleplayer {
			let msg = format!("New player {} joined. Number of players: {}",
				nick, player_count);
			self.handle_chat_msg(msg);
		}
	}
	fn handle_command(&mut self, issuer_id :PlayerIdPair, msg :String) {
		println!("Command: {}", msg);
		let mut it = msg[1..].split(" ");
		let command = it.next().unwrap();
		let _params = it.collect::<Vec<&str>>();
		match command {
			"spawn" => {
				let players = self.players.clone();
				let msg = ServerToClientMsg::SetPos(PlayerPosition::default().pos());
				let remove_player = {
					let player = &players.borrow_mut()[&issuer_id];
					player.conn.send(msg.clone()).is_err()
				};
				if remove_player {
					close_connections(&[issuer_id], &mut *players.borrow_mut());
				}
			},
			_ => {
				// TODO only send this to the player invoking the command,
				// TODO not all players
				self.handle_chat_msg(format!("Unknown command {}", command));
			},
		}
	}
	fn handle_chat_msg(&mut self, msg :String) {
		println!("Chat: {}", msg);
		let players = self.players.clone();
		let mut players_to_remove = Vec::new();
		for (id, player) in players.borrow_mut().iter_mut() {
			let msg = ServerToClientMsg::Chat(msg.clone());
			if player.conn.send(msg).is_err() {
				players_to_remove.push(*id);
			}
		}
		close_connections(&players_to_remove, &mut *players.borrow_mut());
	}
	pub fn run_loop(&mut self) {
		loop {
			let positions = self.players.borrow().iter()
				.map(|(_, player)| {
					(btchn(player.pos.map(|v| v as isize)), player.last_chunk_pos)
				})
				.filter(|(cp, lcp)| cp != lcp)
				.map(|(cp, _lcp)| cp)
				.collect::<Vec<_>>();
			for pos in positions {
				gen_chunks_around(&mut self.map,
					pos.map(|v| v as isize),
					self.config.mapgen_radius_xy,
					self.config.mapgen_radius_z);
			}
			self.send_chunks_to_players();
			self.map.tick();
			let _float_delta = self.update_fps();
			let exit = false;
			while let Some(conn) = self.srv_socket.try_open_conn() {
				if self.is_singleplayer {
					let id = PlayerIdPair::singleplayer();
					self.add_player_waiting(conn, id, "singleplayer".to_owned());
				} else {
					self.unauthenticated_players.push((conn, AuthState::Unauthenticated));
				}
			}
			self.handle_auth_msgs();
			self.handle_players_waiting_for_kv();
			self.store_player_positions().unwrap();

			let msgs = self.get_msgs();

			for (id, msg) in msgs {
				use ClientToServerMsg::*;
				match msg {
					LogIn(_) => {
						// Invalid at this state. Ignore.
						// TODO maybe issue a warning in the log? idk
					},
					SendHash(_h) => {
						// Invalid at this state. Ignore.
						// TODO maybe issue a warning in the log? idk
					},
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
					Chat(m) => {
						if m.starts_with('/') {
							self.handle_command(id, m);
						} else {
							let m = {
								let nick = &self.players.borrow()[&id].nick;
								format!("<{}> {}", nick, m)
							};
							self.handle_chat_msg(m);
						}
					},
				}
			}

			if exit {
				break;
			}
		}
	}
}

fn close_connections(conns_to_close :&[PlayerIdPair], connections :&mut HashMap<PlayerIdPair, impl Sized>) {
	for id in conns_to_close.iter() {
		println!("closing connection");
		connections.remove(&id);
	}
}

fn durtofl(d :Duration) -> f32 {
	d.as_millis() as f32 / 1_000.0
}

// TODO: once euclidean division stabilizes,
// use it: https://github.com/rust-lang/rust/issues/49048
fn mod_euc(a :f32, b :f32) -> f32 {
	((a % b) + b) % b
}

/// Block position to chunk position
pub fn btchn(v :Vector3<isize>) -> Vector3<isize> {
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
