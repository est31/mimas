use anyhow::Result;
use crate::map::MapChunkData;
use nalgebra::Vector3;
use std::str;
use toml::from_str;
use std::num::NonZeroU64;
use crate::game_params::{NameIdMap};

pub struct NullStorageBackend;

impl StorageBackend for NullStorageBackend {
	fn store_chunk(&mut self, _pos :Vector3<isize>,
			_data :&MapChunkData) -> Result<()> {
		Ok(())
	}
	fn tick(&mut self) -> Result<()> {
		Ok(())
	}
	fn load_chunk(&mut self, _pos :Vector3<isize>, _m :&NameIdMap) -> Result<Option<MapChunkData>> {
		Ok(None)
	}
	fn get_global_kv(&mut self, _key :&str) -> Result<Option<Vec<u8>>> {
		Ok(None)
	}
	fn set_global_kv(&mut self, _key :&str, _content :&[u8]) -> Result<()> {
		Ok(())
	}
	fn get_player_kv(&mut self, _id_pair :PlayerIdPair, _key :&str) -> Result<Option<Vec<u8>>> {
		Ok(None)
	}
	fn set_player_kv(&mut self, _id_pair :PlayerIdPair, _key :&str, _content :&[u8]) -> Result<()> {
		Ok(())
	}
}

#[derive(PartialEq, Eq, Hash, Copy, Clone, Serialize, Deserialize)]
pub struct PlayerIdPair(NonZeroU64);

impl PlayerIdPair {
	pub fn singleplayer() -> Self {
		Self::from_components(0, 1)
	}
	pub fn from_components(id_src :u8, id :u64) -> Self {
		// Impose a limit on the id
		// as too large ids interfere
		// with the src component
		// in our local storage.
		// There is simply no need for
		// such high ids anyway so we
		// limit it to make things easier
		// for us.
		assert!(id < 1 << (64 - 17),
			"id of {} is too big", id);
		let v = ((id_src as u64) << (64 - 8)) | id;
		Self(NonZeroU64::new(v).unwrap())
	}
	pub fn id_src(&self) -> u8 {
		self.0.get().to_be_bytes()[0]
	}
	pub fn id_u64(&self) -> u64 {
		self.0.get() & ((1 << (64 - 8)) - 1)
	}
	pub fn id_i64(&self) -> i64 {
		self.id_u64() as i64
	}
}

#[cfg(test)]
#[test]
fn test_player_id_pair() {
	for i in 0 .. 32 {
		for j in 0 .. 32 {
			if (i, j) == (0, 0) {
				continue;
			}
			let id = PlayerIdPair::from_components(i, j);
			assert_eq!((id.id_src(), id.id_u64()), (i, j));
		}
	}
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct PlayerPosition {
	x :f32,
	y :f32,
	z :f32,
	pitch :f32,
	yaw :f32,
}

impl Default for PlayerPosition {
	fn default() -> Self {
		Self {
			x : 60.0,
			y : 40.0,
			z : 20.0,
			pitch : 45.0,
			yaw : 0.0,
		}
	}
}

impl PlayerPosition {
	pub fn from_pos(pos :Vector3<f32>) -> Self {
		Self::from_pos_pitch_yaw(pos, 45.0, 0.0)
	}
	pub fn from_pos_pitch_yaw(pos :Vector3<f32>, pitch :f32, yaw :f32) -> Self {
		Self {
			x : pos.x,
			y : pos.y,
			z : pos.z,
			pitch,
			yaw,
		}
	}
	pub fn pos(&self) -> Vector3<f32> {
		Vector3::new(self.x, self.y, self.z)
	}
	pub fn pitch(&self) -> f32 {
		self.pitch
	}
	pub fn yaw(&self) -> f32 {
		self.yaw
	}
	pub fn deserialize(buf :&[u8]) -> Result<Self> {
		let serialized_str = str::from_utf8(buf)?;
		let deserialized = from_str(serialized_str)?;
		Ok(deserialized)
	}
}

pub type DynStorageBackend = Box<dyn StorageBackend + Send>;

pub trait StorageBackend {
	fn store_chunk(&mut self, pos :Vector3<isize>,
			data :&MapChunkData) -> Result<()>;
	fn tick(&mut self) -> Result<()>;
	fn load_chunk(&mut self, pos :Vector3<isize>, m :&NameIdMap) -> Result<Option<MapChunkData>>;
	fn get_global_kv(&mut self, key :&str) -> Result<Option<Vec<u8>>>;
	fn set_global_kv(&mut self, key :&str, content :&[u8]) -> Result<()>;
	fn get_player_kv(&mut self, id_pair :PlayerIdPair, key :&str) -> Result<Option<Vec<u8>>>;
	fn set_player_kv(&mut self, id_pair :PlayerIdPair, key :&str, content :&[u8]) -> Result<()>;
}
