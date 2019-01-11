use std::fs::read_to_string;
use toml::from_str;
use super::StrErr;

#[derive(Deserialize, Clone)]
pub struct Config {
	pub mapgen_radius_xy :isize,
	pub mapgen_radius_z :isize,

	pub viewing_range :f32,
	pub fog_near :f32,
	pub fog_far :f32,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			mapgen_radius_xy : 5,
			mapgen_radius_z : 2,

			viewing_range : 128.0,
			fog_near : 40.0,
			fog_far : 60.0,
		}
	}
}

pub fn load_config_failible() -> Result<Config, StrErr> {
	let file_str = read_to_string("settings.toml")?;
	let res = from_str(&file_str)?;
	Ok(res)
}

pub fn load_config() -> Config {
	load_config_failible().unwrap_or_else(|_| Default::default())
}
