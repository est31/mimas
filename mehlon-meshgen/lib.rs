#![forbid(unsafe_code)]

extern crate nalgebra;
#[macro_use]
extern crate glium;
extern crate mehlon_server;

use mehlon_server::map::{MapChunkData,
	CHUNKSIZE};
use mehlon_server::game_params::GameParamsHdl;
use mehlon_server::map::MapBlock;
use nalgebra::Vector3;

#[derive(Copy, Clone)]
pub struct Vertex {
	pub position :[f32; 3],
	pub color :[f32; 4],
	pub normal :[f32; 3],
}

implement_vertex!(Vertex, position, color, normal);

pub struct ColorCache {
	colors :Vec<Option<[f32; 4]>>,
}

impl ColorCache {
	pub fn from_hdl(hdl :&GameParamsHdl) -> Self {
		let colors = hdl.block_params.iter()
			.map(|p| p.color)
			.collect::<Vec<_>>();
		Self {
			colors
		}
	}
	fn get_color(&self, bl :&MapBlock) -> Option<[f32; 4]> {
		self.colors.get(bl.id() as usize)
			.map(|v| *v)
			.unwrap_or(Some([0.0, 0.0, 0.0, 1.0]))
	}
}

// This is NOT the same function as f32::signum!
// For -0.0, and 0.0, this function returns 0.0,
// while f32::signum returns -1.0 and 1.0.
fn zsig(v :f32) -> f32 {
	if v > 0.0 {
		return 1.0;
	}
	if v < 0.0 {
		return -1.0;
	}
	0.0
}

macro_rules! sign {
	($x:expr, $y:expr, $z:expr) => {
		[zsig($x), zsig($y), zsig($z)]
	};
}

macro_rules! rpush_face {
	($r:expr, ($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $color:expr) => {
		$r.push(Vertex { position: [$x, $y, $z], color : $color, normal : sign![$xsd, $ysd + $yd, $zd] });
		$r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color, normal : sign![$xsd, $ysd + $yd, $zd] });
		$r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color, normal : sign![$xsd, $ysd + $yd, $zd] });

		$r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color, normal : sign![$xsd, $ysd + $yd, $zd] });
		$r.push(Vertex { position: [$x + $xsd, $y + $yd + $ysd, $z + $zd], color: $color, normal : sign![$xsd, $ysd + $yd, $zd] });
		$r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color, normal : sign![$xsd, $ysd + $yd, $zd] });
	}
}
macro_rules! rpush_face_rev {
	($r:expr, ($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $color:expr) => {
		$r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color, normal : sign![-$xsd, -$ysd - $yd, -$zd] });
		$r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color, normal : sign![-$xsd, -$ysd - $yd, -$zd] });
		$r.push(Vertex { position: [$x, $y, $z], color : $color, normal : sign![-$xsd, -$ysd - $yd, -$zd] });

		$r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color, normal : sign![-$xsd, -$ysd - $yd, -$zd] });
		$r.push(Vertex { position: [$x + $xsd, $y + $yd + $ysd, $z + $zd], color: $color, normal : sign![-$xsd, -$ysd - $yd, -$zd] });
		$r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color, normal : sign![-$xsd, -$ysd - $yd, -$zd] });
	}
}

