use std::thread;
use std::sync::mpsc::{channel, Receiver, Sender};
use nalgebra::Vector3;
use noise::{Perlin, NoiseFn, Seedable};
use std::collections::{HashMap, hash_map::Entry};
use std::mem::replace;
use std::hash::Hasher;
use {btchn, btpic};
use rand_pcg::Pcg32;
use rand::Rng;
use twox_hash::XxHash64;
use map_storage::PlayerIdPair;
use game_params::{ServerGameParamsHdl, BlockRoles};

use super::map::{Map, MapChunkData, MapBlock, MapBackend, CHUNKSIZE};
use map_storage::DynStorageBackend;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum GenerationPhase {
	/// Basic noise, elevation etc
	PhaseOne,
	/// Higher level features done
	PhaseTwo,
	/// The block and all of its neighbours are at least in phase two.
	Done,
}

#[derive(Clone)]
pub struct MapChunk {
	pub data :MapChunkData,
	generation_phase :GenerationPhase,
	tree_spawn_points :Vec<(Vector3<isize>, bool)>,
}

pub struct MapgenMap {
	seed :u64,
	params :ServerGameParamsHdl,
	chunks :HashMap<Vector3<isize>, MapChunk>,
	storage :DynStorageBackend,
}

impl MapChunk {
	pub fn get_blk_mut(&mut self, pos :Vector3<isize>) -> &mut MapBlock {
		let (x, y, z) = (pos.x, pos.y, pos.z);
		&mut self.data.0[(x * CHUNKSIZE * CHUNKSIZE + y * CHUNKSIZE + z) as usize]
	}
	pub fn get_blk(&self, pos :Vector3<isize>) -> &MapBlock {
		let (x, y, z) = (pos.x, pos.y, pos.z);
		&self.data.0[(x * CHUNKSIZE * CHUNKSIZE + y * CHUNKSIZE + z) as usize]
	}
}

fn pos_hash(pos :Vector3<isize>) -> u64 {
	let mut mh :XxHash64 = XxHash64::with_seed(0);
	mh.write_i64(pos.x as i64);
	mh.write_i64(pos.y as i64);
	mh.write_i64(pos.z as i64);
	mh.finish()
}

