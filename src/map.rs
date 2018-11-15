use cgmath::Vector3;
use noise::{Perlin, NoiseFn, Seedable};
use super::mod_euc;

pub const BLOCKSIZE :isize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapBlock {
	Air,
	Water,
	Ground,
}

impl MapBlock {
	pub fn is_pointable(&self) -> bool {
		match self {
			MapBlock::Water |
			MapBlock::Ground => true,
			_ => false
		}
	}
}

pub struct MapChunk {
	pub data :[MapBlock; (BLOCKSIZE * BLOCKSIZE * BLOCKSIZE) as usize],
}

pub struct Map {
	seed :u32,
	pub chunks :Vec<(Vector3<isize>, MapChunk)>,
}

impl MapChunk {
	pub fn get_blk_mut(&mut self, x :isize, y :isize, z :isize) -> &mut MapBlock {
		&mut self.data[(x * BLOCKSIZE * BLOCKSIZE + y * BLOCKSIZE + z) as usize]
	}
	pub fn get_blk(&self, x :isize, y :isize, z :isize) -> &MapBlock {
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
					*res.get_blk_mut(x, y, 0) = MapBlock::Water;
				}
				for z in 0 .. el {
					*res.get_blk_mut(x, y, z) = MapBlock::Ground;
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
		let square_size = 10;
		for x in 0 .. square_size {
			for y in 0 .. square_size {
				for z in 0 .. 3 {
					let pos = Vector3 {
						x : (x * BLOCKSIZE) as isize,
						y : (y * BLOCKSIZE) as isize,
						z : (z * BLOCKSIZE) as isize,
					};
					let chunk = gen_chunk(self.seed, pos);
					self.chunks.push((pos, chunk));
				}
			}
		}
	}
	pub fn get_blk(&self, x :isize, y :isize, z :isize) -> Option<MapBlock> {
		let blk_pos = Vector3::new(x, y, z) / (BLOCKSIZE as isize);
		let bsf = BLOCKSIZE as f32;
		let (pos_x, pos_y, pos_z) = (mod_euc(x as f32, bsf), mod_euc(y as f32, bsf), mod_euc(z as f32, bsf));
		for (bpos, blk) in self.chunks.iter() {
			if &blk_pos == bpos {
				continue;
			}
			return Some(*blk.get_blk(pos_x as isize, pos_y as isize, pos_z as isize))
		}
		None
	}
	pub fn get_blk_mut(&mut self, x :isize, y :isize, z :isize) -> Option<&mut MapBlock> {
		let blk_pos = Vector3::new(x, y, z) / (BLOCKSIZE as isize);
		let bsf = BLOCKSIZE as f32;
		let (pos_x, pos_y, pos_z) = (mod_euc(x as f32, bsf), mod_euc(y as f32, bsf), mod_euc(z as f32, bsf));
		for (bpos, blk) in self.chunks.iter_mut() {
			if &blk_pos == bpos {
				continue;
			}
			return Some(blk.get_blk_mut(pos_x as isize, pos_y as isize, pos_z as isize))
		}
		None
	}
}
