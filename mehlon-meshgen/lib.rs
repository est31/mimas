#![forbid(unsafe_code)]

extern crate nalgebra;
#[macro_use]
extern crate glium;
extern crate mehlon_server;

use mehlon_server::map::{MapChunkData,
	CHUNKSIZE};
use mehlon_server::game_params::{GameParamsHdl, DrawStyle};
use mehlon_server::map::MapBlock;
use nalgebra::Vector3;

#[repr(transparent)]
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct TextureId(pub u16);

#[derive(Copy, Clone)]
pub struct Vertex {
	pub tex_ind :u16,
	pub tex_pos :[f32; 2],
	pub position :[f32; 3],
	pub normal :[f32; 3],
}

implement_vertex!(Vertex, tex_ind, tex_pos, position, normal);

#[derive(Copy, Clone)]
pub enum MeshDrawStyle {
	Blocky(BlockTextureIds),
	Crossed(TextureId),
}

impl MeshDrawStyle {
	fn blocky(&self) -> Option<BlockTextureIds> {
		if let MeshDrawStyle::Blocky(bti) = self {
			Some(*bti)
		} else {
			None
		}
	}
}

#[derive(Copy, Clone)]
pub struct BlockTextureIds {
	pub id_sides :TextureId,
	pub id_top :TextureId,
	pub id_bottom :TextureId,
}

impl BlockTextureIds {
	pub fn uniform(id :TextureId) -> Self {
		Self {
			id_sides : id,
			id_top : id,
			id_bottom : id,
		}
	}
	pub fn new_tb(id_top_bottom :TextureId, id_sides :TextureId) -> Self {
		Self {
			id_sides,
			id_top : id_top_bottom,
			id_bottom : id_top_bottom,
		}
	}
	pub fn new(id_sides :TextureId, id_top :TextureId, id_bottom :TextureId, ) -> Self {
		Self {
			id_sides, id_top, id_bottom,
		}
	}
}

#[derive(Clone)]
pub struct TextureIdCache {
	fallback_id :BlockTextureIds,
	block_texture_ids :Vec<Option<BlockTextureIds>>,
	mesh_draw_styles :Vec<Option<MeshDrawStyle>>,
}

impl TextureIdCache {
	pub fn from_hdl(hdl :&GameParamsHdl,
			mut style_to_id :impl FnMut(&DrawStyle) -> MeshDrawStyle) -> Self {
		let fallback_id = style_to_id(&DrawStyle::Colored([0.0, 0.0, 0.0, 1.0])).blocky().unwrap();
		let mesh_draw_styles = hdl.block_params.iter()
			.map(|p| p.draw_style.as_ref().map(&mut style_to_id))
			.collect::<Vec<_>>();
		let block_texture_ids = mesh_draw_styles.iter()
			.map(|v| v.and_then(|v| v.blocky()))
			.collect::<Vec<_>>();
		Self {
			fallback_id,
			block_texture_ids,
			mesh_draw_styles,
		}
	}
	pub fn get_bl_tex_ids(&self, bl :&MapBlock) -> Option<BlockTextureIds> {
		self.block_texture_ids.get(bl.id() as usize)
			.map(|v| *v)
			.unwrap_or(Some(self.fallback_id))
	}
	pub fn get_mesh_draw_style(&self, bl :&MapBlock) -> Option<MeshDrawStyle> {
		self.mesh_draw_styles.get(bl.id() as usize)
			.and_then(|v| *v)
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
	($r:expr, ($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $tex_ind:expr) => {
		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [0.0, 0.0], position: [$x, $y, $z], normal : sign![$xsd, $ysd + $yd, $zd] });
		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [$xsd + $ysd, 0.0], position: [$x + $xsd, $y + $ysd, $z], normal : sign![$xsd, $ysd + $yd, $zd] });
		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [0.0, $yd + $zd], position: [$x, $y + $yd, $z + $zd], normal : sign![$xsd, $ysd + $yd, $zd] });

		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [$xsd + $ysd, 0.0], position: [$x + $xsd, $y + $ysd, $z], normal : sign![$xsd, $ysd + $yd, $zd] });
		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [$xsd + $ysd, $yd + $zd], position: [$x + $xsd, $y + $yd + $ysd, $z + $zd], normal : sign![$xsd, $ysd + $yd, $zd] });
		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [0.0, $yd + $zd], position: [$x, $y + $yd, $z + $zd], normal : sign![$xsd, $ysd + $yd, $zd] });
	}
}
macro_rules! rpush_face_rev {
	($r:expr, ($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $tex_ind:expr) => {
		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [0.0, $yd + $zd], position: [$x, $y + $yd, $z + $zd], normal : sign![-$xsd, -$ysd - $yd, -$zd] });
		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [$xsd + $ysd, 0.0], position: [$x + $xsd, $y + $ysd, $z], normal : sign![-$xsd, -$ysd - $yd, -$zd] });
		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [0.0, 0.0], position: [$x, $y, $z], normal : sign![-$xsd, -$ysd - $yd, -$zd] });

		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [0.0, $yd + $zd], position: [$x, $y + $yd, $z + $zd], normal : sign![-$xsd, -$ysd - $yd, -$zd] });
		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [$xsd + $ysd, $yd + $zd], position: [$x + $xsd, $y + $yd + $ysd, $z + $zd], normal : sign![-$xsd, -$ysd - $yd, -$zd] });
		$r.push(Vertex { tex_ind : $tex_ind, tex_pos : [$xsd + $ysd, 0.0], position: [$x + $xsd, $y + $ysd, $z], normal : sign![-$xsd, -$ysd - $yd, -$zd] });
	}
}

