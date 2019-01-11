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