fn gen_chunk_phase_one(seed :u64, pos :Vector3<isize>,
		params :&ServerGameParamsHdl) -> MapChunk {
	let role = &params.p.block_roles;
	macro_rules! s {
		($e:expr) => {
			s!($e, u32)
		};
		($e:expr, $t:ident) => {{
			let mut seeder = Pcg32::new(seed, u64::from_be_bytes(*$e));
			let seed :$t= seeder.gen::<$t>();
			seed
		}};
	}
	// Basic chunk noise
	let f = 0.02356;
	let noise = NoiseMag::new(s!(b"chn-base"), f, 8.3);
	// Macro noise
	let mf = 0.0018671;
	let mnoise = NoiseMag::new(s!(b"chn-mcro"), mf, 23.27713);
	// Super macro noise
	let smf = 0.00043571;
	let smnoise = NoiseMag::new(s!(b"chn-smcr"), smf, 137.479131);
	// Amplifier noise
	let smf = 0.0023473;
	let ampnoise = Noise::new(s!(b"chn-ampl"), smf);
	// Amplifier noise 2
	let smf = 0.0023473;
	let ampnoise2 = Noise::new(s!(b"chn-ampt"), smf);
	// Tree noise
	let tf = 0.0088971;
	let tnoise = Noise::new(s!(b"trenoise"), tf);
	// Macro tree noise
	let mtf = 0.00093952;
	let mtnoise = Noise::new(s!(b"mtrnoise"), mtf);
	// Biome noise
	let bf = 0.0023881;
	let binoise = NoiseMag::new(s!(b"biom-bas"), bf, 0.4);
	// Macro biome noise
	let mbf = 0.00113881;
	let mbinoise = NoiseMag::new(s!(b"biom-mac"), mbf, 0.6);
	// Tree pcg
	let mut tpcg = Pcg32::new(s!(b"pcg-tree", u64), pos_hash(pos));

	let mut ore_rngs = params.mapgen_params.ores.iter()
		.map(|ore| {
			let noise = Noise::new(s!(&ore.noise_seed), ore.freq);
			let pcg = Pcg32::new(s!(&ore.pcg_seed, u64), pos_hash(pos));
			(ore, noise, pcg)
		})
		.collect::<Vec<_>>();

	let mut plant_rngs = params.mapgen_params.plants.iter()
		.map(|plant| {
			let pcg = Pcg32::new(s!(&plant.pcg_seed, u64), pos_hash(pos));
			(plant, pcg)
		})
		.collect::<Vec<_>>();

	// Cave noise
	let ca_f = 0.052951;
	let ca_noise = Noise::new(s!(b"nois-cav"), ca_f);
	// Macro cave noise
	let mca_f = 0.0094951;
	let mca_noise = Noise::new(s!(b"mnoi-cav"), mca_f);

	let mut res = MapChunk {
		data : MapChunkData::filled_with(role.air),
		generation_phase : GenerationPhase::PhaseOne,
		tree_spawn_points : Vec::new(),
	};
	for x in 0 .. CHUNKSIZE {
		for y in 0 .. CHUNKSIZE {
			let p = [(pos.x + x) as f64, (pos.y + y) as f64];
			let sm_elev = smnoise.get(p);
			let amp = 1.0 + ampnoise.get(p) * 0.9;
			let amp2 = 0.6 + ampnoise2.get(p) * 0.5;
			let base_noise = amp * noise.get(p) + amp2 * mnoise.get(p);
			let elev = base_noise + sm_elev;
			let elev_blocks = elev as isize;
			if let Some(elev_blocks) = elev_blocks.checked_sub(pos.z) {
				let els = elev_blocks - 4;
				let els = els.min(CHUNKSIZE).max(0);
				let elg = elev_blocks.min(CHUNKSIZE).max(0);
				for z in 0 .. els {
					*res.get_blk_mut(Vector3::new(x, y, z)) = role.stone;
					let p3 = [(pos.x + x) as f64, (pos.y + y) as f64, (pos.z + z) as f64];
					let z_abs = pos.z + z;

					for (ore, noise, pcg) in ore_rngs.iter_mut() {
						let limit = if z_abs < ore.limit_boundary {
							ore.limit_b
						} else {
							ore.limit_a
						};
						if noise.get_3d(p3) > limit {
							if pcg.gen::<f64>() > ore.pcg_chance {
								*res.get_blk_mut(Vector3::new(x, y, z)) = ore.block;
							}
						}
					}

					// Generate caves,
					// but make sure that there is a distance
					// between the cave and where the water starts.
					// We need to compare with elev_blocks instead of el
					// so that there are no artifacts introduced by the
					// maxing with CHUNKSIZE above.
					let mcave_thresh = if z_abs > -400 {
						2.0
					} else {
						1.0 + ((z_abs + 600) as f64 / 300.0).max(-0.502).min(0.0)
					};
					let cave_block = mca_noise.get_3d(p3) > mcave_thresh || ca_noise.get_3d(p3) > 0.45;
					if z + 10 < elev_blocks && cave_block {
						*res.get_blk_mut(Vector3::new(x, y, z)) = role.air;
					}
				}
				if pos.z < 0 {
					for z in  els .. CHUNKSIZE {
						*res.get_blk_mut(Vector3::new(x, y, z)) = role.water;
					}
				} else {
					let (ground_bl, ground_top) = if binoise.get(p) + mbinoise.get(p) < 0.3 {
						(role.ground, role.ground_top)
					} else {
						(role.sand, role.sand)
					};
					for z in els .. elg {
						*res.get_blk_mut(Vector3::new(x, y, z)) = ground_bl;
					}
					if let Some(z) = (els .. elg).rev().next() {
						if elev_blocks <= CHUNKSIZE {
							*res.get_blk_mut(Vector3::new(x, y, z)) = ground_top;
						}
					}
					if pos.z == 0 && elg <= 0 {
						*res.get_blk_mut(Vector3::new(x, y, 0)) = role.water;
					}
					if elg > 0 && elg < CHUNKSIZE {
						let in_desert = ground_bl == role.sand;
						// Tree spawning
						let tree_density = if in_desert {
							0.1
						} else {
							0.4
						};
						let macro_density = mtnoise.get(p);
						let macro_density = if macro_density < 0.0 {
							0.0
						} else {
							macro_density
						};
						let local_density = tnoise.get(p) + macro_density;

						let mut spawning_tree = false;
						if local_density > 1.0 - tree_density {
							// Generate a forest here
							let limit = if in_desert {
								0.99
							} else {
								0.91
							};
							if tpcg.gen::<f64>() > limit {
								spawning_tree = true;
								res.tree_spawn_points.push((pos + Vector3::new(x, y, elg), in_desert));
							}
						}

						if !spawning_tree {
							for (plant, pcg) in plant_rngs.iter_mut() {
								if pcg.gen::<f64>() > plant.pcg_limit {
									*res.get_blk_mut(Vector3::new(x, y, elg)) = plant.block;
								}
							}
						}
					}
				}
			}
		}
	}
	res
}

