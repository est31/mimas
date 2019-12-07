use nalgebra::Vector3;
use std::collections::hash_map::{HashMap, Entry};
use crate::{btchn, btpic};
use crate::map_storage::PlayerIdPair;
use crate::game_params::ServerGameParamsHdl;
use crate::inventory::SelectableInventory;

use super::mapgen::{Schematic, MapgenThread};

pub const CHUNKSIZE :isize = 16;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct MapBlock(u8);

pub mod map_block {
	use super::MapBlock;

	pub const AIR :MapBlock = MapBlock(0);
}

impl Default for MapBlock {
	fn default() -> Self {
		map_block::AIR
	}
}
impl MapBlock {
	pub fn id(self) -> u8 {
		self.0
	}
	pub(super) fn from_id_unchecked(id :u8) -> Self {
		MapBlock(id)
	}
}

#[derive(Serialize, Deserialize, Clone)]
pub enum MetadataEntry {
	Inventory(SelectableInventory),
	//Text(String), // TODO
	//Orientation, // TODO
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MapChunkMetadata {
	pub metadata :HashMap<Vector3<u8>, MetadataEntry>,
}

impl MapChunkMetadata {
	pub fn empty() -> Self {
		Self {
			metadata : HashMap::new(),
		}
	}
}

big_array! { BigArray; }

#[derive(Serialize, Deserialize, Clone)]
pub struct MapChunkData(
	#[serde(with = "BigArray")]
	pub(in super) [MapBlock; (CHUNKSIZE * CHUNKSIZE * CHUNKSIZE) as usize],
	pub MapChunkMetadata,
);

pub struct Map<B :MapBackend> {
	backend :B,
	chunks :HashMap<Vector3<isize>, MapChunkData>,
	on_change :Box<dyn Fn(Vector3<isize>, &MapChunkData)>,
}

pub type ServerMap = Map<MapgenThread>;
pub type ClientMap = Map<ClientBackend>;

impl MapChunkData {
	pub fn uninitialized() -> Self {
		Self::filled_with(MapBlock::default())
	}
	pub fn filled_with(m :MapBlock) -> Self {
		Self([m; (CHUNKSIZE * CHUNKSIZE * CHUNKSIZE) as usize],
			MapChunkMetadata::empty())
	}
	pub fn get_blk_mut(&mut self, pos :Vector3<isize>) -> &mut MapBlock {
		let (x, y, z) = (pos.x, pos.y, pos.z);
		&mut self.0[(x * CHUNKSIZE * CHUNKSIZE + y * CHUNKSIZE + z) as usize]
	}
	pub fn get_blk(&self, pos :Vector3<isize>) -> &MapBlock {
		let (x, y, z) = (pos.x, pos.y, pos.z);
		&self.0[(x * CHUNKSIZE * CHUNKSIZE + y * CHUNKSIZE + z) as usize]
	}
	pub fn get_blk_meta_entry(&mut self, pos :Vector3<isize>) -> Entry<'_, Vector3<u8>, MetadataEntry> {
		self.1.metadata.entry(pos.map(|v| v as u8))
	}
	pub fn get_blk_meta(&self, pos :Vector3<isize>) -> Option<&MetadataEntry> {
		self.1.metadata.get(&pos.map(|v| v as u8))
	}
}

fn spawn_schematic<B :MapBackend>(map :&mut Map<B>, pos :Vector3<isize>, schematic :&Schematic) {
	for (bpos, mb) in schematic.items.iter() {
		let blk = map.get_blk_mut_no_upd(pos + bpos).unwrap();
		*blk = *mb;
	}
	let pos_min = btchn(pos + schematic.aabb_min);
	let pos_max = btchn(pos + schematic.aabb_max);
	for x in pos_min.x ..= pos_max.x {
		for y in pos_min.y ..= pos_max.y {
			for z in pos_min.z ..= pos_max.z {
				let p = Vector3::new(x, y, z);
				if let Some(chn) = map.get_chunk(p).cloned() {
					(map.on_change)(p, &chn);
					map.backend.chunk_changed(p, chn);
				}
			}
		}
	}
}

pub fn spawn_tree<B :MapBackend>(map :&mut Map<B>, pos :Vector3<isize>,
		params :&ServerGameParamsHdl) {
	spawn_schematic(map, pos, &params.p.schematics.tree_schematic);
}

pub struct MapBlockHandle<'a, B :MapBackend> {
	pos :Vector3<isize>,
	chk :&'a mut MapChunkData,
	backend :&'a mut B,
	on_change :&'a Box<dyn Fn(Vector3<isize>, &MapChunkData)>,
}

impl<'a, B :MapBackend> MapBlockHandle<'a, B> {
	pub fn set(&mut self, b :MapBlock) {
		let chunk_pos = btchn(self.pos);
		let pos_in_chunk = btpic(self.pos);
		*self.chk.get_blk_mut(pos_in_chunk) = b;
		self.backend.chunk_changed(chunk_pos, self.chk.clone());
		(*self.on_change)(chunk_pos, &self.chk);
	}
	pub fn fake_change(&mut self) {
		let chunk_pos = btchn(self.pos);
		self.backend.chunk_changed(chunk_pos, self.chk.clone());
		(*self.on_change)(chunk_pos, &self.chk);
	}
	pub fn get(&mut self) -> MapBlock {
		let pos_in_chunk = btpic(self.pos);
		*self.chk.get_blk(pos_in_chunk)
	}
}

pub struct MetadataHandle<'a, B :MapBackend> {
	pos :Vector3<isize>,
	chk :&'a mut MapChunkData,
	backend :&'a mut B,
	on_change :&'a Box<dyn Fn(Vector3<isize>, &MapChunkData)>,
}

