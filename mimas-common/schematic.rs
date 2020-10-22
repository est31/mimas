use nalgebra::Vector3;

use crate::map::MapBlock;
use crate::game_params::BlockRoles;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Schematic {
	pub items :Vec<(Vector3<isize>, MapBlock)>,
	pub aabb_min :Vector3<isize>,
	pub aabb_max :Vector3<isize>,
}

impl Schematic {
	pub fn from_items(items :Vec<(Vector3<isize>, MapBlock)>) -> Self {
		let (aabb_min, aabb_max) = aabb_min_max(&items);
		Schematic {
			items,
			aabb_min,
			aabb_max,
		}
	}
}

pub fn tree_schematic(roles :&BlockRoles) -> Schematic {
	let mut items = Vec::new();
	for x in -1 ..= 1 {
		for y in -1 ..= 1 {
			items.push((Vector3::new(x, y, 3), roles.leaves));
			items.push((Vector3::new(x, y, 4), roles.leaves));
			items.push((Vector3::new(x, y, 5), roles.leaves));
		}
	}
	for z in 0 .. 4 {
		items.push((Vector3::new(0, 0, z), roles.tree));
	}
	Schematic::from_items(items)
}

pub fn cactus_schematic(roles :&BlockRoles) -> Schematic {
	let mut items = Vec::new();
	for z in 0 .. 4 {
		items.push((Vector3::new(0, 0, z), roles.cactus));
	}
	Schematic::from_items(items)
}

fn aabb_min_max(items :&[(Vector3<isize>, MapBlock)]) -> (Vector3<isize>, Vector3<isize>) {
	let min_x = items.iter().map(|(pos, _)| pos.x).min().unwrap();
	let min_y = items.iter().map(|(pos, _)| pos.y).min().unwrap();
	let min_z = items.iter().map(|(pos, _)| pos.z).min().unwrap();
	let max_x = items.iter().map(|(pos, _)| pos.x).max().unwrap();
	let max_y = items.iter().map(|(pos, _)| pos.y).max().unwrap();
	let max_z = items.iter().map(|(pos, _)| pos.z).max().unwrap();
	(Vector3::new(min_x, min_y, min_z), Vector3::new(max_x, max_y, max_z))
}
