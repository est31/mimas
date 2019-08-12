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
pub struct GameParams {
	pub recipes :Vec<Recipe>,
	pub colors :HashMap<MapBlock, Option<[f32; 4]>>,
}

impl GameParams {
	pub fn load() -> GameParamsHdl {
		load_params_failible().expect("Couldn't load game params")
	}
	pub fn get_color_for_blk(&self, blk :&MapBlock) -> Option<[f32; 4]> {
		self.colors.get(blk).cloned().unwrap_or(None)
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

	let colors = val.read::<Array>("block")?
		.iter()
		.map(|block| {
			let name = block.read::<str>("name")?;
			let id = MapBlock::from_str(name)
				.ok_or("invalid name")?;
			let color = block.read::<Value>("color")?
				.clone()
				.try_into()?;
			Ok((id, color))
		})
		.collect::<Result<HashMap<_, _>, StrErr>>()?;

	Ok(GameParams {
		recipes,
		colors,
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
