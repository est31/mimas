#![forbid(unsafe_code)]

extern crate nalgebra;
#[macro_use]
extern crate glium;
extern crate mehlon_server;

use mehlon_server::map::{MapChunkData,
	CHUNKSIZE, MapBlock};
use nalgebra::Vector3;

#[derive(Copy, Clone)]
pub struct Vertex {
	pub position :[f32; 3],
	pub color :[f32; 4],
	pub normal :[f32; 3],
}

implement_vertex!(Vertex, position, color, normal);

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
fn push_block_internal<F :FnMut([isize; 3]) -> bool>(r :&mut Vec<Vertex>, [x, y, z] :[f32; 3], color :[f32; 4], colorh :[f32; 4], siz :f32, mut blocked :F) {
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
	/*
	// X-Y face (z+1)
	if !blocked([0, 0, 1]) {
		push_face_rev!((x, y, z + siz), (siz, 0.0, siz, 0.0), color);
	}
	*/
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
pub fn get_color_for_blk(blk :MapBlock) -> Option<[f32; 4]> {
	match blk {
		MapBlock::Air => None,
		MapBlock::Ground => Some([0.0, 1.0, 0.0, 1.0]),
		MapBlock::Water => Some([0.0, 0.0, 1.0, 1.0]),
		MapBlock::Wood => Some([0.5, 0.25, 0.0, 1.0]),
		MapBlock::Stone => Some([0.5, 0.5, 0.5, 1.0]),
		MapBlock::Tree => Some([0.38, 0.25, 0.125, 1.0]),
		MapBlock::Leaves => Some([0.0, 0.4, 0.0, 1.0]),
		MapBlock::Coal => Some([0.05, 0.05, 0.05, 1.0]),
	}
}

#[inline]
pub fn colorh(col :[f32; 4]) -> [f32; 4] {
	[col[0]/2.0, col[1]/2.0, col[2]/2.0, col[3]]
}

pub fn mesh_for_chunk(offs :Vector3<isize>, chunk :&MapChunkData) -> Vec<Vertex> {
	let mut r = Vec::new();
	for x in 0 .. CHUNKSIZE {
		for y in 0 .. CHUNKSIZE {
			for z in 0 .. CHUNKSIZE {
				let blk = *chunk.get_blk(Vector3::new(x, y, z));
				let color = if let Some(color) = get_color_for_blk(blk) {
					color
				} else {
					continue;
				};

				let pos = offs + Vector3::new(x, y, z);
				let fpos = [pos.x as f32, pos.y as f32, pos.z as f32];
				let colorh = colorh(color);
				push_block_internal(&mut r, fpos, color, colorh, 1.0, |[xo, yo, zo]| {
					let pos = Vector3::new(x + xo, y + yo, z + zo);
					let outside = pos.map(|v| v < 0 || v >= CHUNKSIZE);
					if outside.x || outside.y || outside.z {
						return false;
					}
					match *chunk.get_blk(pos) {
						MapBlock::Air => {
							false
						},
						_ => true,
					}
				});
			}
		}
	}
	let siz = 1.0;
	for z in 0 .. CHUNKSIZE {
		for x in 0 .. CHUNKSIZE {
			let mut last = None;
			for y in 0 .. CHUNKSIZE {
				let blk = *chunk.get_blk(Vector3::new(x, y, z));
				let mut color = get_color_for_blk(blk);
				let blocked = |[xo, yo, zo]: [isize; 3]| {
					let pos = Vector3::new(x + xo, y + yo, z + zo);
					let outside = pos.map(|v| v < 0 || v >= CHUNKSIZE);
					if outside.x || outside.y || outside.z {
						return false;
					}
					match *chunk.get_blk(pos) {
						MapBlock::Air => {
							false
						},
						_ => true,
					}
				};
				if color.is_some() && blocked([0, 0, 1]) {
					color = None;
				}
				let pos = offs + Vector3::new(x, y, z);
				let (x, y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				match (color, last).clone() {
					(None, Some((last_y, l_col))) => {
						// Some mesh ends here. Emit it.
						let ylen = y - last_y;
						rpush_face_rev!(r, (x, last_y, z + siz), (siz, 0.0, ylen, 0.0), l_col);
						last = None;
					},
					(Some(color), Some((last_y, l_col))) => {
						if color != l_col {
							// Color changed. Emit the old color.
							let ylen = y - last_y;
							rpush_face_rev!(r, (x, last_y, z + siz), (siz, 0.0, ylen, 0.0), l_col);
							last = Some((y, color));
						} else {
							// Color is the same. do nothing.
						}
					},
					// Start a new thing.
					(Some(color), None) => {
						last = Some((y, color));
					},
					// Nothing to do if there is no color
					// and no last color
					(None, None) => (),
				}

			}
			if let Some((last_y, l_col)) = last {
				// Emit if we reached the end
				let (x, z) = ((offs.x + x) as f32, (offs.z + z) as f32);
				let ylen = (offs.y + CHUNKSIZE) as f32 - last_y;
				rpush_face_rev!(r, (x, last_y, z + siz), (siz, 0.0, ylen, 0.0), l_col);
			}
		}
	}
	r
}