struct Noise {
	freq :f64,
	perlin :Perlin,
}

impl Noise {
	fn new(seed :u32, freq :f64) -> Self {
		Noise {
			freq,
			perlin : Perlin::new().set_seed(seed),
		}
	}
	fn get(&self, pos :[f64; 2]) -> f64 {
		let fp = [pos[0] * self.freq, pos[1] * self.freq];
		self.perlin.get(fp)
	}
	fn get_3d(&self, pos :[f64; 3]) -> f64 {
		let fp = [pos[0] * self.freq, pos[1] * self.freq, pos[2] * self.freq];
		self.perlin.get(fp)
	}
}

struct NoiseMag {
	freq :f64,
	mag :f64,
	perlin :Perlin,
}

impl NoiseMag {
	fn new(seed :u32, freq :f64, mag :f64) -> Self {
		NoiseMag {
			freq,
			mag,
			perlin : Perlin::new().set_seed(seed),
		}
	}
	fn get(&self, pos :[f64; 2]) -> f64 {
		let fp = [pos[0] * self.freq, pos[1] * self.freq];
		self.perlin.get(fp) * self.mag
	}
	/*fn get_3d(&self, pos :[f64; 3]) -> f64 {
		let fp = [pos[0] * self.freq, pos[1] * self.freq, pos[2] * self.freq];
		self.perlin.get(fp) * self.mag
	}*/
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Schematic {
	pub(super) items :Vec<(Vector3<isize>, MapBlock)>,
	pub(super) aabb_min :Vector3<isize>,
	pub(super) aabb_max :Vector3<isize>,
}

impl Schematic {
	pub fn from_items(items :Vec<(Vector3<isize>, MapBlock)>) -> Self {
		let (aabb_min, aabb_max) = aabb_min_max(&items);
		Schematic {
			items,
			aabb_min,
			aabb_max,
		}
	}
}

fn aabb_min_max(items :&[(Vector3<isize>, MapBlock)]) -> (Vector3<isize>, Vector3<isize>) {
	let min_x = items.iter().map(|(pos, _)| pos.x).min().unwrap();
	let min_y = items.iter().map(|(pos, _)| pos.y).min().unwrap();
	let min_z = items.iter().map(|(pos, _)| pos.z).min().unwrap();
	let max_x = items.iter().map(|(pos, _)| pos.x).max().unwrap();
	let max_y = items.iter().map(|(pos, _)| pos.y).max().unwrap();
	let max_z = items.iter().map(|(pos, _)| pos.z).max().unwrap();
	(Vector3::new(min_x, min_y, min_z), Vector3::new(max_x, max_y, max_z))
}

pub(super) fn tree_schematic(roles :&BlockRoles) -> Schematic {
	let mut items = Vec::new();
	for x in -1 ..= 1 {
		for y in -1 ..= 1 {
			items.push((Vector3::new(x, y, 3), roles.leaves));
			items.push((Vector3::new(x, y, 4), roles.leaves));
			items.push((Vector3::new(x, y, 5), roles.leaves));
		}
	}
	for z in 0 .. 4 {
		items.push((Vector3::new(0, 0, z), roles.tree));
	}
	Schematic::from_items(items)
}

pub(super) fn cactus_schematic(roles :&BlockRoles) -> Schematic {
	let mut items = Vec::new();
	for z in 0 .. 4 {
		items.push((Vector3::new(0, 0, z), roles.cactus));
	}
	Schematic::from_items(items)
}

fn spawn_schematic_mapgen(map :&mut MapgenMap, pos :Vector3<isize>,
		schematic :&Schematic) {
	for (bpos, mb) in schematic.items.iter() {
		let blk = map.get_blk_p1_mut(pos + bpos).unwrap();
		*blk = *mb;
	}
}

impl MapgenMap {
	pub fn new(seed :u64, params :ServerGameParamsHdl,
			storage :DynStorageBackend) -> Self {
		MapgenMap {
			seed,
			params,
			chunks : HashMap::new(),
			storage,
		}
	}
	pub fn get_chunk_p1(&self, pos :Vector3<isize>) -> Option<&MapChunk> {
		self.chunks.get(&pos)
	}
	pub fn get_chunk_p1_mut(&mut self, pos :Vector3<isize>) -> Option<&mut MapChunk> {
		self.chunks.get_mut(&pos)
	}
	fn gen_chunk_phase_one(&mut self, pos :Vector3<isize>) {
		if let Entry::Vacant(v) = self.chunks.entry(pos) {
			v.insert(gen_chunk_phase_one(self.seed, pos, &self.params));
		}
	}
	fn gen_chunk_phase_two(&mut self, pos :Vector3<isize>) {
		let tree_spawn_points = {
			let chnk = self.chunks.get_mut(&pos).unwrap();
			if chnk.generation_phase >= GenerationPhase::PhaseTwo {
				return;
			}
			chnk.generation_phase = GenerationPhase::PhaseTwo;
			replace(&mut chnk.tree_spawn_points, Vec::new())
		};
		// We clone the RC because of Rust's aliasing rules
		let schematics = &self.params.clone().p.schematics;
		for (p, in_desert) in tree_spawn_points {
			if in_desert {
				spawn_schematic_mapgen(self, p, &schematics.cactus_schematic);
			} else {
				spawn_schematic_mapgen(self, p, &schematics.tree_schematic);
			}
		}
	}

