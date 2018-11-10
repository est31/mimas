use cgmath::Vector3;
use noise::{Perlin, NoiseFn, Seedable};

pub const BLOCKSIZE :usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MapBlock {
	Air,
	Ground,
}

pub struct MapChunk {
	pub data :[MapBlock; BLOCKSIZE * BLOCKSIZE * BLOCKSIZE],
}

pub struct Map {
	seed :u32,
	pub chunks :Vec<(Vector3<usize>, MapChunk)>,
}

impl MapChunk {
	pub fn get_blk_mut(&mut self, x :usize, y :usize, z :usize) -> &mut MapBlock {
		&mut self.data[x * BLOCKSIZE * BLOCKSIZE + y * BLOCKSIZE + z]
	}
	pub fn get_blk(&self, x :usize, y :usize, z :usize) -> &MapBlock {
		&self.data[x * BLOCKSIZE * BLOCKSIZE + y * BLOCKSIZE + z]
	}
}

pub fn gen_chunk(seed :u32, pos :Vector3<usize>) -> MapChunk {
	let noise = Perlin::new().set_seed(seed);
	let mut res = MapChunk {
		data :[MapBlock::Air; BLOCKSIZE * BLOCKSIZE * BLOCKSIZE],
	};
	for x in 0 .. BLOCKSIZE {
		for y in 0 .. BLOCKSIZE {
			let f = 0.1356;
			let p = [(pos.x + x) as f64 * f, (pos.y + y) as f64 * f];
			let elev = noise.get(p) * 8.0;
			let elev_blocks = (::clamp(elev as f32, 0.0, elev as f32)) as usize;
			if let Some(elev_blocks) = elev_blocks.checked_sub(pos.z) {
				let el = std::cmp::min(elev_blocks, BLOCKSIZE);
				if el >= 1 {
				for z in 0 .. el {
					*res.get_blk_mut(x, y, z) = MapBlock::Ground;
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
			chunks : Vec::new(),
		}
	}
	pub fn gen_chunks(&mut self) {
		let square_size = 20;
		for x in 0 .. square_size {
			for y in 0 .. square_size {
				for z in 0 .. 3 {
					let pos = Vector3 {
						x : x * BLOCKSIZE,
						y : y * BLOCKSIZE,
						z : z * BLOCKSIZE,
					};
					let chunk = gen_chunk(self.seed, pos);
					self.chunks.push((pos, chunk));
				}
			}
		}
	}
}
