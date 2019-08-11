use crafting::Recipe;
use map::MapBlock;
use std::sync::Arc;

pub type GameParamsHdl = Arc<GameParams>;

pub struct GameParams {
	pub recipes :Vec<Recipe>,
}

impl GameParams {
	pub fn load() -> Self {
		GameParams {
			recipes : vec![
				Recipe {
					inputs : &[
						Some(MapBlock::Tree),
					],
					output : (MapBlock::Wood, 4),
				},
			],
		}
	}
}
