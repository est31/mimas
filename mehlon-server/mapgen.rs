use nalgebra::Vector3;
use noise::{Perlin, NoiseFn, Seedable};
use std::collections::{HashMap, hash_map::Entry};
use std::mem::replace;
use std::hash::Hasher;
use {btchn, btpic};
use rand_pcg::Pcg32;
use rand::Rng;
use fasthash::{MetroHasher, FastHasher};

use super::map::{Map, MapChunkData, MapBlock, MapBackend, CHUNKSIZE};

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
	tree_spawn_points :Vec<Vector3<isize>>,
}

pub struct MapgenMap {
	seed :u32,
	chunks :HashMap<Vector3<isize>, MapChunk>,
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
	let mut mh :MetroHasher = MetroHasher::new();
	mh.write_isize(pos.x);
	mh.write_isize(pos.y);
	mh.write_isize(pos.z);
	mh.finish()
}

fn gen_chunk_phase_one(seed :u32, pos :Vector3<isize>) -> MapChunk {
	let mut seeder = Pcg32::new(seed.wrapping_add(24) as u64, seed.wrapping_add(400) as u64);
	// Basic chunk noise
	let f = 0.02356;
	let noise = NoiseMag::new(seeder.gen::<u32>(), f, 8.3);
	// Macro noise
	let mf = 0.0018671;
	let mnoise = NoiseMag::new(seeder.gen::<u32>(), mf, 23.27713);
	// Super macro noise
	let smf = 0.00043571;
	let smnoise = NoiseMag::new(seeder.gen::<u32>(), smf, 137.479131);
	// Tree noise
	let tf = 0.0088971;
	let tnoise = Noise::new(seeder.gen::<u32>(), tf);
	// Macro tree noise
	let mtf = 0.00093952;
	let mtnoise = Noise::new(seeder.gen::<u32>(), mtf);
	// Tree pcg
	let mut tpcg = Pcg32::new(seeder.gen::<u64>(), pos_hash(pos));
	// Coal noise
	let cf = 0.033951;
	let cnoise = Noise::new(seeder.gen::<u32>(), cf);
	// Cave noise
	let ca_f = 0.052951;
	let ca_noise = Noise::new(seeder.gen::<u32>(), ca_f);

	let mut res = MapChunk {
		data : MapChunkData([MapBlock::Air; (CHUNKSIZE * CHUNKSIZE * CHUNKSIZE) as usize]),
		generation_phase : GenerationPhase::PhaseOne,
		tree_spawn_points : Vec::new(),
	};
	for x in 0 .. CHUNKSIZE {
		for y in 0 .. CHUNKSIZE {
			let p = [(pos.x + x) as f64, (pos.y + y) as f64];
			let elev = noise.get(p) + mnoise.get(p) + smnoise.get(p);
			let elev_blocks = elev as isize;
			if let Some(elev_blocks) = elev_blocks.checked_sub(pos.z) {
				let el = std::cmp::max(std::cmp::min(elev_blocks, CHUNKSIZE), 0);
				if pos.z < 0 {
					for z in 0 .. el {
						*res.get_blk_mut(Vector3::new(x, y, z)) = MapBlock::Stone;
						let p3 = [(pos.x + x) as f64, (pos.y + y) as f64, (pos.z + z) as f64];
						if cnoise.get_3d(p3) > 0.8 {
							*res.get_blk_mut(Vector3::new(x, y, z)) = MapBlock::Coal;
						}
						// Generate caves,
						// but make sure that there is a distance
						// between the cave and where the water starts.
						// We need to compare with elev_blocks instead of el
						// so that there are no artifacts introduced by the
						// maxing with CHUNKSIZE above.
						if z + 10 < elev_blocks && ca_noise.get_3d(p3) > 0.45 {
							*res.get_blk_mut(Vector3::new(x, y, z)) = MapBlock::Air;
						}
					}
					for z in  el .. CHUNKSIZE {
						*res.get_blk_mut(Vector3::new(x, y, z)) = MapBlock::Water;
					}
				} else {
					for z in 0 .. el {
						*res.get_blk_mut(Vector3::new(x, y, z)) = MapBlock::Ground;
					}
					if pos.z == 0 && el <= 0 {
						*res.get_blk_mut(Vector3::new(x, y, 0)) = MapBlock::Water;
					}
					if el > 0 && el < CHUNKSIZE {
						// Tree spawning
						let tree_density = 0.4;
						let macro_density = mtnoise.get(p);
						let macro_density = if macro_density < 0.0 {
							0.0
						} else {
							macro_density
						};
						let local_density = tnoise.get(p) + macro_density;

						if local_density > 1.0 - tree_density {
							// Generate a forest here
							if tpcg.gen::<f64>() > 0.9 {
								res.tree_spawn_points.push(pos + Vector3::new(x, y, el));
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

pub struct Schematic {
	pub(super) items :Vec<(Vector3<isize>, MapBlock)>,
	pub(super) aabb_min :Vector3<isize>,
	pub(super) aabb_max :Vector3<isize>,
}

lazy_static! {
    pub static ref TREE_SCHEMATIC :Schematic = tree_schematic();
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

fn tree_schematic() -> Schematic {
	let mut items = Vec::new();
	for x in -1 ..= 1 {
		for y in -1 ..= 1 {
			items.push((Vector3::new(x, y, 3), MapBlock::Leaves));
			items.push((Vector3::new(x, y, 4), MapBlock::Leaves));
			items.push((Vector3::new(x, y, 5), MapBlock::Leaves));
		}
	}
	for z in 0 .. 4 {
		items.push((Vector3::new(0, 0, z), MapBlock::Tree));
	}
	let (aabb_min, aabb_max) = aabb_min_max(&items);
	Schematic {
		items,
		aabb_min,
		aabb_max,
	}
}

fn spawn_schematic_mapgen(map :&mut MapgenMap, pos :Vector3<isize>, schematic :&Schematic) {
	for (bpos, mb) in schematic.items.iter() {
		let blk = map.get_blk_p1_mut(pos + bpos).unwrap();
		*blk = *mb;
	}
}
fn spawn_tree_mapgen(map :&mut MapgenMap, pos :Vector3<isize>) {
	spawn_schematic_mapgen(map, pos, &TREE_SCHEMATIC);
}

impl MapBackend for MapgenMap {
	fn gen_chunks_in_area<F :FnMut(Vector3<isize>, &MapChunkData)>(&mut self, pos_min :Vector3<isize>,
			pos_max :Vector3<isize>, f :&mut F) {
		let pos_min = pos_min.map(|v| v / CHUNKSIZE);
		let pos_max = pos_max.map(|v| v / CHUNKSIZE);

		let mut sth_to_generate = false;
		'o :for x in pos_min.x ..= pos_max.x {
			for y in pos_min.y ..= pos_max.y {
				for z in pos_min.z ..= pos_max.z {
					let pos = Vector3::new(x, y, z) * CHUNKSIZE;
					if let Some(c) = self.chunks.get(&pos) {
						if c.generation_phase != GenerationPhase::Done {
							sth_to_generate = true;
							break 'o;
						}
					} else {
						sth_to_generate = true;
						break 'o;
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
						f(pos, &chk.data);
					}
				}
			}
		}
	}
}

impl MapgenMap {
	pub fn new(seed :u32) -> Self {
		MapgenMap {
			seed,
			chunks : HashMap::new(),
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
			v.insert(gen_chunk_phase_one(self.seed, pos));
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
		for p in tree_spawn_points {
			spawn_tree_mapgen(self, p);
		}
	}

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
}

impl Map<MapgenMap> {
	pub fn new(seed :u32) -> Self {
		Map::from_backend(MapgenMap::new(seed))
	}
}