	#[allow(unused)]
	fn get_blk_p1(&self, pos :Vector3<isize>) -> Option<MapBlock> {
		let chunk_pos = btchn(pos);
		let pos_in_chunk = btpic(pos);
		self.get_chunk_p1(chunk_pos)
			.map(|blk| *blk.get_blk(pos_in_chunk))
	}
	fn get_blk_p1_mut(&mut self, pos :Vector3<isize>) -> Option<&mut MapBlock> {
		let chunk_pos = btchn(pos);
		let pos_in_chunk = btpic(pos);
		self.get_chunk_p1_mut(chunk_pos)
			.map(|blk| blk.get_blk_mut(pos_in_chunk))
	}


	fn gen_chunks_in_area<F :FnMut(Vector3<isize>, &MapChunkData)>(&mut self,
			pos_min :Vector3<isize>, pos_max :Vector3<isize>, f :&mut F) {

		let pos_min = pos_min.map(|v| v / CHUNKSIZE);
		let pos_max = pos_max.map(|v| v / CHUNKSIZE);

		let mut sth_to_generate = false;

		let ex = 2;
		for x in pos_min.x - ex ..= pos_max.x + ex {
			for y in pos_min.y - ex ..= pos_max.y + ex {
				for z in pos_min.z - ex ..= pos_max.z + ex {
					let pos = Vector3::new(x, y, z) * CHUNKSIZE;
					if let Some(c) = self.chunks.get(&pos) {
						if (pos_min.x .. pos_max.x).contains(&x) &&
								(pos_min.y .. pos_max.y).contains(&y) &&
								(pos_min.z .. pos_max.z).contains(&z) {
							if c.generation_phase != GenerationPhase::Done {
								sth_to_generate = true;
							}
						}
					} else {
						let m = &self.params.p.name_id_map;
						if let Some(data) = self.storage.load_chunk(pos, m).unwrap() {
							let chn = MapChunk {
								data,
								generation_phase : GenerationPhase::Done,
								tree_spawn_points : Vec::new(),
							};
							f(pos, &chn.data);
							self.chunks.insert(pos, chn);
						} else {
							if (pos_min.x .. pos_max.x).contains(&x) &&
									(pos_min.y .. pos_max.y).contains(&y) &&
									(pos_min.z .. pos_max.z).contains(&z) {
								sth_to_generate = true;
							}
						}
					}
				}
			}
		}
		if !sth_to_generate {
			return;
		}

		let s = 2;
		for x in pos_min.x - s ..= pos_max.x + s {
			for y in pos_min.y - s ..= pos_max.y + s {
				for z in pos_min.z - s ..= pos_max.z + s {
					let pos = Vector3::new(x, y, z) * CHUNKSIZE;
					self.gen_chunk_phase_one(pos);
				}
			}
		}
		let t = 1;
		for x in pos_min.x - t ..= pos_max.x + t {
			for y in pos_min.y - t ..= pos_max.y + t {
				for z in pos_min.z - t ..= pos_max.z + t {
					let pos = Vector3::new(x, y, z) * CHUNKSIZE;
					self.gen_chunk_phase_two(pos);
				}
			}
		}
		for x in pos_min.x ..= pos_max.x {
			for y in pos_min.y ..= pos_max.y {
				for z in pos_min.z ..= pos_max.z {
					let pos = Vector3::new(x, y, z) * CHUNKSIZE;
					let chk = self.chunks.get_mut(&pos).unwrap();
					if chk.generation_phase != GenerationPhase::Done {
						chk.generation_phase = GenerationPhase::Done;
						self.storage.store_chunk(pos, &chk.data).unwrap();
						f(pos, &chk.data);
					}
				}
			}
		}
	}
}

pub enum MapgenMsg {
	ChunkChanged(Vector3<isize>, MapChunkData),
	Tick,
	GenArea(Vector3<isize>, Vector3<isize>),
	SetPlayerKv(PlayerIdPair, String, Vec<u8>),
	GetPlayerKv(PlayerIdPair, String, u32),
}

pub struct MapgenThread {
	area_s :Sender<MapgenMsg>,
	result_r :Receiver<(Vector3<isize>, MapChunkData)>,
	result_kv_r :Receiver<(PlayerIdPair, u32, String, Option<Vec<u8>>)>,
}

impl MapgenThread {
	pub fn new(seed :u64, params :ServerGameParamsHdl,
			storage :DynStorageBackend) -> Self {
		let mut mapgen_map = MapgenMap::new(seed, params, storage);
		let (area_s, area_r) = channel();
		let (result_s, result_r) = channel();
		let (result_kv_s, result_kv_r) = channel();
		thread::spawn(move || {
			while let Ok(msg) = area_r.recv() {
				match msg {
					MapgenMsg::ChunkChanged(pos, data) => {
						mapgen_map.storage.store_chunk(pos, &data).unwrap();
					},
					MapgenMsg::Tick => {
						mapgen_map.storage.tick().unwrap();
					},
					MapgenMsg::GenArea(pos_min, pos_max) => {
						mapgen_map.gen_chunks_in_area(pos_min, pos_max, &mut |pos, chk|{
							result_s.send((pos, chk.clone())).unwrap();
						})
					},
					MapgenMsg::SetPlayerKv(id_pair, key, content) => {
						mapgen_map.storage.set_player_kv(id_pair, &key, &content).unwrap();
					},
					MapgenMsg::GetPlayerKv(id, key, payload) => {
						let res = mapgen_map.storage.get_player_kv(id, &key).unwrap();
						result_kv_s.send((id, payload, key, res)).unwrap();
					},
				}
			}
		});
		MapgenThread {
			area_s,
			result_r,
			result_kv_r,
		}
	}
}

impl MapBackend for MapgenThread {
	fn gen_chunks_in_area(&mut self, pos_min :Vector3<isize>,
			pos_max :Vector3<isize>) {
		self.area_s.send(MapgenMsg::GenArea(pos_min, pos_max)).unwrap();
	}
	fn run_for_generated_chunks<F :FnMut(Vector3<isize>, &MapChunkData)>(&mut self,
			f :&mut F) {
		while let Ok((pos, chk)) = self.result_r.try_recv() {
			f(pos, &chk);
		}
		// This is called once per tick
		self.area_s.send(MapgenMsg::Tick).unwrap();
	}
	fn chunk_changed(&mut self, pos :Vector3<isize>, data :MapChunkData) {
		self.area_s.send(MapgenMsg::ChunkChanged(pos, data)).unwrap();
	}
	fn set_player_kv(&mut self, id :PlayerIdPair, key :&str, value :Vec<u8>) {
		self.area_s.send(MapgenMsg::SetPlayerKv(id, key.to_owned(), value)).unwrap();
	}
	fn get_player_kv(&mut self, id: PlayerIdPair, key :&str, payload :u32) {
		self.area_s.send(MapgenMsg::GetPlayerKv(id, key.to_owned(), payload)).unwrap();
	}
	fn run_for_kv_results<F :FnMut(PlayerIdPair, u32, String, Option<Vec<u8>>)>(&mut self, f :&mut F) {
		while let Ok((id, payload, key, value)) = self.result_kv_r.try_recv() {
			f(id, payload, key, value);
		}
	}
}

impl Map<MapgenThread> {
	pub fn new(seed :u64, params :ServerGameParamsHdl,
			storage :DynStorageBackend) -> Self {
		Map::from_backend(MapgenThread::new(seed, params, storage))
	}
}
