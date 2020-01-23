use crate::crafting::Recipe;
use std::sync::Arc;
use toml::from_str;
use toml::value::{Value, Array, Table};
use std::fs::{read_to_string, File};
use std::path::Path;
use crate::map::MapBlock;
use super::StrErr;
use crate::toml_util::TomlReadExt;
use std::collections::HashMap;
use std::fmt::Display;
use std::num::NonZeroU16;
use std::str::FromStr;
use std::io::Read;
use crate::inventory::Stack;
use crate::mapgen::{Schematic, self};
use sha2::{Sha256, Digest};

pub type GameParamsHdl = Arc<GameParams>;
pub type ServerGameParamsHdl = Arc<ServerGameParams>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DrawStyle {
	Colored([f32; 4]),
	Crossed(String),
	Texture(String),
	TextureSidesTop(String, String),
	TextureSidesTopBottom(String, String, String),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolGroup {
	pub group :DigGroup,
	pub speed :f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockParams {
	pub draw_style :Option<DrawStyle>,
	pub pointable :bool,
	pub placeable :bool,
	pub solid :bool,
	pub inventory :Option<u8>,
	pub display_name :String,
	pub drops :Stack,
	pub dig_group :DigGroup,
	pub tool_groups :Vec<ToolGroup>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct DigGroup(u8);

impl Default for DigGroup {
	fn default() -> Self {
		DigGroup::from_id_unchecked(UncheckedId::new(0))
	}
}

impl Id for DigGroup {
	fn id(self) -> u8 {
		self.0
	}
	fn from_id_unchecked(id :UncheckedId) -> Self {
		DigGroup(id.id())
	}
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockRoles {
	pub air :MapBlock,
	pub water :MapBlock,
	pub sand :MapBlock,
	pub ground :MapBlock,
	pub ground_top :MapBlock,
	pub wood :MapBlock,
	pub stone :MapBlock,
	pub leaves :MapBlock,
	pub tree :MapBlock,
	pub cactus :MapBlock,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Schematics {
	pub tree_schematic :Schematic,
	pub cactus_schematic :Schematic,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct UncheckedId(u8);

impl UncheckedId {
	pub(super) fn new(id :u8) -> Self {
		Self(id)
	}
	pub fn id(self) -> u8 {
		self.0
	}
}

pub trait Id :Sized + Clone + Copy + Eq {
	fn id(self) -> u8;
	fn from_id_unchecked(id :UncheckedId) -> Self;
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NameIdMap<T :Id = MapBlock> {
	first_invalid_id :u8,
	name_to_id :HashMap<String, T>,
	id_to_name :Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameParams {
	pub recipes :Vec<Recipe>,
	pub block_params :Vec<BlockParams>,
	pub block_roles :BlockRoles,
	pub schematics :Schematics,
	pub name_id_map :NameIdMap,
	pub dig_group_ids :NameIdMap<DigGroup>,
	pub texture_hashes :HashMap<String, Vec<u8>>,
	pub hand_tool_groups :Vec<ToolGroup>,
}

pub struct Ore {
	pub(crate) block :MapBlock,
	pub(crate) noise_seed :[u8; 8],
	pub(crate) pcg_seed :[u8; 8],
	pub(crate) freq :f64,
	pub(crate) pcg_chance :f64,
	pub(crate)limit_a :f64,
	pub(crate) limit_b :f64,
	pub(crate) limit_boundary :isize,
}

pub struct Plant {
	pub(crate) block :MapBlock,
	pub(crate) pcg_seed :[u8; 8],
	pub(crate) pcg_limit :f64,
}

pub struct MapgenParams {
	pub ores :Vec<Ore>,
	pub plants :Vec<Plant>,
}

pub struct ServerGameParams {
	pub p :GameParams,
	pub mapgen_params :MapgenParams,
	pub textures :HashMap<Vec<u8>, Vec<u8>>,
}

fn hand_tool_groups(dig_group_ids :&mut NameIdMap<DigGroup>) -> Vec<ToolGroup> {
	vec![
		ToolGroup {
			group : dig_group_ids.get_or_extend("default:default"),
			speed : 10.0,
		},
	]
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
			placeable : true,
			solid : true,
			display_name : String::new(),
			inventory : None,
			drops : Stack::Empty,
			dig_group : DigGroup::default(),
			tool_groups : Vec::new(),
		}
	}
}

impl BlockRoles {
	fn with_get_id(get_id :impl Fn(&'static str) -> Result<MapBlock, String>) -> Result<Self, StrErr> {
		Ok(Self {
			air : get_id("default:air")?,
			water : get_id("default:water")?,
			sand : get_id("default:sand")?,
			ground : get_id("default:ground")?,
			ground_top : get_id("default:ground_with_grass")?,
			wood : get_id("default:wood")?,
			stone : get_id("default:stone")?,
			leaves : get_id("default:leaves")?,
			tree : get_id("default:tree")?,
			cactus : get_id("default:cactus")?,
		})
	}
	pub fn new(m :&NameIdMap) -> Result<Self, StrErr> {
		let get_id = |n| {
			m.get_id(n)
				.ok_or_else(|| format!("Coudln't find id for builtin role '{}'", n))
		};
		Self::with_get_id(get_id)
	}
	pub fn dummy(m :&NameIdMap) -> Result<Self, StrErr> {
		let get_id = |n| {
			m.get_id(n)
				.ok_or_else(|| format!("Coudln't find id for builtin role '{}'", n))
		};
		let air_id = get_id("default:air")?;
		Self::with_get_id(|_| Ok(air_id))
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
}
impl NameIdMap<DigGroup> {
	pub fn default_group_list() -> Self {
		Self::from_name_list(vec![
			"group:default",
		])
	}
}

impl<T :Id> NameIdMap<T> {
	pub fn from_name_list(names :Vec<impl Into<String>>) -> Self {
		let mut name_to_id = HashMap::new();
		let mut id_to_name = Vec::with_capacity(names.len());
		let mut id = 0;
		for name in names.into_iter() {
			let name = name.into();
			id_to_name.push(name.clone());
			let mb = T::from_id_unchecked(UncheckedId::new(id));
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
	fn get_or_extend(&mut self, name :impl Into<String>) -> T {
		let name = name.into();
		let id_to_name = &mut self.id_to_name;
		let first_invalid_id = &mut self.first_invalid_id;
		let mb = self.name_to_id.entry(name.clone())
			.or_insert_with(|| {
				id_to_name.push(name.clone());
				let mb = T::from_id_unchecked(UncheckedId::new(*first_invalid_id));
				*first_invalid_id += 1;
				mb
			});
		*mb
	}
	pub fn mb_from_id(&self, id :u8) -> Option<T> {
		if id >= self.first_invalid_id {
			return None;
		}
		Some(T::from_id_unchecked(UncheckedId::new(id)))
	}
	pub fn get_name(&self, mb :T) -> Option<&str> {
		self.id_to_name.get(mb.id() as usize)
			.map(|v| {
				let v :&str = &*v;
				v
			})
	}
	pub fn get_id<'a>(&self, s :impl Into<&'a str>) -> Option<T> {
		self.name_to_id.get(s.into())
			.map(|v| *v)
	}
}

impl GameParams {
	pub fn get_block_params(&self, blk :MapBlock) -> Option<&BlockParams> {
		self.block_params.get(blk.id() as usize)
	}
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
				MapBlock::from_id_unchecked(UncheckedId::new(id as u8))
			})
	}
}

impl ServerGameParams {
	pub fn load(nm :NameIdMap) -> ServerGameParamsHdl {
		Arc::new(load_params_failible(nm).expect("Couldn't load game params"))
	}
}

/// Ensures that the modname:name format is used and
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
		let block_roles = BlockRoles::dummy(&nm_from_db)?;
		let schematics = Schematics::new(&block_roles);
		let mut dig_group_ids = NameIdMap::default_group_list();
		let hand_tool_groups = hand_tool_groups(&mut dig_group_ids);
		let p = GameParams {
			recipes : Vec::new(),
			block_params : Vec::new(),
			schematics,
			block_roles,
			name_id_map : nm_from_db,
			texture_hashes : HashMap::new(),
			dig_group_ids,
			hand_tool_groups,
		};
		let mapgen_params = MapgenParams {
			ores : Vec::new(),
			plants : Vec::new(),
		};
		ServerGameParams {
			p,
			mapgen_params,
			textures :HashMap::new(),
		}
	};
	let name_id_map = &mut params.p.name_id_map;

	// First step: populate the name id map.
	// This allows us to refer to blocks other than our own
	// regardless of order.
	let blocks = val.read::<Array>("block")?;
	for block in blocks.iter() {
		let name = block.read::<str>("name")?;
		let _name_components = parse_block_name(name)?;
		let _id = name_id_map.get_or_extend(name);
	}

	// Second step: non-dummy block roles
	// (now that we have the name id map).
	params.p.block_roles = BlockRoles::new(&name_id_map)?;
	params.p.schematics = Schematics::new(&params.p.block_roles);

	params.p.block_params.resize_with(name_id_map.first_invalid_id as usize,
		Default::default);

	let mut textures = Vec::new();

	for block in blocks.iter() {
		let name = block.read::<str>("name")?;
		let name_components = parse_block_name(name)?;
		// unwrap is okay because we have added it in the first pass
		let id = name_id_map.get_id(name).unwrap();
		let texture = block.get("texture");
		let crossed = if let Some(v) = block.get("crossed") {
			Some(v.convert::<bool>()?.to_owned())
		} else {
			None
		};
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
				if crossed == Some(true) {
					Some(DrawStyle::Crossed(texture.to_owned()))
				} else {
					Some(DrawStyle::Texture(texture.to_owned()))
				}
			},
			(None, Some(Value::Array(arr))) => {
				if arr.len() == 2 {
					let arr :[String; 2] = Value::Array(arr.clone()).try_into()?;
					textures.extend_from_slice(&arr);
					Some(DrawStyle::TextureSidesTop(arr[0].clone(), arr[1].clone()))
				} else if arr.len() == 3 {
					let arr :[String; 3] = Value::Array(arr.clone()).try_into()?;
					textures.extend_from_slice(&arr);
					Some(DrawStyle::TextureSidesTopBottom(arr[0].clone(), arr[1].clone(), arr[2].clone()))
				} else {
					Err(format!("false number of textures: {}", arr.len()))?
				}
			},
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
		let placeable = block.get("placeable")
			.unwrap_or(&Value::Boolean(true));
		let placeable = *placeable.convert::<bool>()?;
		let solid = block.get("solid")
			.unwrap_or(&Value::Boolean(true));
		let solid = *solid.convert::<bool>()?;
		let inventory = if let Some(v) = block.get("inventory") {
			Some(v.convert::<i64>()?.to_owned() as u8)
		} else {
			None
		};
		let drops = if let Some(drops) = block.get("drops") {
			let drops_sp = drops.convert::<str>()?;
			resolve_stack_specifier(&name_id_map, drops_sp)?
		} else {
			Stack::with(id, 1)
		};
		let dig_group = if let Some(dg) = block.get("dig_group") {
			let dg = dg.convert::<str>()?;
			params.p.dig_group_ids.get_or_extend(dg)
		} else {
			DigGroup::default()
		};
		let tool_groups = if let Some(tgs) = block.get("tool_groups") {
			let tgs = tgs.convert::<Vec<Value>>()?;
			let dig_group_ids = &mut params.p.dig_group_ids;
			tgs.iter()
				.map(|tg| {
					let group = tg.read::<str>("group")?;
					let gr_id = dig_group_ids.get_or_extend(group);
					let speed = *tg.read::<f64>("speed")?;
					Ok(ToolGroup {
						group : gr_id,
						speed,
					})
				})
				.collect::<Result<Vec<ToolGroup>, StrErr>>()?
		} else {
			Vec::new()
		};

		let block_params = BlockParams {
			draw_style,
			pointable,
			placeable,
			solid,
			display_name,
			inventory,
			drops,
			dig_group,
			tool_groups,
		};
		params.p.block_params[id.id() as usize] = block_params;
	}

	let texture_h = &mut params.p.texture_hashes;
	let texture_bl = &mut params.textures;
	let asset_dir = std::env::current_exe()?
		.parent()
		.unwrap_or_else(|| Path::new("."))
		.join("..")
		.join("..");
	#[cfg(test)]
	let asset_dir = asset_dir.join("..");
	texture_hashes(asset_dir, textures)?
		.into_iter()
		.for_each(|(name, hash, blob)| {
			texture_h.insert(name, hash.clone());
			texture_bl.insert(hash, blob);
		});

	if let Some(recipes_list) = val.get("recipe") {
		let recipes_list = recipes_list.convert::<Array>()?;
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
	}

	if let Some(mapgen) = val.get("mapgen") {
		let mapgen = mapgen.convert::<Table>()?;
		if let Some(ores) = mapgen.get("ore") {
			let ores = ores.convert::<Array>()?;
			for ore in ores.iter() {
				let name = ore.read::<str>("name")?;
				let _name_components = parse_block_name(name)?;
				let id = name_id_map.get_id(name).ok_or("invalid name")?;

				let noise_seed = ore.read::<str>("noise_seed")?;
				if noise_seed.len() != 8 {
					Err(format!("noise_seed needs to be 8 bytes long but has length {}", noise_seed.len()))?
				}
				let noise_seed = noise_seed.as_bytes();
				let noise_seed = [noise_seed[0], noise_seed[1], noise_seed[2], noise_seed[3],
					noise_seed[4], noise_seed[5], noise_seed[6], noise_seed[7]];

				let pcg_seed = ore.read::<str>("pcg_seed")?;
				if pcg_seed.len() != 8 {
					Err(format!("pcg_seed needs to be 8 bytes long but has length {}", pcg_seed.len()))?
				}
				let pcg_seed = pcg_seed.as_bytes();
				let pcg_seed = [pcg_seed[0], pcg_seed[1], pcg_seed[2], pcg_seed[3],
					pcg_seed[4], pcg_seed[5], pcg_seed[6], pcg_seed[7]];

				let freq = *ore.read::<f64>("freq")?;
				let pcg_chance = *ore.read::<f64>("pcg_limit")?;
				let limit_a = *ore.read::<f64>("limit_a")?;
				let limit_b = *ore.read::<f64>("limit_b")?;
				let limit_boundary = *ore.read::<i64>("limit_boundary")? as isize;
				params.mapgen_params.ores.push(Ore {
					block : id,
					noise_seed,
					pcg_seed,
					freq,
					pcg_chance,
					limit_a,
					limit_b,
					limit_boundary,
				});
			}
		}
		if let Some(plants) = mapgen.get("plant") {
			let plants = plants.convert::<Array>()?;
			for plant in plants.iter() {
				let name = plant.read::<str>("name")?;
				let _name_components = parse_block_name(name)?;
				let id = name_id_map.get_id(name).ok_or("invalid name")?;

				let pcg_seed = plant.read::<str>("pcg_seed")?;
				if pcg_seed.len() != 8 {
					Err(format!("pcg_seed needs to be 8 bytes long but has length {}", pcg_seed.len()))?
				}
				let pcg_seed = pcg_seed.as_bytes();
				let pcg_seed = [pcg_seed[0], pcg_seed[1], pcg_seed[2], pcg_seed[3],
					pcg_seed[4], pcg_seed[5], pcg_seed[6], pcg_seed[7]];

				let pcg_limit = *plant.read::<f64>("pcg_limit")?;
				params.mapgen_params.plants.push(Plant {
					block : id,
					pcg_seed,
					pcg_limit,
				});
			}

		}
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
