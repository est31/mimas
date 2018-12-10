#![forbid(unsafe_code)]

extern crate nalgebra;
extern crate ncollide3d;
#[macro_use]
extern crate glium;
extern crate mehlon_server;

use mehlon_server::map::{MapChunkData,
	CHUNKSIZE, MapBlock};
use nalgebra::{Vector3, Isometry3};
use ncollide3d::shape::{Cuboid, Compound, ShapeHandle};

#[derive(Copy, Clone)]
pub struct Vertex {
	pub position :[f32; 3],
	pub color :[f32; 4],
}

implement_vertex!(Vertex, position, color);

#[inline]
pub fn push_block<F :FnMut([isize; 3]) -> bool>(r :&mut Vec<Vertex>, [x, y, z] :[f32; 3], color :[f32; 4], colorh :[f32; 4], siz :f32, mut blocked :F) {
	macro_rules! push_face {
		(($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $color:expr) => {
		r.push(Vertex { position: [$x, $y, $z], color : $color });
		r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color });
		r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color });

		r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color });
		r.push(Vertex { position: [$x + $xsd, $y + $yd + $ysd, $z + $zd], color: $color });
		r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color });
		}
	};
	macro_rules! push_face_rev {
		(($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $color:expr) => {
		r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color });
		r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color });
		r.push(Vertex { position: [$x, $y, $z], color : $color });

		r.push(Vertex { position: [$x, $y + $yd, $z + $zd], color : $color });
		r.push(Vertex { position: [$x + $xsd, $y + $yd + $ysd, $z + $zd], color: $color });
		r.push(Vertex { position: [$x + $xsd, $y + $ysd, $z], color : $color });
		}
	};
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

fn mesh_for_chunk<F :FnMut(Vector3<isize>)>(offs :Vector3<isize>, chunk :&MapChunkData, mut f :F) ->
		Vec<Vertex> {
	let mut r = Vec::new();
	for x in 0 .. CHUNKSIZE {
		for y in 0 .. CHUNKSIZE {
			for z in 0 .. CHUNKSIZE {
				let mut push_blk = |color :[f32; 4]| {
						let pos = offs + Vector3::new(x, y, z);
						let fpos = [pos.x as f32, pos.y as f32, pos.z as f32];
						let colorh = [color[0]/2.0, color[1]/2.0, color[2]/2.0, color[3]];
						let mut any_non_blocked = false;
						push_block(&mut r, fpos, color, colorh, 1.0, |[xo, yo, zo]| {
							let pos = Vector3::new(x + xo, y + yo, z + zo);
							let outside = pos.map(|v| v < 0 || v >= CHUNKSIZE);
							if outside.x || outside.y || outside.z {
								any_non_blocked = true;
								return false;
							}
							match *chunk.get_blk(pos) {
								MapBlock::Air => {
									any_non_blocked = true;
									false
								},
								_ => true,
							}
						});
						// If any of the faces is unblocked, this block
						// will be reported
						if any_non_blocked {
							f(pos);
						}
				};
				match *chunk.get_blk(Vector3::new(x, y, z)) {
					MapBlock::Air => (),
					MapBlock::Ground => {
						push_blk([0.0, 1.0, 0.0, 1.0]);
					},
					MapBlock::Water => {
						push_blk([0.0, 0.0, 1.0, 1.0]);
					},
					MapBlock::Wood => {
						push_blk([0.5, 0.25, 0.0, 1.0]);
					},
					MapBlock::Stone => {
						push_blk([0.5, 0.5, 0.5, 1.0]);
					},
					MapBlock::Tree => {
						push_blk([0.38, 0.25, 0.125, 1.0]);
					},
					MapBlock::Leaves => {
						push_blk([0.0, 0.4, 0.0, 1.0]);
					},
				}
			}
		}
	}
	r
}

pub fn mesh_compound_for_chunk(offs :Vector3<isize>,
		chunk :&MapChunkData) -> (Vec<Vertex>, Option<Compound<f32>>) {
	let mut shapes = Vec::new();
	let mesh = mesh_for_chunk(offs, &chunk, |p :Vector3<isize>| {
		let iso = Isometry3::new(p.map(|v| v as f32).into(), nalgebra::zero());
		let cuboid = ShapeHandle::new(Cuboid::new(Vector3::new(0.5, 0.5, 0.5)));
		shapes.push((iso, cuboid));
	});
	let compound = if shapes.len() > 0 {
		Some(Compound::new(shapes))
	} else {
		None
	};
	(mesh, compound)
}
