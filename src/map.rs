use nalgebra::Vector3;
use noise::{Perlin, NoiseFn, Seedable};
use std::collections::HashMap;
use {btchn, btpic};

pub const BLOCKSIZE :isize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapBlock {
	Air,
	Water,
	Ground,
	Wood,
}

impl MapBlock {
	pub fn is_pointable(&self) -> bool {
		match self {
			MapBlock::Water |
			MapBlock::Ground |
			MapBlock::Wood => true,
			_ => false
		}
	}
}

pub struct MapChunk {
	pub data :[MapBlock; (BLOCKSIZE * BLOCKSIZE * BLOCKSIZE) as usize],
}

pub struct Map {
	seed :u32,
	pub chunks :HashMap<Vector3<isize>, MapChunk>,
}

impl MapChunk {
	pub fn get_blk_mut(&mut self, pos :Vector3<isize>) -> &mut MapBlock {
		let (x, y, z) = (pos.x, pos.y, pos.z);
		&mut self.data[(x * BLOCKSIZE * BLOCKSIZE + y * BLOCKSIZE + z) as usize]
	}
	pub fn get_blk(&self, pos :Vector3<isize>) -> &MapBlock {
		let (x, y, z) = (pos.x, pos.y, pos.z);
		&self.data[(x * BLOCKSIZE * BLOCKSIZE + y * BLOCKSIZE + z) as usize]
	}
}

pub fn gen_chunk(seed :u32, pos :Vector3<isize>) -> MapChunk {
	let noise = Perlin::new().set_seed(seed);
	let mut res = MapChunk {
		data :[MapBlock::Air; (BLOCKSIZE * BLOCKSIZE * BLOCKSIZE) as usize],
	};
	for x in 0 .. BLOCKSIZE {
		for y in 0 .. BLOCKSIZE {
			let f = 0.1356;
			let p = [(pos.x + x) as f64 * f, (pos.y + y) as f64 * f];
			let elev = noise.get(p) * 8.0;
			let elev_blocks = (::clamp(elev as f32, 0.0, elev as f32)) as isize;
			if let Some(elev_blocks) = elev_blocks.checked_sub(pos.z) {
				let el = std::cmp::min(elev_blocks, BLOCKSIZE);
				if el == 0 {
					*res.get_blk_mut(Vector3::new(x, y, 0)) = MapBlock::Water;
				}
				for z in 0 .. el {
					*res.get_blk_mut(Vector3::new(x, y, z)) = MapBlock::Ground;
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
	pub fn gen_chunks(&mut self) {
		let square_size = 10;
		for x in 0 .. square_size {
			for y in 0 .. square_size {
				for z in 0 .. 3 {
					let pos = Vector3::new(
						(x * BLOCKSIZE) as isize,
						(y * BLOCKSIZE) as isize,
						(z * BLOCKSIZE) as isize,
					);
					let chunk = gen_chunk(self.seed, pos);
					self.chunks.insert(pos, chunk);
				}
			}
		}
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
