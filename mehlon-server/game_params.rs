use crafting::Recipe;
use std::sync::Arc;
use toml::{from_str, value::Table};
use std::fs::read_to_string;
use map::MapBlock;
use super::StrErr;

pub type GameParamsHdl = Arc<GameParams>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameParams {
	pub recipes :Vec<Recipe>,
}

impl GameParams {
	pub fn load() -> GameParamsHdl {
		load_params_failible().expect("Couldn't load game params")
	}
}

fn from_tbl(tbl :Table) -> Result<GameParams, StrErr> {

	let recipes_list = tbl.get("recipe")
		.ok_or("No recipe list found")?
		.as_array().ok_or("Expected table for recipe")?;
	let recipes = recipes_list.iter()
		.map(|recipe| {
			let inputs = recipe.get("inputs")
				.ok_or("inputs not found")?
				.as_array().ok_or("map expected")?;
			let inputs = inputs.iter()
				.map(|input| {
					let name = input.as_str().ok_or("expected str")?;
					if name == "" {
						Ok(None)
					} else {
						let mb = MapBlock::from_str(name).ok_or("invalid name")?;
						Ok(Some(mb))
					}
				})
				.collect::<Result<Vec<Option<MapBlock>>, StrErr>>()?;
			let output_itm = recipe.get("output-itm")
				.ok_or("output item not found")?
				.as_str().ok_or("expected str")?;
			let output_itm = MapBlock::from_str(output_itm)
				.ok_or("invalid name")?;
			let output_qty = recipe.get("output-qty")
				.ok_or("output qty not found")?
				.as_integer().ok_or("expected int")?;

			Ok(Recipe {
				inputs,
				output : (output_itm, output_qty as u16),
			})
		})
		.collect::<Result<Vec<Recipe>, StrErr>>()?;

	Ok(GameParams {
		recipes,
	})
}

pub fn load_params_failible() -> Result<GameParamsHdl, StrErr> {
	let file_str = read_to_string("game-params.toml")
		.unwrap_or_else(|err| {
			println!("Using default game params because of error: {}", err);
			DEFAULT_GAME_PARAMS_STR.to_owned()
		});

	let tbl = from_str(&file_str)?;
	let res = from_tbl(tbl)?;
	Ok(Arc::new(res))
}

static DEFAULT_GAME_PARAMS_STR :&str = include_str!("game-params.toml");
