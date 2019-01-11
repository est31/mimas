use std::fs::read_to_string;
use toml::from_str;
use super::StrErr;

#[derive(Deserialize)]
pub struct ServerConfig {
	pub mapgen_radius_xy :isize,
	pub mapgen_radius_z :isize,
}

impl Default for ServerConfig {
	fn default() -> Self {
		Self {
			mapgen_radius_xy : 5,
			mapgen_radius_z : 2,
		}
	}
}

pub fn load_config_failible() -> Result<ServerConfig, StrErr> {
	let file_str = read_to_string("settings.toml")?;
	let res = from_str(&file_str)?;
	Ok(res)
}

pub fn load_config() -> ServerConfig {
	load_config_failible().unwrap_or_else(|_| Default::default())
}