impl<'a, B :MapBackend> MetadataHandle<'a, B> {
	pub fn set(&mut self, b :MetadataEntry) {
		let chunk_pos = btchn(self.pos);
		let pos_in_chunk = btpic(self.pos);
		match self.chk.get_blk_meta_entry(pos_in_chunk) {
			Entry::Occupied(mut e) => {
				e.insert(b);
			},
			Entry::Vacant(e) => {
				e.insert(b);
			},
		}
		self.backend.chunk_changed(chunk_pos, self.chk.clone());
		(*self.on_change)(chunk_pos, &self.chk);
	}
	pub fn get(&mut self) -> Option<&MetadataEntry> {
		let pos_in_chunk = btpic(self.pos);
		self.chk.get_blk_meta(pos_in_chunk)
	}
	pub fn clear(&mut self) {
		let chunk_pos = btchn(self.pos);
		let pos_in_chunk = btpic(self.pos);
		match self.chk.get_blk_meta_entry(pos_in_chunk) {
			Entry::Occupied(e) => {
				e.remove_entry();
			},
			Entry::Vacant(_e) => (),
		}
		self.backend.chunk_changed(chunk_pos, self.chk.clone());
		(*self.on_change)(chunk_pos, &self.chk);
	}
}

pub struct ClientBackend;

impl MapBackend for ClientBackend {
	fn gen_chunks_in_area(&mut self, _pos_min :Vector3<isize>,
			_pos_max :Vector3<isize>) {
		// Do nothing. The server just pushes any chunks.
	}
	fn run_for_generated_chunks<F :FnMut(Vector3<isize>, &MapChunkData)>(&mut self,
			_f :&mut F) {
		// Do nothing. The server just pushes any chunks.
	}
	fn chunk_changed(&mut self, _pos :Vector3<isize>, _data :MapChunkData) {
		// Do nothing. The server just pushes any chunks.
	}
	fn set_player_kv(&mut self, _id :PlayerIdPair, _key :&str, _value :Vec<u8>) {
		// Do nothing. There is no storage on the client.
	}
	fn get_player_kv(&mut self, _id: PlayerIdPair, _key :&str, _data :u32) {
		// Do nothing. There is no storage on the client.
	}
	fn run_for_kv_results<F :FnMut(PlayerIdPair, u32, String, Option<Vec<u8>>)>(
			&mut self, _f :&mut F) {
		// Do nothing. There is no storage on the client.
	}
}

pub trait MapBackend {
	fn gen_chunks_in_area(&mut self, pos_min :Vector3<isize>,
			pos_max :Vector3<isize>);
	fn run_for_generated_chunks<F :FnMut(Vector3<isize>, &MapChunkData)>(&mut self,
			f :&mut F);
	fn chunk_changed(&mut self, pos :Vector3<isize>, data :MapChunkData);
	fn set_player_kv(&mut self, id :PlayerIdPair, key :&str, value :Vec<u8>);
	fn get_player_kv(&mut self, id: PlayerIdPair, key :&str, data :u32);
	fn run_for_kv_results<F :FnMut(PlayerIdPair, u32, String, Option<Vec<u8>>)>(
		&mut self, f :&mut F);
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
		self.chunks.insert(pos, data.clone());
		self.backend.chunk_changed(pos, data.clone());
		(self.on_change)(pos, &data);
	}
	pub fn gen_chunks_in_area(&mut self, pos_min :Vector3<isize>,
			pos_max :Vector3<isize>) {
		self.backend.gen_chunks_in_area(pos_min, pos_max,);
	}
	pub fn tick(&mut self) {
		let on_change = &self.on_change;
		let chunks = &mut self.chunks;
		self.backend.run_for_generated_chunks(&mut |pos, chn :&MapChunkData| {
			chunks.insert(pos, chn.clone());
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
	pub fn get_blk_mut<'s>(&'s mut self, pos :Vector3<isize>) -> Option<MapBlockHandle<'s, B>> {
		let chunk_pos = btchn(pos);
		let on_change = &self.on_change;
		let backend = &mut self.backend;
		self.chunks.get_mut(&chunk_pos)
			.map(move |chk| MapBlockHandle {
				pos,
				chk,
				backend,
				on_change,
			})
	}
	pub fn get_blk_meta_mut(&mut self, pos :Vector3<isize>) -> Option<MetadataHandle<'_, B>> {
		let chunk_pos = btchn(pos);
		let on_change = &self.on_change;
		let backend = &mut self.backend;
		self.chunks.get_mut(&chunk_pos)
			.map(move |chk| MetadataHandle {
				pos,
				chk,
				backend,
				on_change,
			})
	}
	pub fn get_blk_meta(&self, pos :Vector3<isize>) -> Option<Option<&MetadataEntry>> {
		let chunk_pos = btchn(pos);
		let pos_in_chunk = btpic(pos);
		self.get_chunk(chunk_pos)
			.map(|blk| blk.get_blk_meta(pos_in_chunk))
	}
	pub fn set_player_kv(&mut self, id :PlayerIdPair, key :&str, value :Vec<u8>) {
		self.backend.set_player_kv(id, key, value);
	}
	pub fn get_player_kv(&mut self, id: PlayerIdPair, key :&str, payload :u32) {
		self.backend.get_player_kv(id, key, payload);
	}
	pub fn run_for_kv_results<F :FnMut(PlayerIdPair, u32, String, Option<Vec<u8>>)>(&mut self, f :&mut F) {
		self.backend.run_for_kv_results(f);
	}
}
