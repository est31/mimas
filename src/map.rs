use nalgebra::Vector3;
use noise::{Perlin, NoiseFn, Seedable};
use std::collections::HashMap;
use {btchn, btpic};

pub const CHUNKSIZE :isize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapBlock {
	Air,
	Water,
	Ground,
	Wood,
	Stone,
	Tree,
	Leaves,
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

#[derive(Copy, Clone)]
pub struct MapChunk {
	pub data :[MapBlock; (CHUNKSIZE * CHUNKSIZE * CHUNKSIZE) as usize],
}

pub struct Map {
	seed :u32,
	pub chunks :HashMap<Vector3<isize>, MapChunk>,
}

impl MapChunk {
	pub fn get_blk_mut(&mut self, pos :Vector3<isize>) -> &mut MapBlock {
		let (x, y, z) = (pos.x, pos.y, pos.z);
		&mut self.data[(x * CHUNKSIZE * CHUNKSIZE + y * CHUNKSIZE + z) as usize]
	}
	pub fn get_blk(&self, pos :Vector3<isize>) -> &MapBlock {
		let (x, y, z) = (pos.x, pos.y, pos.z);
		&self.data[(x * CHUNKSIZE * CHUNKSIZE + y * CHUNKSIZE + z) as usize]
	}
}

pub fn gen_chunk(seed :u32, pos :Vector3<isize>) -> MapChunk {
	let noise = Perlin::new().set_seed(seed);
	let mnoise = Perlin::new().set_seed(seed.wrapping_add(23));
	let mut res = MapChunk {
		data :[MapBlock::Air; (CHUNKSIZE * CHUNKSIZE * CHUNKSIZE) as usize],
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
				}
			}
		}
	}
	res
}

impl Map {
	pub fn new(seed :u32) -> Self {
		Map {
			seed,
			chunks : HashMap::new(),
		}
	}
	pub fn gen_chunks_start(&mut self) {
		let square_size = 10;
		for x in 0 .. square_size {
			for y in 0 .. square_size {
				for z in 0 .. 3 {
					let pos = Vector3::new(x, y, z) * CHUNKSIZE;
					self.gen_chunk(pos);
				}
			}
		}
	}
	pub fn gen_chunk(&mut self, pos :Vector3<isize>) {
		let chunk = gen_chunk(self.seed, pos);
		self.chunks.insert(pos, chunk);
	}
	pub fn get_blk(&self, pos :Vector3<isize>) -> Option<MapBlock> {
		let chunk_pos = btchn(pos);
		let pos_in_chunk = btpic(pos);
		self.chunks.get(&chunk_pos)
			.map(|blk| *blk.get_blk(pos_in_chunk))
	}
	pub fn get_blk_mut(&mut self, pos :Vector3<isize>) -> Option<&mut MapBlock> {
		let chunk_pos = btchn(pos);
		let pos_in_chunk = btpic(pos);
		self.chunks.get_mut(&chunk_pos)
			.map(|blk| blk.get_blk_mut(pos_in_chunk))
	}
}
