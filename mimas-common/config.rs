use anyhow::Result;
use std::fs::read_to_string;
use toml::from_str;

#[derive(Deserialize, Clone)]
pub struct Config {
	// Server settings

	#[serde(default = "mapgen_seed_default")]
	pub mapgen_seed :u64,
	#[serde(default = "mapgen_radius_xy_default")]
	pub mapgen_radius_xy :isize,
	#[serde(default = "mapgen_radius_z_default")]
	pub mapgen_radius_z :isize,
	#[serde(default = "sent_chunks_radius_xy_default")]
	pub sent_chunks_radius_xy :isize,
	#[serde(default = "sent_chunks_radius_z_default")]
	pub sent_chunks_radius_z :isize,
	#[serde(default)]
	pub map_storage_path :Option<String>,

	// Client settings

	#[serde(default)]
	pub draw_poly_lines :bool,
	#[serde(default = "viewing_range_default")]
	pub viewing_range :f32,
	#[serde(default = "fog_near_default")]
	pub fog_near :f32,
	#[serde(default = "fog_far_default")]
	pub fog_far :f32,
}

// Long-term missing feature of serde
// https://github.com/serde-rs/serde/issues/368

fn mapgen_seed_default() -> u64 { 78 }
fn mapgen_radius_xy_default() -> isize { 5 }
fn mapgen_radius_z_default() -> isize { 2 }
fn sent_chunks_radius_xy_default() -> isize { 6 }
fn sent_chunks_radius_z_default() -> isize { 3 }
fn viewing_range_default() -> f32 { 128.0 }
fn fog_near_default() -> f32 { 40.0 }
fn fog_far_default() -> f32 { 60.0 }

impl Default for Config {
	fn default() -> Self {
		Self {
			mapgen_seed : 78,
			mapgen_radius_xy : 5,
			mapgen_radius_z : 2,
			sent_chunks_radius_xy : 6,
			sent_chunks_radius_z : 3,
			map_storage_path : None,

			draw_poly_lines : false,
			viewing_range : 128.0,
			fog_near : 40.0,
			fog_far : 60.0,
		}
	}
}

pub fn load_config_failible() -> Result<Config> {
	let file_str = read_to_string("settings.toml")?;
	let res = from_str(&file_str)?;
	Ok(res)
}

pub fn load_config() -> Config {
	load_config_failible().unwrap_or_else(|e| {
		println!("Using default configuration due to error: {:?}", e);
		Default::default()
	})
}
