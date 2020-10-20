use mimas_common::generic_net::{NetworkServerSocket, NetworkServerConn, NetErr};
use mimas_common::config::Config;
use mimas_common::crafting::get_matching_recipe;
use mimas_common::map::{self, Map, ServerMap, MapBackend,
	CHUNKSIZE, MetadataEntry};
use mimas_common::map_storage::{PlayerIdPair, PlayerPosition};
use mimas_common::inventory::{self, SelectableInventory, Stack, InventoryPos,
	InventoryLocation, InvRef};
use mimas_common::local_auth::{SqliteLocalAuth, AuthBackend};
use mimas_common::game_params::ServerGameParamsHdl;
use mimas_common::protocol::{ClientToServerMsg, ServerToClientMsg};
use mimas_common::player::PlayerMode;
use mimas_common::btchn;
use anyhow::Result;
use nalgebra::Vector3;
use std::time::{Instant, Duration};
use std::thread;
use std::cell::RefCell;
use std::collections::{HashSet, HashMap, hash_map};
use std::rc::Rc;
use srp::server::{SrpServer, UserRecord};
use srp::client::SrpClient;
use srp::groups::G_4096;
use sha2::Sha256;
use rand::RngCore;

use crate::game_params::load_server_game_params;
use crate::map_storage;

enum AuthState {
	Unauthenticated,
	NewUser(String),
	WaitingForM1(String, PlayerIdPair, SrpServer<Sha256>),
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

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
struct PlayerSlowStates {
	// Nick of the player.
	// This is NOT the authoritative location,
	// only a cached value meant to help people
	// who haven't gotten a copy of the auth db.
	nick :String,
	modes :HashSet<PlayerMode>,
}

impl PlayerSlowStates {
	fn new(nick :&str) -> Self {
		Self {
			nick : nick.to_owned(),
			modes : HashSet::new(),
		}
	}
}

struct Player<C: NetworkServerConn> {
	conn :C,
	ids :PlayerIdPair,
	nick :String,
	pos :PlayerPosition,

	inventory :SelectableInventory,
	inventory_last_ser :SelectableInventory,
	craft_inventory :SelectableInventory,
	craft_inventory_last_ser :SelectableInventory,
	slow_states :PlayerSlowStates,
	slow_states_last_ser :PlayerSlowStates,

	sent_chunks :HashSet<Vector3<isize>>,
	last_chunk_pos :Vector3<isize>,
}

impl<C: NetworkServerConn> Player<C> {
	pub fn from_waiting(waiting :KvWaitingPlayer<C>) -> Self {
		let mut slow_states = waiting.slow_states.clone().unwrap();
		// The nick field in slow_states is only a map db cache of
		// the real nick stored in the auth db, helpful e.g. when
		// you only get the map db and want to figure out player nicks.
		// Here we update the nick if we notice it has changed,
		// but make sure that it's done in a way that the update
		// is written to the DB as well (by not updating
		// the _last_ser field).
		if slow_states.nick != waiting.nick {
			slow_states.nick = waiting.nick.clone();
		}
		Player {
			conn : waiting.conn,
			ids : waiting.ids,
			nick : waiting.nick,
			pos : PlayerPosition::default(),
			inventory : waiting.inv.clone().unwrap(),
			inventory_last_ser : waiting.inv.unwrap(),
			craft_inventory : waiting.craft_inv.clone().unwrap(),
			craft_inventory_last_ser : waiting.craft_inv.clone().unwrap(),
			slow_states,
			slow_states_last_ser : waiting.slow_states.clone().unwrap(),
			sent_chunks : HashSet::new(),
			last_chunk_pos : Vector3::new(0, 0, 0),
		}
	}
	fn pos(&self) -> Vector3<f32> {
		self.pos.pos()
	}
}

struct KvWaitingPlayer<C: NetworkServerConn> {
	conn :C,
	ids :PlayerIdPair,
	nick :String,
	pos :Option<PlayerPosition>,
	inv :Option<SelectableInventory>,
	craft_inv :Option<SelectableInventory>,
	slow_states :Option<PlayerSlowStates>,
}

impl<C: NetworkServerConn> KvWaitingPlayer<C> {
	fn new(conn :C, ids :PlayerIdPair, nick :String) -> Self {
		Self {
			conn,
			ids,
			nick,
			pos : None,
			inv : None,
			craft_inv : None,
			slow_states : None,
		}
	}
	fn ready(&self) -> bool {
		self.pos.is_some() && self.inv.is_some()
			&& self.craft_inv.is_some()
			&& self.slow_states.is_some()
	}
}

pub struct Server<S :NetworkServerSocket> {
	srv_socket :S,
	params :ServerGameParamsHdl,
	is_singleplayer :bool,
	config :Config,
	auth_back :Option<SqliteLocalAuth>,
	unauthenticated_players :Vec<(S::Conn, AuthState)>,
	players_waiting_for_kv :HashMap<PlayerIdPair, KvWaitingPlayer<S::Conn>>,
	players :Rc<RefCell<HashMap<PlayerIdPair, Player<S::Conn>>>>,

