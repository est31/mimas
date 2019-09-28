use crafting::Recipe;
use std::sync::Arc;
use toml::from_str;
use toml::value::{Value, Array};
use std::fs::{read_to_string, File};
use std::path::Path;
use map::MapBlock;
use super::StrErr;
use toml_util::TomlReadExt;
use std::collections::HashMap;
use std::fmt::Display;
use std::num::NonZeroU16;
use std::str::FromStr;
use std::io::Read;
use inventory::Stack;
use mapgen::{Schematic, self};
use sha2::{Sha256, Digest};

pub type GameParamsHdl = Arc<GameParams>;
pub type ServerGameParamsHdl = Arc<ServerGameParams>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DrawStyle {
	Colored([f32; 4]),
	Texture(String),
	TextureSidesTop(String, String),
	TextureSidesTopBottom(String, String, String),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockParams {
	pub draw_style :Option<DrawStyle>,
	pub pointable :bool,
	pub display_name :String,
	pub drops :Stack,
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
	pub block_params :Vec<BlockParams>,
	pub block_roles :BlockRoles,
	pub schematics :Schematics,
	pub name_id_map :NameIdMap,
	pub texture_hashes :HashMap<String, Vec<u8>>,
}

pub struct ServerGameParams {
	pub p :GameParams,
	pub textures :HashMap<Vec<u8>, Vec<u8>>,
}

impl Default for DrawStyle {
	fn default() -> Self {
		DrawStyle::Colored([0.0, 0.0, 0.0, 1.0])
	}
}

impl Default for BlockParams {
	fn default() -> Self {
		Self {
			draw_style : Some(DrawStyle::default()),
			pointable : true,
			display_name : String::new(),
			drops : Stack::Empty,
		}
	}
}

impl BlockRoles {
	pub fn new(m :&NameIdMap) -> Result<Self, StrErr> {
		let get_id = |n| {
			m.get_id(n)
				.ok_or_else(|| format!("Coudln't find id for builtin role '{}'", n))
		};
		Ok(Self {
			air : get_id("default:air")?,
			water : get_id("default:water")?,
			sand : get_id("default:sand")?,
			ground : get_id("default:ground")?,
			wood : get_id("default:wood")?,
			stone : get_id("default:stone")?,
			leaves : get_id("default:leaves")?,
			tree : get_id("default:tree")?,
			cactus : get_id("default:cactus")?,
			coal : get_id("default:coal")?,
			iron_ore : get_id("default:iron_ore")?,
		})
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
			"default:air",
			"default:water",
			"default:sand",
			"default:ground",
			"default:wood",
			"default:stone",
			"default:leaves",
			"default:tree",
			"default:cactus",
			"default:coal",
			"default:iron_ore",
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
	fn get_or_extend(&mut self, name :impl Into<String>) -> MapBlock {
		let name = name.into();
		let id_to_name = &mut self.id_to_name;
		let first_invalid_id = &mut self.first_invalid_id;
		let mb = self.name_to_id.entry(name.clone())
			.or_insert_with(|| {
				id_to_name.push(name.clone());
				let mb = MapBlock::from_id_unchecked(*first_invalid_id);
				*first_invalid_id += 1;
				mb
			});
		*mb
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
	pub fn get_pointability_for_blk(&self, blk :&MapBlock) -> bool {
		self.block_params.get(blk.id() as usize)
			.map(|p| p.pointable)
			.unwrap_or(true)
	}
	pub fn block_display_name(&self, mb :MapBlock) -> impl Display {
		if let Some(bp) = self.block_params.get(mb.id() as usize) {
			bp.display_name.to_owned()
		} else {
			format!("{:?}", mb)
		}
	}
	pub fn search_block_name(&self, name :&str) -> Option<MapBlock> {
		if let Some(n) = self.name_id_map.get_id(name) {
			return Some(n);
		}
		// Fall back to search for matching display names
		// TODO use a hashmap instead of linear search
		self.block_params.iter().enumerate()
			.find(|(_id, p)| {
				p.display_name.eq_ignore_ascii_case(name)
			})
			.map(|(id, _p)| {
				MapBlock::from_id_unchecked(id as u8)
			})
	}
}

impl ServerGameParams {
	pub fn load(nm :NameIdMap) -> ServerGameParamsHdl {
		Arc::new(load_params_failible(nm).expect("Couldn't load game params"))
	}
}

/// Ensures that the modname::name format is used and
/// returns (modname, name) tuple if it is
pub(crate) fn parse_block_name(name :&str) -> Result<(&str, &str), StrErr> {
	fn check_chars(v :&str) -> bool {
		v.chars().all(|c| c.is_ascii_alphabetic() || c.is_ascii_digit() || c == '_')
	}
	let mut cit = name.split(':');
	if let (Some(mn), Some(n), None) = (cit.next(), cit.next(), cit.next()) {
		if !check_chars(mn) {
			Err(format!("Invalid mod name '{}'. Only alphanumeric chars and _ allowed.", mn))?;
		}
		if !check_chars(n) {
			Err(format!("Invalid name '{}'. Only alphanumeric chars and _ allowed.", n))?;
		}
		Ok((mn, n))
	} else {
		Err(format!("Invalid name '{}'. Must be in format modname:name.", name))?;
		unreachable!()
	}
}

pub fn resolve_stack_specifier(nm :&NameIdMap, sp :&str)
		-> Result<Stack, StrErr> {
	if sp.is_empty() {
		return Ok(Stack::Empty);
	}
	let mut nit = sp.split(' ');
	if let (Some(name), Some(count), None) = (nit.next(), nit.next(), nit.next()) {
		let item = nm.get_id(name)
			.ok_or_else(|| format!("Can't find any item named '{}'.", name))?;
		let count = u16::from_str(count)?;
		let count = NonZeroU16::new(count)
			.ok_or_else(|| format!("Count may not be 0. Use \"\" instead."))?;
		Ok(Stack::Content { item, count })
	} else {
		Err(format!("Invalid stack specifier '{}'. Must be in format 'modname:name count'.", sp))?;
		unreachable!()
	}
}

fn texture_hashes(asset_dir :impl AsRef<Path>,
		textures :Vec<String>) -> Result<Vec<(String, Vec<u8>, Vec<u8>)>, StrErr> {
	let asset_dir :&Path = asset_dir.as_ref();
	textures.iter()
		// TODO perform proper parsing and share it with the client
		.flat_map(|p| p.split("^"))
		.map(|tx| {
			let path = asset_dir.to_owned().join(&tx);
			let mut file = File::open(&path)
				.map_err(|e| format!("Error opening file at {}: {}", path.to_string_lossy(), e))?;
			let mut buf = Vec::new();
			file.read_to_end(&mut buf)?;
			let mut hasher = Sha256::new();
			let mut buf_rdr = buf.as_slice();
			std::io::copy(&mut buf_rdr, &mut hasher)?;
			let hash = hasher.result().as_slice().to_owned();
			Ok((tx.to_owned(), hash, buf))
		})
		.collect::<Result<Vec<_>, StrErr>>()
}

fn from_val(val :Value, nm_from_db :NameIdMap) -> Result<ServerGameParams, StrErr> {

	let override_default = val.get("override-default")
		.unwrap_or(&Value::Boolean(false));
	let mut params = if !*override_default.convert::<bool>()? {
		default_game_params(nm_from_db)?
	} else {
		let block_roles = BlockRoles::new(&nm_from_db)?;
		let schematics = Schematics::new(&block_roles);
		let p = GameParams {
			recipes : Vec::new(),
			block_params : Vec::new(),
			schematics,
			block_roles,
			name_id_map : nm_from_db,
			texture_hashes : HashMap::new(),
		};
		ServerGameParams {
			p,
			textures :HashMap::new(),
		}
	};
	let name_id_map = &mut params.p.name_id_map;

	// First pass: populate the name id map.
	// This allows us to refer to blocks other than our own
	// regardless of order.
	let blocks = val.read::<Array>("block")?;
	for block in blocks.iter() {
		let name = block.read::<str>("name")?;
		let _name_components = parse_block_name(name)?;
		let _id = name_id_map.get_or_extend(name);
	}

	params.p.block_params.resize_with(name_id_map.first_invalid_id as usize,
		Default::default);

	let mut textures = Vec::new();

	for block in blocks.iter() {
		let name = block.read::<str>("name")?;
		let name_components = parse_block_name(name)?;
		// unwrap is okay because we have added it in the first pass
		let id = name_id_map.get_id(name).unwrap();
		let texture = block.get("texture");
		let color = if let Some(color) = block.get("color") {
			if color == &Value::Boolean(false) {
				None
			} else {
				Some(color.clone().try_into()?)
			}
		} else {
			None
		};
		let draw_style = match (color, texture) {
			(Some(_), Some(_)) => Err("Both color and texture specified")?,
			(Some(col), None) => Some(DrawStyle::Colored(col)),
			(None, Some(Value::String(texture))) => {
				textures.push(texture.to_owned());
				Some(DrawStyle::Texture(texture.to_owned()))
			},
			(None, Some(Value::Array(arr))) if arr.len() == 2 => {
				let arr :[String; 2] = Value::Array(arr.clone()).try_into()?;
				textures.extend_from_slice(&arr);
				Some(DrawStyle::TextureSidesTop(arr[0].clone(), arr[1].clone()))
			},
			(None, Some(Value::Array(arr))) if arr.len() == 3 => {
				let arr :[String; 3] = Value::Array(arr.clone()).try_into()?;
				textures.extend_from_slice(&arr);
				Some(DrawStyle::TextureSidesTopBottom(arr[0].clone(), arr[1].clone(), arr[2].clone()))
			},
			(None, Some(Value::Array(arr))) => Err(format!("false number of textures: {}", arr.len()))?,
			(None, Some(_)) => Err("false type")?,
			(None, None) => None,
		};
		let pointable = block.get("pointable")
			.unwrap_or(&Value::Boolean(true));
		let pointable = *pointable.convert::<bool>()?;
		let display_name = if let Some(n) = block.get("display-name") {
			n.convert::<str>()?.to_owned()
		} else {
			name_components.1.to_owned()
		};
		let drops = if let Some(drops) = block.get("drops") {
			let drops_sp = drops.convert::<str>()?;
			resolve_stack_specifier(&name_id_map, drops_sp)?
		} else {
			Stack::with(id, 1)
		};
		let block_params = BlockParams {
			draw_style,
			pointable,
			display_name,
			drops,
		};
		params.p.block_params[id.id() as usize] = block_params;
	}

	let texture_h = &mut params.p.texture_hashes;
	let texture_bl = &mut params.textures;
	texture_hashes(".", textures)?
		.into_iter()
		.for_each(|(name, hash, blob)| {
			texture_h.insert(name, hash.clone());
			texture_bl.insert(hash, blob);
		});

	let recipes_list = val.read::<Array>("recipe")?;
	for recipe in recipes_list.iter() {
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
		let output_sp = recipe.read::<str>("output")?;
		let output = resolve_stack_specifier(&name_id_map, output_sp)?;

		params.p.recipes.push(Recipe {
			inputs,
			output,
		});
	}

	Ok(params)
}

fn default_game_params(nm :NameIdMap) -> Result<ServerGameParams, StrErr> {
	let file_str = DEFAULT_GAME_PARAMS_STR;
	let val = from_str(&file_str)?;
	let res = from_val(val, nm)?;
	Ok(res)
}

#[cfg(test)]
#[test]
fn default_game_params_parse_test() {
	let nm = NameIdMap::builtin_name_list();
	default_game_params(nm).unwrap();
}

pub fn load_params_failible(nm :NameIdMap) -> Result<ServerGameParams, StrErr> {
	let file_str = read_to_string("game-params.toml")
		.unwrap_or_else(|err| {
			println!("Using default game params because of error: {}", err);
			DEFAULT_GAME_PARAMS_STR.to_owned()
		});

	let val = from_str(&file_str)?;
	let res = from_val(val, nm)?;
	Ok(res)
}

static DEFAULT_GAME_PARAMS_STR :&str = include_str!("game-params.toml");
