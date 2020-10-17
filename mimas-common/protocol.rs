use crate::map::{MapChunkData, MapBlock};
use crate::map_storage::{PlayerIdPair, PlayerPosition};
use crate::inventory::{SelectableInventory, InventoryPos};
use crate::local_auth::{PlayerPwHash, HashParams};
use crate::game_params::GameParams;
use nalgebra::Vector3;

#[derive(Serialize, Deserialize)]
pub enum ClientToServerMsg {
	LogIn(String, Vec<u8>),
	SendHash(PlayerPwHash), // "Auth" for new users
	SendM1(Vec<u8>), // Auth for existing users
	GetHashedBlobs(Vec<Vec<u8>>),

	/// Params: Position, current inventory selection location, mapblock to place
	///
	/// The redundancy of specifying both inventory selection location
	/// and mapblock to place allows the server to recognize cases
	/// where client and server have desynced, and prevents mistakingly
	/// placing a wrong block.
	PlaceBlock(Vector3<isize>, usize, MapBlock),
	/// Params: Position, current inventory selection location, mapblock to place
	PlaceTree(Vector3<isize>, usize, MapBlock),
	Dig(Vector3<isize>),

	SetPos(PlayerPosition),
	InventorySwap(InventoryPos, InventoryPos, bool),
	Craft,
	InventorySelect(Option<usize>),
	Chat(String),
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ServerToClientMsg {
	HashEnrollment,
	HashParamsBpub(HashParams, Vec<u8>),
	LogInFail(String),
	GameParams(GameParams),
	HashedBlobs(Vec<(Vec<u8>, Vec<u8>)>),

	PlayerPositions(PlayerIdPair, Vec<(PlayerIdPair, PlayerPosition)>),

	SetPos(PlayerPosition),
	SetInventory(SelectableInventory),
	SetCraftInventory(SelectableInventory),
	ChunkUpdated(Vector3<isize>, MapChunkData),
	Chat(String),
}