#[inline]
pub fn push_block<F :FnMut([isize; 3]) -> bool>(r :&mut Vec<Vertex>, [x, y, z] :[f32; 3], block_ids :BlockTextureIds, siz :f32, mut blocked :F) {
	macro_rules! push_face {
		(($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $tex_ind:expr) => {
			rpush_face!(r, ($x, $y, $z), ($xsd, $ysd, $yd, $zd), $tex_ind);
		};
	}
	macro_rules! push_face_rev {
		(($x:expr, $y:expr, $z:expr), ($xsd:expr, $ysd:expr, $yd:expr, $zd:expr), $tex_ind:expr) => {
			rpush_face_rev!(r, ($x, $y, $z), ($xsd, $ysd, $yd, $zd), $tex_ind);
		};
	}
	// X-Y face
	if !blocked([0, 0, -1]) {
		push_face!((x, y, z), (siz, 0.0, siz, 0.0), block_ids.id_bottom.0);
	}
	// X-Z face
	if !blocked([0, -1, 0]) {
		push_face_rev!((x, y, z), (siz, 0.0, 0.0, siz), block_ids.id_sides.0);
	}
	// Y-Z face
	if !blocked([-1, 0, 0]) {
		push_face!((x, y, z), (0.0, siz, 0.0, siz), block_ids.id_sides.0);
	}
	// X-Y face (z+1)
	if !blocked([0, 0, 1]) {
		push_face_rev!((x, y, z + siz), (siz, 0.0, siz, 0.0), block_ids.id_top.0);
	}
	// X-Z face (y+1)
	if !blocked([0, 1, 0]) {
		push_face!((x, y + siz, z), (siz, 0.0, 0.0, siz), block_ids.id_sides.0);
	}
	// Y-Z face (x+1)
	if !blocked([1, 0, 0]) {
		push_face_rev!((x + siz, y, z), (0.0, siz, 0.0, siz), block_ids.id_sides.0);
	}
}

#[inline]
pub fn colorh(col :[f32; 4]) -> [f32; 4] {
	[col[0]/2.0, col[1]/2.0, col[2]/2.0, col[3]]
}

pub fn mesh_for_chunk(offs :Vector3<isize>, chunk :&MapChunkData,
		cache :&TextureIdCache) -> Vec<Vertex> {
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
			cache :&TextureIdCache) -> bool {
		let pos = Vector3::new(pos.x + xo, pos.y + yo, pos.z + zo);
		let outside = pos.map(|v| v < 0 || v >= CHUNKSIZE);
		if outside.x || outside.y || outside.z {
			return false;
		}
		let blk = chunk.get_blk(pos);
		cache.get_bl_tex_ids(blk).is_some()
	};
	fn get_tex_ind(chunk: &MapChunkData, pos :Vector3<isize>,
			offsets :[isize; 3], cache :&TextureIdCache) -> Option<BlockTextureIds> {
		let blk = chunk.get_blk(pos);
		let texture_ids = cache.get_bl_tex_ids(blk);
		if texture_ids.is_some() && blocked(chunk, offsets, pos, cache) {
			None
		} else {
			texture_ids
		}
	};
	fn walk_for_all_blocks<G :FnMut(&mut Walker<TextureId>, Option<TextureId>, Vector3<isize>)>(
			f :fn(isize, isize, isize) -> Vector3<isize>,
			face :fn(BlockTextureIds) -> TextureId,
			offsets :[isize; 3],
			chunk :&MapChunkData, g :&mut G,
			cache :&TextureIdCache) {
		for c1 in 0 .. CHUNKSIZE {
			for c2 in 0 .. CHUNKSIZE {
				let mut walker = Walker::new();
				for cinner in 0 .. CHUNKSIZE {
					let rel_pos = f(c1, c2, cinner);
					let texture_ids = get_tex_ind(chunk, rel_pos, offsets, cache);
					let tex_ind = texture_ids.map(face);
					g(&mut walker, tex_ind, rel_pos)
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
		|bti| bti.id_bottom,
		[0, 0, -1],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.y as f32, color, |tx, last_y, ylen| {
				let (x, _y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				rpush_face!(r, (x, last_y, z), (siz, 0.0, ylen, 0.0), tx.0);
			});
		},
		cache
	);

	// X-Z face (unify over x)
	walk_for_all_blocks(
		|c1, c2, cinner| Vector3::new(cinner, c1, c2),
		|bti| bti.id_sides,
		[0, -1, 0],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.x as f32, color, |tx, last_x, xlen| {
				let (_x, y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				rpush_face_rev!(r, (last_x, y, z), (xlen, 0.0, 0.0, siz), tx.0);
			});
		},
		cache
	);

	// Y-Z face (unify over y)
	walk_for_all_blocks(
		|c1, c2, cinner| Vector3::new(c1, cinner, c2),
		|bti| bti.id_sides,
		[-1, 0, 0],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.y as f32, color, |tx, last_y, ylen| {
				let (x, _y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				rpush_face!(r, (x, last_y, z), (0.0, ylen, 0.0, siz), tx.0);
			});
		},
		cache
	);

	// X-Y face (z+1) (unify over y)
	walk_for_all_blocks(
		|c1, c2, cinner| Vector3::new(c1, cinner, c2),
		|bti| bti.id_top,
		[0, 0, 1],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.y as f32, color, |tx, last_y, ylen| {
				let (x, _y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				rpush_face_rev!(r, (x, last_y, z + siz), (siz, 0.0, ylen, 0.0), tx.0);
			});
		},
		cache
	);

	// X-Z face (y+1) (unify over x)
	walk_for_all_blocks(
		|c1, c2, cinner| Vector3::new(cinner, c1, c2),
		|bti| bti.id_sides,
		[0, 1, 0],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.x as f32, color, |tx, last_x, xlen| {
				let (_x, y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				rpush_face!(r, (last_x, y + siz, z), (xlen, 0.0, 0.0, siz), tx.0);
			});
		},
		cache
	);

	// Y-Z face (x+1) (unify over y)
	walk_for_all_blocks(
		|c1, c2, cinner| Vector3::new(c1, cinner, c2),
		|bti| bti.id_sides,
		[1, 0, 0],
		chunk,
		&mut |walker, color, rel_pos| {
			let pos = offs + rel_pos;
			walker.next(pos.y as f32, color, |tx, last_y, ylen| {
				let (x, _y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);
				rpush_face_rev!(r, (x + siz, last_y, z), (0.0, ylen, 0.0, siz), tx.0);
			});
		},
		cache
	);

	// Crossed nodes
	for x in 0 .. CHUNKSIZE {
		for y in 0 .. CHUNKSIZE {
			for z in 0 .. CHUNKSIZE {
				let rel_pos = Vector3::new(x, y, z);
				let blk = chunk.get_blk(rel_pos);
				let mds = cache.get_mesh_draw_style(blk);
				let tx = if let Some(MeshDrawStyle::Crossed(id)) = mds {
					id
				} else {
					continue;
				};
				let pos = offs + rel_pos;

				let (x, y, z) = (pos.x as f32, pos.y as f32, pos.z as f32);

				// rpush_face_rev!(r, (last_x, y, z), (xlen, 0.0, 0.0, siz), tx.0);
				// rpush_face!(r, (x, last_y, z), (0.0, ylen, 0.0, siz), tx.0);
				// rpush_face!(r, (last_x, y + siz, z), (xlen, 0.0, 0.0, siz), tx.0);
				// rpush_face_rev!(r, (x + siz, last_y, z), (0.0, ylen, 0.0, siz), tx.0);

				let xlen = siz;
				let ylen = siz;
				// X-Z
				rpush_face_rev!(r, (x, y + siz * 0.5, z), (xlen, 0.0, 0.0, siz), tx.0);
				// Y-Z
				rpush_face!(r, (x + siz * 0.5, y, z), (0.0, ylen, 0.0, siz), tx.0);
				// X-Z
				rpush_face!(r, (x, y + siz * 0.5, z), (xlen, 0.0, 0.0, siz), tx.0);
				// Y-Z
				rpush_face_rev!(r, (x + siz * 0.5, y, z), (0.0, ylen, 0.0, siz), tx.0);
			}
		}
	}

	r
}