	last_frame_time :Instant,
	last_pos_storage_time :Instant,
	last_fps :f32,

	map :ServerMap,
}

impl<S :NetworkServerSocket> Server<S> {
	pub fn new(srv_socket :S,
			singleplayer :bool, mut config :Config) -> Self {
		let backends = map_storage::backends_from_config(&mut config, !singleplayer);
		let (mut storage_back, auth_back) = backends;
		let nm = map_storage::load_name_id_map(&mut storage_back).unwrap();
		let params = load_server_game_params(nm);
		map_storage::save_name_id_map(&mut storage_back, &params.p.name_id_map).unwrap();
		let mut map = ServerMap::new(config.mapgen_seed,
			params.clone(), storage_back);

		let unauthenticated_players = Vec::<_>::new();
		let players = Rc::new(RefCell::new(HashMap::<_, Player<S::Conn>>::new()));
		let playersc = players.clone();
		map.register_on_change(Box::new(move |chunk_pos, chunk| {
			let mut players = playersc.borrow_mut();
			let msg = ServerToClientMsg::ChunkUpdated(chunk_pos, chunk.clone());
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
			params,
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
		let float_delta = (cur_time - self.last_frame_time).as_secs_f32();
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
					Ok(Some(ClientToServerMsg::LogIn(nick, a_pub))) => {
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
								let verifier = {
									// Note that by computing the verifier on the server side
									// we don't do SRP as intended. SRP wants you to compute
									// the verifier from the password key on the client side, only
									// giving the server the verifier, which is also only
									// useful for that very purpose. Our design gives the
									// server direct access to the (stretched) password hash.
									//
									// This brings the disadvantage that if e.g. the server
									// database gets compromised, clients could use those keys
									// to log into the server. However, there are two disadvantages
									// to the recommended SRP approach:
									// * First, for the G_4096 group used right now,
									//   the keys are quite long, about 690 characters in base64.
									//   Compare that to the 58 characters in base64 for the "bare"
									//   argon2 hash.
									// * Second, the keys are bound to the very group used.
									//   If one day we decided to migrate off SRP e.g. to spake2
									//   or use a different SRP group (e.g. a more secure one),
									//   we'd have to do complex protocol redesigns.
									// Furthermore, as the salt during enrolling is chosen at
									// random by the client, knowledge of the salted password hash
									// does not give access to anything but that very same server.
									//
									// Also note thet the algorithm SPAKE2 has the same disadvantage
									// as our chosen approach.
									// Here, too, a server compromise would allow the bad guys to
									// authenticate as that user, but like with our approach
									// that's only fixed to the specific seed stored on the server.
									// This disadvantage in fact has motivated SPAKE2+, which is
									// probably our long term replacement for SRP. But our
									// current situation is easiest to migrate away from.
									//
									// Currently, usage of SPAKE2+ is blocked due to no
									// implementation being available. An issue requesting support
									// for it has been filed upstream:
									// https://github.com/RustCrypto/PAKEs/issues/30


									// TODO hopefully upstream gives us a more convenient function
									// than having to go through the client.
									// https://github.com/RustCrypto/PAKEs/issues/17
									let srp_client = SrpClient::<Sha256>::new(&[], &*G_4096);
									srp_client.get_password_verifier(pwh.hash())
								};
								let user_record = UserRecord {
									username : &[],
									salt : &[],
									verifier : &verifier,
								};

								let mut b = [0; 64];
								let mut rng = rand::rngs::OsRng;
								rng.fill_bytes(&mut b);

								let srp_server = SrpServer::new(&user_record, &a_pub, &b, &*G_4096).unwrap();
								let b_pub = srp_server.get_b_pub();
								conn.send(ServerToClientMsg::HashParamsBpub(params, b_pub));
								*state = AuthState::WaitingForM1(nick, id, srp_server);
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
							_ => {
								verdict!(Verdict::LogInFail("Wrong auth state".to_string()));
							},
						}
					},
					Ok(Some(ClientToServerMsg::SendM1(m1))) => {
						match state {
							AuthState::WaitingForM1(nick, id, srp_server) => {
								let verdict = if srp_server.verify(&m1).is_ok() {
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
		let pwfk = &mut self.players_waiting_for_kv;
		let nm = &self.params.p.name_id_map;
		self.map.run_for_kv_results(&mut |id, _payload, key, value| {
			if let hash_map::Entry::Occupied(mut kvw) = pwfk.entry(id) {
				let mut check = false;
				if key == "position" {
					let KvWaitingPlayer { pos, .. } = kvw.get_mut();
					*pos = Some(if let Some(buf) = value {
						PlayerPosition::deserialize(&buf)
							.ok()
							.unwrap_or_else(PlayerPosition::default)
					} else {
						// No value could be found
						PlayerPosition::default()
					});
					check = true;
				} else if key == "inventory" {
					let KvWaitingPlayer { inv, .. } = kvw.get_mut();
					*inv = Some(if let Some(buf) = value {
						SelectableInventory::deserialize(&buf, nm)
							.ok()
							.unwrap_or_else(SelectableInventory::new)
					} else {
						// No value could be found
						SelectableInventory::new()
					});
					check = true;
				} else if key == "craft_inventory" {
					let KvWaitingPlayer { craft_inv, .. } = kvw.get_mut();
					*craft_inv = Some(if let Some(buf) = value {
						SelectableInventory::deserialize(&buf, nm)
							.ok()
							.unwrap_or_else(SelectableInventory::crafting_inv)
					} else {
						// No value could be found
						SelectableInventory::crafting_inv()
					});
					check = true;
				} else if key == "slow_states" {
					let KvWaitingPlayer { slow_states, nick,.. } = kvw.get_mut();
					*slow_states = Some(if let Some(buf) = value {
						toml::from_slice(&buf)
							.ok()
							.unwrap_or_else(|| PlayerSlowStates::new(nick))
					} else {
						// No value could be found
						PlayerSlowStates::new(nick)
					});
					check = true;
				}
				if check && kvw.get().ready() {
					players_to_add.push(kvw.remove());
				}
			}
		});
		for kvw in players_to_add {
			self.add_player(kvw);
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
		let isize_pos = player.pos().map(|v| v as isize);
		let (pmin, pmax) = chunk_positions_around(isize_pos,
			self.config.sent_chunks_radius_xy, self.config.sent_chunks_radius_z);
		let pmin = pmin / CHUNKSIZE;
		let pmax = pmax / CHUNKSIZE;
		for x in pmin.x .. pmax.x {
			for y in pmin.y .. pmax.y {
				for z in pmin.z .. pmax.z {
					let p = Vector3::new(x, y, z) * CHUNKSIZE;
					if let Some(c) = self.map.get_chunk(p) {
						if !player.sent_chunks.contains(&p) {
							let msg = ServerToClientMsg::ChunkUpdated(p, c.clone());
							player.conn.send(msg)?;
							player.sent_chunks.insert(p);
						}
					}
				}
			}
		}
		Ok(())
	}
	fn store_player_kvs(&mut self) -> Result<()> {
		self.store_player_positions()?;
		self.store_player_inventories()?;
		Ok(())
	}
	fn store_player_inventories(&mut self) -> Result<()> {
		// Store the player inventories at each update instead of at certain
		// intervals because in general, intervals don't change around.
		let players = self.players.clone();
		for (_, player) in players.borrow_mut().iter_mut() {
			if player.inventory_last_ser != player.inventory {
				let serialized_inv = player.inventory.serialize();
				self.map.set_player_kv(player.ids, "inventory", serialized_inv);
				player.inventory_last_ser = player.inventory.clone();
			}
			if player.craft_inventory_last_ser != player.craft_inventory {
				let serialized_inv = player.craft_inventory.serialize();
				self.map.set_player_kv(player.ids, "craft_inventory", serialized_inv);
				player.inventory_last_ser = player.craft_inventory.clone();
			}
			if player.slow_states_last_ser != player.slow_states {
				let serialized_states = toml::to_string(&player.slow_states).unwrap().into_bytes();
				self.map.set_player_kv(player.ids, "slow_states", serialized_states);
				player.slow_states_last_ser = player.slow_states.clone();
			}
		}
		Ok(())
	}
	fn store_player_positions(&mut self) -> Result<()> {
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
			let serialized_str = toml::to_string(&player.pos)?;
			self.map.set_player_kv(player.ids, "position", serialized_str.into());
		}
		Ok(())
	}
	fn send_chunks_to_players(&mut self) {
		let players = self.players.clone();
		let mut players_to_remove = Vec::new();
		for (id, player) in players.borrow_mut().iter_mut() {
			let isize_pos = player.pos().map(|v| v as isize);
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
	fn send_positions_to_players(&mut self) {
		let players = self.players.clone();
		let mut players_to_remove = Vec::new();
		let player_positions = players.borrow().iter()
			.map(|(id, player)| (*id, player.pos))
			.collect::<Vec<_>>();
		for (id, player) in players.borrow_mut().iter_mut() {
			let msg = ServerToClientMsg::PlayerPositions(*id, player_positions.clone());
			if player.conn.send(msg).is_err() {
				players_to_remove.push(*id);
			}
		}
		close_connections(&players_to_remove, &mut *players.borrow_mut());
	}
	fn add_player_waiting(&mut self, conn :S::Conn, id :PlayerIdPair, nick :String) {
		const PAYLOAD :u32 = 0;
		self.map.get_player_kv(id, "position", PAYLOAD);
		self.map.get_player_kv(id, "inventory", PAYLOAD);
		self.map.get_player_kv(id, "craft_inventory", PAYLOAD);
		self.map.get_player_kv(id, "slow_states", PAYLOAD);
		self.players_waiting_for_kv.insert(id, KvWaitingPlayer::new(conn, id, nick));
	}
	fn add_player(&mut self, pl :KvWaitingPlayer<S::Conn>) {
		let nick = pl.nick.clone();
		let player_count = {
			let msg = ServerToClientMsg::GameParams(self.params.p.clone());
			// TODO get rid of unwrap
			pl.conn.send(msg).unwrap();

			let msg = ServerToClientMsg::SetPos(pl.pos.unwrap());
			// TODO get rid of unwrap
			pl.conn.send(msg).unwrap();

			let msg = ServerToClientMsg::SetInventory(pl.inv.clone().unwrap());
			// TODO get rid of unwrap
			pl.conn.send(msg).unwrap();

			let msg = ServerToClientMsg::SetCraftInventory(pl.craft_inv.clone().unwrap());
			// TODO get rid of unwrap
			pl.conn.send(msg).unwrap();

			let modes = pl.slow_states.as_ref().unwrap().modes.clone();
			let msg = ServerToClientMsg::SetModes(modes);
			// TODO get rid of unwrap
			pl.conn.send(msg).unwrap();

			let mut players = self.players.borrow_mut();
			let id = pl.ids;
			let player = Player::from_waiting(pl);
			players.insert(id, player);
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
		let params = it.collect::<Vec<&str>>();
		match command {
			"info" => {
				self.chat_msg_for(issuer_id, format!(
					"{} {}",
					env!("CARGO_PKG_NAME"),
					env!("CARGO_PKG_VERSION")));
			},
			"spawn" => {
				let players = self.players.clone();
				let msg = ServerToClientMsg::SetPos(PlayerPosition::default());
				let remove_player = {
					let player = &players.borrow_mut()[&issuer_id];
					player.conn.send(msg.clone()).is_err()
				};
				if remove_player {
					close_connections(&[issuer_id], &mut *players.borrow_mut());
				}
			},
			"gime" => {
				let content = params.get(0);
				let content = if let Some(content) = content {
					if let Some(mb) = self.params.p.search_block_name(*content) {
						mb
					} else {
						self.chat_msg_for(issuer_id, format!("Invalid item {}", content));
						return;
					}
				} else {
					self.chat_msg_for(issuer_id, "No content to give specified");
					return;
				};
				// TODO print an error if parsing fails. Or maybe not? IDK.
				let count = params.get(1)
					.and_then(|v| v.parse().ok())
					.unwrap_or(1);
				let content_disp = self.params.p.block_display_name(content);
				self.chat_msg_for(issuer_id, format!("Giving {} of {}", count, content_disp));
				let mut players = self.players.borrow_mut();
				let remove_player = {
					let player = players.get_mut(&issuer_id).unwrap();
					player.inventory.put(Stack::with(content, count));
					let msg = ServerToClientMsg::SetInventory(player.inventory.clone());
					player.conn.send(msg).is_err()
				};
				if remove_player {
					close_connections(&[issuer_id], &mut *players);
				}
			},
			"clear" => {
				let to_clear = params.get(0);
				enum Cmd {
					Selection,
					Inventory,
				}
				let cmd = match to_clear {
					Some(&"selection") | Some(&"sel") => {
						self.chat_msg_for(issuer_id, "Clearing selection");
						Cmd::Selection
					},
					Some(&"inventory") | Some(&"inv") => {
						self.chat_msg_for(issuer_id, "Clearing inventory");
						Cmd::Inventory
					},
					_ => {
						self.chat_msg_for(issuer_id, "Invalid clearing command.");
						return;
					},
				};

				let mut players = self.players.borrow_mut();
				let remove_player = {
					let player = players.get_mut(&issuer_id).unwrap();
					match cmd {
						Cmd::Selection => {
							let sel = player.inventory.selection();
							let sel_stack = sel.and_then(|s|player.inventory.stacks_mut().get_mut(s));
							if let Some(sel_stack) = sel_stack {
								*sel_stack = Stack::Empty;
							} else {
								return;
							}
						},
						Cmd::Inventory => {
							player.inventory.stacks_mut().iter_mut()
								.for_each(|i| *i = Stack::Empty);
						},
					}
					let msg = ServerToClientMsg::SetInventory(player.inventory.clone());
					player.conn.send(msg).is_err()
				};
				if remove_player {
					close_connections(&[issuer_id], &mut *players);
				}
			},
			_ => {
				self.chat_msg_for(issuer_id, format!("Unknown command {}", command));
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
	fn chat_msg_for(&mut self, for_id :PlayerIdPair, msg :impl Into<String>) {
		let players = self.players.clone();
		let mut players_to_remove = Vec::new();
		let msg = msg.into();
		for (id, player) in players.borrow_mut().iter_mut() {
			if *id != for_id {
				continue;
			}
			let msg = ServerToClientMsg::Chat(msg.clone());
			if player.conn.send(msg).is_err() {
				players_to_remove.push(*id);
			}
		}
		close_connections(&players_to_remove, &mut *players.borrow_mut());
	}
	pub fn handle_dig(&mut self, id :PlayerIdPair, p :Vector3<isize>) {
		let mut remove = true;
		if let Some(chest_meta) = self.map.get_blk_meta(p) {
			if let Some(MetadataEntry::Inventory(inv)) = chest_meta {
				if !inv.is_empty() {
					remove = false;
				}
			}
		} else {
			// TODO log something about an attempted action in an unloaded chunk
			remove = false;
		}
		let mut drops = None;
		if remove {
			{
				// We can unwrap here as above we set remove to false if
				// the result is None
				let mut hdl = self.map.get_blk_mut(p).unwrap();
				drops = Some(self.params.p.get_block_params(hdl.get()).unwrap().drops);
				let air_bl = self.params.p.block_roles.air;
				hdl.set(air_bl);
			}
			let mut hdl = self.map.get_blk_meta_mut(p).unwrap();
			hdl.clear();
		} else {
			// Send the unchanged block to the client
			if let Some(mut hdl) = self.map.get_blk_mut(p) {
				hdl.fake_change();
			}
		}
		let remove_player = {
			let mut players = self.players.borrow_mut();
			let player = &mut players.get_mut(&id).unwrap();
			if remove {
				// If we remove the block, put the dropped item
				// into the inventory. Send the new inventory to
				// the client in any case to override any
				// possibly mistaken local prediction.
				player.inventory.put(drops.unwrap());
			}
			let msg = ServerToClientMsg::SetInventory(player.inventory.clone());
			player.conn.send(msg).is_err()
		};
		if remove_player {
			close_connections(&[id], &mut *self.players.borrow_mut());
		}
	}
	pub fn handle_inv_move_or_swap(&mut self, id :PlayerIdPair, from_pos :InventoryPos,
			to_pos :InventoryPos, only_move_one :bool) {
		// Create a temporary RefCell so that we can have code that
		// seems to access self.map twice.
		// Note though that we forbid moving between two different
		// chests in the map by a manual check, so there won't be
		// an instance where the RefCell is borrowed twice at the same time.
		let map_cell = RefCell::new(&mut self.map);

		// Helper macro to save us some repetitive code
		macro_rules! do_for_inv_ref {
			($location:expr, $name:ident, $thing:expr) => {
				if let InventoryLocation::WorldMeta(p) = $location {
					// Move between two locations inside the chest
					if let Some(mut hdl) = map_cell.borrow_mut().get_blk_meta_mut(p) {
						if let Some(MetadataEntry::Inventory(inv)) = hdl.get().clone() {
							let mut $name = inv.clone();
							let invs = $thing;
							hdl.set(MetadataEntry::Inventory(invs.0));
							invs.1
						} else {
							// TODO log something about no metadata present
							return;
						}
					} else {
						// TODO log something about an attempted action in an unloaded chunk
						return;
					}
				} else if InventoryLocation::PlayerInv == $location {
					// Move inside the player's inventory

					// Store the inventory inside a local variable so that
					// the borrow to all players gets invalidated
					let mut inv = {
						let mut players = self.players.borrow_mut();
						let player = &mut players.get_mut(&id).unwrap();
						player.inventory.clone()
					};
					let mut $name = &mut inv;

					let invs = $thing;

					{
						let mut players = self.players.borrow_mut();
						let player = &mut players.get_mut(&id).unwrap();
						player.inventory = invs.0.clone();
					};
					// TODO maybe send changed inventory to player?

					invs.1
				} else { // InventoryLocation::CraftInv
					// Move inside the player's craft inventory

					// Store the inventory inside a local variable so that
					// the borrow to all players gets invalidated
					let mut inv = {
						let mut players = self.players.borrow_mut();
						let player = &mut players.get_mut(&id).unwrap();
						player.craft_inventory.clone()
					};
					let mut $name = &mut inv;

					let invs = $thing;

					{
						let mut players = self.players.borrow_mut();
						let player = &mut players.get_mut(&id).unwrap();
						player.craft_inventory = invs.0.clone();
					};
					// TODO maybe send changed inventory to player?

					invs.1
				}
			};
		}

		if from_pos.location == to_pos.location {
			// Move inside the same inventory
			let (from, to) = ((0, from_pos.stack_pos), (0, to_pos.stack_pos));
			do_for_inv_ref!(from_pos.location, inv, {
				{
					let mut invs = [(&mut inv) as &mut SelectableInventory];
					inventory::merge_or_move(&mut invs, from, to, only_move_one);
				}
				(inv, ())
			});
		} else {
			if from_pos.location.is_world_meta() && to_pos.location.is_world_meta() {
				// TODO log something about attempted move between two chests.
				// For now, this is unsupported (and would panic anyways due to the RefCell).
				return;
			}
			// Move between different inventories
			let (from, to) = ((0, from_pos.stack_pos), (1, to_pos.stack_pos));
			do_for_inv_ref!(from_pos.location, inv_from, {
				let inv_from = do_for_inv_ref!(to_pos.location, inv_to, {
					{
						let mut invs = [(&mut inv_from) as &mut dyn InvRef, (&mut inv_to) as &mut dyn InvRef];
						inventory::merge_or_move(&mut invs, from, to, only_move_one);
					}
					(inv_to, inv_from,)
				});
				(inv_from, (),)
			});
		}
	}

	pub fn handle_craft(&mut self, id :PlayerIdPair) {
		let mut players = self.players.borrow_mut();
		let player = &mut players.get_mut(&id).unwrap();

		let recipe = get_matching_recipe(&player.craft_inventory, &self.params.p);
		let output = recipe.map(|r| r.output);

		if let Some(output) = output {
			player.inventory.put(output);
			// Reduce inputs of input inv.
			for st in player.craft_inventory.stacks_mut().iter_mut() {
				st.take_n(1);
			}
		}
	}
	pub fn run_loop(&mut self) {
		loop {
			self.tick();
		}
	}
	fn tick(&mut self) {
		let positions = self.players.borrow().iter()
			.map(|(_, player)| {
				(btchn(player.pos.pos().map(|v| v as isize)), player.last_chunk_pos)
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
		self.send_positions_to_players();
		self.map.tick();
		let _float_delta = self.update_fps();
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
		self.store_player_kvs().unwrap();

		let msgs = self.get_msgs();

		for (id, msg) in msgs {
			use mimas_common::ClientToServerMsg::*;
			match msg {
				LogIn(..) |
				SendHash(_) |
				SendM1(..) => {
					// Invalid at this state. Ignore.
					// TODO maybe issue a warning in the log? idk
				},
				GetHashedBlobs(blob_list) => {
					let hashed_blobs = blob_list.iter()
						.filter_map(|h| self.params.textures.get(h)
							.map(|b| (h.clone(), b.clone())))
						.collect::<Vec<_>>();
					let msg = ServerToClientMsg::HashedBlobs(hashed_blobs);
					let remove_player = {
						let player = &self.players.borrow_mut()[&id];
						player.conn.send(msg.clone()).is_err()
					};
					if remove_player {
						close_connections(&[id], &mut *self.players.borrow_mut());
					}
				},
				PlaceBlock(p, sel_idx, b) => {
					// Block to make the borrow_mut work
					{
						let players = &mut self.players.borrow_mut();
						let player = players.get_mut(&id).unwrap();
						let sel = player.inventory.get_sel_idx_and_content();
						if Some((sel_idx, b)) != sel {
							// TODO log something about selected inventory mismatch
							// between client and server

							// TODO Maybe send msg to the client
							// that the block placing failed??
							continue;
						}
						player.inventory.take_selected();
						// Don't send anything to the client, its
						// prediction was alright.
					};
					let mut block_set = false;
					if let Some(mut hdl) = self.map.get_blk_mut(p) {
						// TODO check if air
						hdl.set(b);
						block_set = true;
					} else {
						// TODO log something about an attempted action in an unloaded chunk
					}
					let has_inv = self.params.p.get_block_params(b).unwrap().inventory;
					if let (Some(stack_num), true) = (has_inv, block_set) {
						if let Some(mut hdl) = self.map.get_blk_meta_mut(p) {
							let inv = SelectableInventory::empty_with_size(stack_num as usize);
							hdl.set(MetadataEntry::Inventory(inv));
						} else {
							// TODO log error about an attempted action in an unloaded chunk
							// It needs to be an ERROR because we should already have
							// set the block above successfully so if we can't set it now
							// again, there is some problem...
						}
					}
				},
				PlaceTree(p, sel_idx, b) => {
					// Block to make the borrow_mut work
					{
						let players = &mut self.players.borrow_mut();
						let player = players.get_mut(&id).unwrap();
						let sel = player.inventory.get_sel_idx_and_content();
						if Some((sel_idx, b)) != sel {
							// TODO log something about selected inventory mismatch
							// between client and server

							// TODO Maybe send msg to the client
							// that the block placing failed??
							continue;
						}
						player.inventory.take_selected();
						// Don't send anything to the client, its
						// prediction was alright.
					};
					let on_place_plants_tree = self.params.p.get_block_params(b).unwrap().on_place_plants_tree;
					if !on_place_plants_tree {
						// TODO log something about on_place_plants_tree not being set
						continue;
					}
					map::spawn_tree(&mut self.map, p, &self.params);
				},
				Dig(p) => {
					self.handle_dig(id, p);
				},
				SetPos(_p) => unreachable!(),
				SetMode(mode, enabled) => {
					let mut players = self.players.borrow_mut();
					let player = &mut players.get_mut(&id).unwrap();
					if enabled {
						player.slow_states.modes.insert(mode);
					} else {
						player.slow_states.modes.remove(&mode);
					}
				},

				InventorySwap(from_pos, to_pos, only_move_one) => {
					self.handle_inv_move_or_swap(id, from_pos, to_pos, only_move_one);
				},
				Craft => {
					self.handle_craft(id);
				},
				InventorySelect(selection) => {
					let mut players = self.players.borrow_mut();
					let player = &mut players.get_mut(&id).unwrap();
					if !player.inventory.set_selection(selection) {
						// TODO log something about an error or something
					}
				},
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
	}
}

fn close_connections(conns_to_close :&[PlayerIdPair], connections :&mut HashMap<PlayerIdPair, impl Sized>) {
	for id in conns_to_close.iter() {
		println!("closing connection");
		connections.remove(&id);
	}
}
