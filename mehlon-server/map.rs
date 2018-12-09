use nalgebra::Vector3;
use noise::{Perlin, NoiseFn, Seedable};
use std::collections::{HashMap, hash_map::Entry};
use std::mem::replace;
use {btchn, btpic};
use rand_pcg::Pcg32;
use rand::Rng;

pub const CHUNKSIZE :isize = 16;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapBlock {
	Air,
	Water,
	Ground,
	Wood,
	Stone,
	Tree,
	Leaves,
}

impl Default for MapBlock {
	fn default() -> Self {
		MapBlock::Air
	}
}

impl MapBlock {
	pub fn is_pointable(&self) -> bool {
		match self {
			MapBlock::Water |
			MapBlock::Ground |
			MapBlock::Wood |
			MapBlock::Stone |
			MapBlock::Tree |
			MapBlock::Leaves => true,
			_ => false
		}
	}
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum GenerationPhase {
	/// Basic noise, elevation etc
	PhaseOne,
	/// Higher level features done
	PhaseTwo,
	/// The block and all of its neighbours are at least in phase two.
	Done,
}

big_array! { BigArray; }

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct MapChunkData(
	#[serde(with = "BigArray")]
	[MapBlock; (CHUNKSIZE * CHUNKSIZE * CHUNKSIZE) as usize]
);



#[derive(Clone)]
pub struct MapChunk {
	pub data :MapChunkData,
	generation_phase :GenerationPhase,
	tree_spawn_points :Vec<Vector3<isize>>,
}

pub struct Map<B :MapBackend> {
	backend :B,
	chunks :HashMap<Vector3<isize>, MapChunkData>,
	on_change :Box<dyn Fn(Vector3<isize>, &MapChunkData)>,
}

pub type ServerMap = Map<MapgenMap>;
pub type ClientMap = Map<ClientBackend>;

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

impl MapChunkData {
	pub fn get_blk_mut(&mut self, pos :Vector3<isize>) -> &mut MapBlock {
		let (x, y, z) = (pos.x, pos.y, pos.z);
		&mut self.0[(x * CHUNKSIZE * CHUNKSIZE + y * CHUNKSIZE + z) as usize]
	}
	pub fn get_blk(&self, pos :Vector3<isize>) -> &MapBlock {
		let (x, y, z) = (pos.x, pos.y, pos.z);
		&self.0[(x * CHUNKSIZE * CHUNKSIZE + y * CHUNKSIZE + z) as usize]
	}
}

fn gen_chunk_phase_one(seed :u32, pos :Vector3<isize>) -> MapChunk {
	let noise = Perlin::new().set_seed(seed);
	let mnoise = Perlin::new().set_seed(seed.wrapping_add(23));
	let tnoise = Perlin::new().set_seed(seed.wrapping_add(99));
	let mut tpcg = Pcg32::new(seed.wrapping_add(53) as u64, seed.wrapping_add(47) as u64);
	let mut res = MapChunk {
		data : MapChunkData([MapBlock::Air; (CHUNKSIZE * CHUNKSIZE * CHUNKSIZE) as usize]),
		generation_phase : GenerationPhase::PhaseOne,
		tree_spawn_points : Vec::new(),
	};
	for x in 0 .. CHUNKSIZE {
		for y in 0 .. CHUNKSIZE {
			let f = 0.02356;
			let p = [(pos.x + x) as f64 * f, (pos.y + y) as f64 * f];
			let mf = 0.0018671;
			let mp = [(pos.x + x) as f64 * mf, (pos.y + y) as f64 * mf];
			let elev = noise.get(p) * 8.3 + mnoise.get(mp) * 23.27713;
			let elev_blocks = elev as isize;
			if let Some(elev_blocks) = elev_blocks.checked_sub(pos.z) {
				let el = std::cmp::max(std::cmp::min(elev_blocks, CHUNKSIZE), 0);
				if pos.z < 0 {
					for z in 0 .. el {
						*res.get_blk_mut(Vector3::new(x, y, z)) = MapBlock::Stone;
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
						let tf = 0.018971;
						let tp = [(pos.x + x) as f64 * tf, (pos.y + y) as f64 * tf];
						let tree_density = 0.3;
						let local_density = tnoise.get(tp);
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

struct Schematic {
	items :Vec<(Vector3<isize>, MapBlock)>,
	aabb_min :Vector3<isize>,
	aabb_max :Vector3<isize>,
}

lazy_static! {
    static ref TREE_SCHEMATIC :Schematic = tree_schematic();
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

fn spawn_schematic<B :MapBackend>(map :&mut Map<B>, pos :Vector3<isize>, schematic :&Schematic) {
	for (bpos, mb) in schematic.items.iter() {
		let mut blk = map.get_blk_mut_no_upd(pos + bpos).unwrap();
		*blk = *mb;
	}
	let pos_min = btchn(pos + schematic.aabb_min);
	let pos_max = btchn(pos + schematic.aabb_max);
	for x in pos_min.x ..= pos_max.x {
		for y in pos_min.y ..= pos_max.y {
			for z in pos_min.z ..= pos_max.z {
				let p = Vector3::new(x, y, z);
				if let Some(chn) = map.get_chunk(p) {
					(map.on_change)(p, &chn);
				}
			}
		}
	}
}

fn spawn_schematic_mapgen(map :&mut MapgenMap, pos :Vector3<isize>, schematic :&Schematic) {
	for (bpos, mb) in schematic.items.iter() {
		let blk = map.get_blk_p1_mut(pos + bpos).unwrap();
		*blk = *mb;
	}
}

pub fn spawn_tree<B :MapBackend>(map :&mut Map<B>, pos :Vector3<isize>) {
	spawn_schematic(map, pos, &TREE_SCHEMATIC);
}

fn spawn_tree_mapgen(map :&mut MapgenMap, pos :Vector3<isize>) {
	spawn_schematic_mapgen(map, pos, &TREE_SCHEMATIC);
}

pub struct MapBlockHandle<'a> {
	pos :Vector3<isize>,
	chk :&'a mut MapChunkData,
	on_change :&'a Box<dyn Fn(Vector3<isize>, &MapChunkData)>,
}

impl<'a> MapBlockHandle<'a> {
	pub fn set(&mut self, b :MapBlock) {
		let chunk_pos = btchn(self.pos);
		let pos_in_chunk = btpic(self.pos);
		*self.chk.get_blk_mut(pos_in_chunk) = b;
		(*self.on_change)(chunk_pos, &self.chk);
	}
}


pub struct ClientBackend;

impl MapBackend for ClientBackend {
	fn gen_chunks_in_area<F :FnMut(Vector3<isize>, &MapChunkData)>(&mut self, _pos_min :Vector3<isize>,
			_pos_max :Vector3<isize>, _f :&mut F) {
		// Do nothing. The server just pushes any chunks.
	}
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

pub trait MapBackend {
	fn gen_chunks_in_area<F :FnMut(Vector3<isize>, &MapChunkData)>(&mut self, pos_min :Vector3<isize>,
			pos_max :Vector3<isize>, f :&mut F);
}

impl Map<MapgenMap> {
	pub fn new(seed :u32) -> Self {
		Map::from_backend(MapgenMap::new(seed))
	}
}

impl Map<ClientBackend> {
	pub fn new() -> Self {
		Map::from_backend(ClientBackend)
	}
}

impl<B :MapBackend> Map<B> {
	pub fn from_backend(backend :B) -> Self {
		Map {
			backend,
			chunks : HashMap::new(),
			on_change : Box::new(|_, _| {}),
		}
	}
	pub fn register_on_change(&mut self, f :Box<dyn Fn(Vector3<isize>, &MapChunkData)>) {
		self.on_change = f;
	}
	pub fn get_chunk(&self, pos :Vector3<isize>) -> Option<&MapChunkData> {
		self.chunks.get(&pos)
	}
	fn get_chunk_mut(&mut self, pos :Vector3<isize>) -> Option<&mut MapChunkData> {
		self.chunks.get_mut(&pos)
	}
	pub fn set_chunk(&mut self, pos :Vector3<isize>, data :MapChunkData) {
		self.chunks.insert(pos, data);
		(self.on_change)(pos, &data);
	}
	pub fn gen_chunks_in_area(&mut self, pos_min :Vector3<isize>,
			pos_max :Vector3<isize>) {
		let on_change = &self.on_change;
		let chunks = &mut self.chunks;
		self.backend.gen_chunks_in_area(pos_min, pos_max, &mut |pos, chn :&MapChunkData| {
			chunks.insert(pos, *chn);
			on_change(pos, chn);
		});
	}
	pub fn get_blk(&self, pos :Vector3<isize>) -> Option<MapBlock> {
		let chunk_pos = btchn(pos);
		let pos_in_chunk = btpic(pos);
		self.get_chunk(chunk_pos)
			.map(|blk| *blk.get_blk(pos_in_chunk))
	}
	pub fn get_blk_mut_no_upd(&mut self, pos :Vector3<isize>) -> Option<&mut MapBlock> {
		let chunk_pos = btchn(pos);
		let pos_in_chunk = btpic(pos);
		self.get_chunk_mut(chunk_pos)
			.map(|blk| blk.get_blk_mut(pos_in_chunk))
	}
	pub fn get_blk_mut<'s>(&'s mut self, pos :Vector3<isize>) -> Option<MapBlockHandle<'s>> {
		let chunk_pos = btchn(pos);
		let on_change = &self.on_change;
		self.chunks.get_mut(&chunk_pos)
			.map(|chk| MapBlockHandle {
				pos,
				chk,
				on_change,
			})
	}
}