#[inline]
pub fn push_block<F :FnMut([isize; 3]) -> bool>(r :&mut Vec<Vertex>, [x, y, z] :[f32; 3], color :[f32; 4], colorh :[f32; 4], siz :f32, mut blocked :F) {
	macro_rules! push_face {
		(($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $color:expr) => {
			rpush_face!(r, ($x, $y, $z), ($xsd, $ysd, $yd, $zd), $color);
		};
	}
	macro_rules! push_face_rev {
		(($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $color:expr) => {
			rpush_face_rev!(r, ($x, $y, $z), ($xsd, $ysd, $yd, $zd), $color);
		};
	}
	// X-Y face
	if !blocked([0, 0, -1]) {
		push_face!((x, y, z), (siz, 0.0, siz, 0.0), color);
	}
	// X-Z face
	if !blocked([0, -1, 0]) {
		push_face_rev!((x, y, z), (siz, 0.0, 0.0, siz), colorh);
	}
	// Y-Z face
	if !blocked([-1, 0, 0]) {
		push_face!((x, y, z), (0.0, siz, 0.0, siz), colorh);
	}
	// X-Y face (z+1)
	if !blocked([0, 0, 1]) {
		push_face_rev!((x, y, z + siz), (siz, 0.0, siz, 0.0), color);
	}
	// X-Z face (y+1)
	if !blocked([0, 1, 0]) {
		push_face!((x, y + siz, z), (siz, 0.0, 0.0, siz), colorh);
	}
	// Y-Z face (x+1)
	if !blocked([1, 0, 0]) {
		push_face_rev!((x + siz, y, z), (0.0, siz, 0.0, siz), colorh);
	}
}

#[inline]
pub fn colorh(col :[f32; 4]) -> [f32; 4] {
	[col[0]/2.0, col[1]/2.0, col[2]/2.0, col[3]]
}

pub fn mesh_for_chunk(offs :Vector3<isize>, chunk :&MapChunkData,
		cache :&ColorCache) -> Vec<Vertex> {
	let mut r = Vec::new();

	struct Walker<D> {
		last :Option<(f32, D)>,
	}
	impl<D :PartialEq + Copy> Walker<D> {
		fn new() -> Self {
			Walker {
				last : None,
			}
		}
		fn next<F :FnOnce(D, f32, f32)>(&mut self,
				v :f32, item :Option<D>, emit :F) {
			match (item, self.last) {
				(None, Some((last_v, l_item))) => {
					// Some mesh ends here. Emit it.
					let vlen = v - last_v;
					emit(l_item, last_v, vlen);
					self.last = None;
				},
				(Some(item), Some((last_v, l_item))) => {
					if item != l_item {
						// Item changed. Emit the old item.
						let vlen = v - last_v;
						emit(l_item, last_v, vlen);
						self.last = Some((v, item));
					} else {
						// Item is the same. do nothing.
					}
				},
				// Start a new thing.
				(Some(item), None) => {
					self.last = Some((v, item));
				},
				// Nothing to do if there is no item
				// and no last item
				(None, None) => (),
			}
		}
	}
	fn blocked(chunk :&MapChunkData,
			[xo, yo, zo] :[isize; 3], pos :Vector3<isize>,
			cache :&ColorCache) -> bool {
		let pos = Vector3::new(pos.x + xo, pos.y + yo, pos.z + zo);
		let outside = pos.map(|v| v < 0 || v >= CHUNKSIZE);
		if outside.x || outside.y || outside.z {
			return false;
		}
		let blk = chunk.get_blk(pos);
		cache.get_color(blk).is_some()
	};
	fn get_col(chunk: &MapChunkData, pos :Vector3<isize>,
			offsets :[isize; 3], cache :&ColorCache) -> Option<[f32; 4]> {
		let blk = chunk.get_blk(pos);
		let mut color = cache.get_color(blk);
		if color.is_some() && blocked(chunk, offsets, pos, cache) {
			color = None;
		}
		color
	};
	fn walk_for_all_blocks<G :FnMut(&mut Walker<[f32; 4]>, Option<[f32; 4]>, Vector3<isize>)>(
			f :fn(isize, isize, isize) -> Vector3<isize>,
			offsets :[isize; 3],
			chunk :&MapChunkData, g :&mut G,
			cache :&ColorCache) {
		for c1 in 0 .. CHUNKSIZE {
			for c2 in 0 .. CHUNKSIZE {
				let mut walker = Walker::new();
				for cinner in 0 .. CHUNKSIZE {
					let rel_pos = f(c1, c2, cinner);
					let color = get_col(chunk, rel_pos, offsets, cache);
					g(&mut walker, color, rel_pos)
				}
				let rel_pos = f(c1, c2, CHUNKSIZE);
				g(&mut walker, None, rel_pos)
			}
		}
	}
	let siz = 1.0;

	// X-Y face (unify over y)
	walk_for_all_blocks(
		|c1, c2, cinner| Vector3::new(c1, cinner, c2),
		[0, 0, -1],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.y as f32, color, |l_col, last_y, ylen| {
				let (x, _y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				rpush_face!(r, (x, last_y, z), (siz, 0.0, ylen, 0.0), l_col);
			});
		},
		cache
	);

	// X-Z face (unify over x)
	walk_for_all_blocks(
		|c1, c2, cinner| Vector3::new(cinner, c1, c2),
		[0, -1, 0],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.x as f32, color, |l_col, last_x, xlen| {
				let (_x, y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				let colorh = colorh(l_col);
				rpush_face_rev!(r, (last_x, y, z), (xlen, 0.0, 0.0, siz), colorh);
			});
		},
		cache
	);

	// Y-Z face (unify over y)
	walk_for_all_blocks(
		|c1, c2, cinner| Vector3::new(c1, cinner, c2),
		[-1, 0, 0],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.y as f32, color, |l_col, last_y, ylen| {
				let (x, _y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				let colorh = colorh(l_col);
				rpush_face!(r, (x, last_y, z), (0.0, ylen, 0.0, siz), colorh);
			});
		},
		cache
	);

	// X-Y face (z+1) (unify over y)
	walk_for_all_blocks(
		|c1, c2, cinner| Vector3::new(c1, cinner, c2),
		[0, 0, 1],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.y as f32, color, |l_col, last_y, ylen| {
				let (x, _y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				rpush_face_rev!(r, (x, last_y, z + siz), (siz, 0.0, ylen, 0.0), l_col);
			});
		},
		cache
	);

	// X-Z face (y+1) (unify over x)
	walk_for_all_blocks(
		|c1, c2, cinner| Vector3::new(cinner, c1, c2),
		[0, 1, 0],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.x as f32, color, |l_col, last_x, xlen| {
				let (_x, y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				let colorh = colorh(l_col);
				rpush_face!(r, (last_x, y + siz, z), (xlen, 0.0, 0.0, siz), colorh);
			});
		},
		cache
	);

	// Y-Z face (x+1) (unify over y)
	walk_for_all_blocks(
		|c1, c2, cinner| Vector3::new(c1, cinner, c2),
		[1, 0, 0],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.y as f32, color, |l_col, last_y, ylen| {
				let (x, _y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				let colorh = colorh(l_col);
				rpush_face_rev!(r, (x + siz, last_y, z), (0.0, ylen, 0.0, siz), colorh);
			});
		},
		cache
	);

	r
}
