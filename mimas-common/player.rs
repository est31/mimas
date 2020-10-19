#[derive(Serialize, Deserialize, Hash, Clone, Copy, PartialEq, Eq)]
pub enum PlayerMode {
	/// The player has fly mode enabled
	Fly,
	/// The player has noclip mode enabled
	Noclip,
	/// The player has fast mode enabled
	Fast,
}
