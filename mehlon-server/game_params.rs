use crafting::Recipe;
use std::sync::Arc;
use toml::from_str;
use toml::value::{Value, Array};
use std::fs::read_to_string;
use map::MapBlock;
use super::StrErr;
use toml_util::TomlReadExt;
use std::collections::HashMap;

pub type GameParamsHdl = Arc<GameParams>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockParams {
	pub color :Option<[f32; 4]>,
	pub pointable :bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockRoles {
	pub air :MapBlock,
	pub water :MapBlock,
	pub sand :MapBlock,
	pub ground :MapBlock,
	pub wood :MapBlock,
	pub stone :MapBlock,
	pub leaves :MapBlock,
	pub tree :MapBlock,
	pub cactus :MapBlock,
	pub coal :MapBlock,
	pub iron_ore :MapBlock,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameParams {
	pub recipes :Vec<Recipe>,
	pub block_params :HashMap<MapBlock, BlockParams>,
	pub block_roles :BlockRoles,
}

impl BlockRoles {
	pub fn new() -> Self {
		use MapBlock::*;
		Self {
			air : Air,
			water : Water,
			sand : Sand,
			ground : Ground,
			wood : Wood,
			stone : Stone,
			leaves : Leaves,
			tree : Tree,
			cactus : Cactus,
			coal : Coal,
			iron_ore : IronOre,
		}
	}
}

impl GameParams {
	pub fn load() -> GameParamsHdl {
		load_params_failible().expect("Couldn't load game params")
	}
	pub fn get_color_for_blk(&self, blk :&MapBlock) -> Option<[f32; 4]> {
		self.block_params.get(blk)
			.map(|p| p.color)
			.unwrap_or(Some([0.0, 0.0, 0.0, 1.0]))
	}
	pub fn get_pointability_for_blk(&self, blk :&MapBlock) -> bool {
		self.block_params.get(blk)
			.map(|p| p.pointable)
			.unwrap_or(true)
	}
}

fn from_val(val :Value) -> Result<GameParams, StrErr> {

	let recipes_list = val.read::<Array>("recipe")?;
	let recipes = recipes_list.iter()
		.map(|recipe| {
			let inputs = recipe.read::<Array>("inputs")?;
			let inputs = inputs.iter()
				.map(|input| {
					let name = input.convert::<str>()?;
					if name == "" {
						Ok(None)
					} else {
						let mb = MapBlock::from_str(name).ok_or("invalid name")?;
						Ok(Some(mb))
					}
				})
				.collect::<Result<Vec<Option<MapBlock>>, StrErr>>()?;
			let output_itm = recipe.read::<str>("output-itm")?;
			let output_itm = MapBlock::from_str(output_itm)
				.ok_or("invalid name")?;
			let output_qty = *recipe.read::<i64>("output-qty")?;

			Ok(Recipe {
				inputs,
				output : (output_itm, output_qty as u16),
			})
		})
		.collect::<Result<Vec<Recipe>, StrErr>>()?;

	let block_params = val.read::<Array>("block")?
		.iter()
		.map(|block| {
			let name = block.read::<str>("name")?;
			let id = MapBlock::from_str(name)
				.ok_or("invalid name")?;
			let color = block.read::<Value>("color")?
				.clone();
			let color = if color == Value::Boolean(false) {
				None
			} else {
				Some(color.try_into()?)
			};
			let pointable = block.get("pointable")
				.unwrap_or(&Value::Boolean(true));
			let pointable = *pointable.convert::<bool>()?;
			let block_params = BlockParams {
				color,
				pointable,
			};
			Ok((id, block_params))
		})
		.collect::<Result<HashMap<_, _>, StrErr>>()?;

	Ok(GameParams {
		recipes,
		block_params,
		block_roles : BlockRoles::new(),
	})
}

pub fn load_params_failible() -> Result<GameParamsHdl, StrErr> {
	let file_str = read_to_string("game-params.toml")
		.unwrap_or_else(|err| {
			println!("Using default game params because of error: {}", err);
			DEFAULT_GAME_PARAMS_STR.to_owned()
		});

	let val = from_str(&file_str)?;
	let res = from_val(val)?;
	Ok(Arc::new(res))
}

static DEFAULT_GAME_PARAMS_STR :&str = include_str!("game-params.toml");

#[cfg(test)]
#[test]
fn default_game_params_parse_test() {
	let file_str = DEFAULT_GAME_PARAMS_STR;
	let val = from_str(&file_str).unwrap();
	let _res = from_val(val).unwrap();
}
