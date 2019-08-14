use crafting::Recipe;
use std::sync::Arc;
use toml::from_str;
use toml::value::{Value, Array};
use std::fs::read_to_string;
use map::MapBlock;
use super::StrErr;
use toml_util::TomlReadExt;
use std::collections::HashMap;
use mapgen::{Schematic, self};

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
pub struct Schematics {
	pub tree_schematic :Schematic,
	pub cactus_schematic :Schematic,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NameIdMap {
	first_invalid_id :u8,
	name_to_id :HashMap<String, MapBlock>,
	id_to_name :Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameParams {
	pub recipes :Vec<Recipe>,
	pub block_params :HashMap<MapBlock, BlockParams>,
	pub block_roles :BlockRoles,
	pub schematics :Schematics,
	pub name_id_map :NameIdMap,
}

impl BlockRoles {
	pub fn new() -> Self {
		use map::map_block::*;
		Self {
			air : AIR,
			water : WATER,
			sand : SAND,
			ground : GROUND,
			wood : WOOD,
			stone : STONE,
			leaves : LEAVES,
			tree : TREE,
			cactus : CACTUS,
			coal : COAL,
			iron_ore : IRON_ORE,
		}
	}
}

impl Schematics {
	pub fn new(roles :&BlockRoles) -> Self {
		Self {
			tree_schematic : mapgen::tree_schematic(roles),
			cactus_schematic : mapgen::cactus_schematic(roles),
		}
	}
}

impl NameIdMap {
	pub fn builtin_name_list() -> Self {
		Self::from_name_list(vec![
			"Air",
			"Water",
			"Sand",
			"Ground",
			"Wood",
			"Stone",
			"Leaves",
			"Tree",
			"Cactus",
			"Coal",
			"IronOre",
		])
	}
	pub fn from_name_list(names :Vec<impl Into<String>>) -> Self {
		let mut name_to_id = HashMap::new();
		let mut id_to_name = Vec::with_capacity(names.len());
		let mut id = 0;
		for name in names.into_iter() {
			let name = name.into();
			id_to_name.push(name.clone());
			let mb = MapBlock::from_id_unchecked(id);
			name_to_id.insert(name.clone(), mb);
			id += 1;
		}
		Self {
			first_invalid_id : id,
			name_to_id,
			id_to_name,
		}
	}
	pub fn names(&self) -> &[String] {
		&self.id_to_name
	}
	pub fn extend_existing(mut other :Self,
			names :Vec<impl Into<String>>) -> Self {
		let mut id = other.first_invalid_id;

		for name in names.into_iter() {
			let name = name.into();
			other.id_to_name.push(name.clone());
			let mb = MapBlock::from_id_unchecked(id);
			other.name_to_id.insert(name.clone(), mb);
			id += 1;
		}
		other.first_invalid_id = id;
		other
	}
	pub fn mb_from_id(&self, id :u8) -> Option<MapBlock> {
		if id >= self.first_invalid_id {
			return None;
		}
		Some(MapBlock::from_id_unchecked(id))
	}
	pub fn get_name(&self, mb :MapBlock) -> Option<&str> {
		self.id_to_name.get(mb.id() as usize)
			.map(|v| {
				let v :&str = &*v;
				v
			})
	}
	pub fn get_id<'a>(&self, s :impl Into<&'a str>) -> Option<MapBlock> {
		self.name_to_id.get(s.into())
			.map(|v| *v)
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

	let name_id_map = NameIdMap::builtin_name_list();

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
						let mb = name_id_map.get_id(name).ok_or("invalid name")?;
						Ok(Some(mb))
					}
				})
				.collect::<Result<Vec<Option<MapBlock>>, StrErr>>()?;
			let output_itm = recipe.read::<str>("output-itm")?;
			let output_itm = name_id_map.get_id(output_itm)
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
			let id = name_id_map.get_id(name)
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

	let block_roles = BlockRoles::new();
	let schematics = Schematics::new(&block_roles);
	Ok(GameParams {
		recipes,
		block_params,
		schematics,
		block_roles,
		name_id_map,
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
