use nalgebra::{Vector3, Isometry3};
use ncollide3d::shape::Cuboid;
use ncollide3d::query;

pub fn collide(player_pos :Vector3<f32>, pos :Vector3<isize>) -> Option<()> {
	const PRED :f32 = 0.01;
	let player_colb_extent = Vector3::new(0.35, 0.35, 0.9);
	let player_collisionbox = Cuboid::new(player_colb_extent);
	let iso = Isometry3::new(pos.map(|v| v as f32).into(), nalgebra::zero());
	let player_pos_iso = Isometry3::new(player_pos, nalgebra::zero());
	let cube = Cuboid::new(Vector3::new(0.5, 0.5, 0.5));
	query::contact(&iso, &cube, &player_pos_iso, &player_collisionbox, PRED).map(|_|())
}
