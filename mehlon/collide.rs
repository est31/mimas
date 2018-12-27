use nalgebra::Vector3;

fn overlap(a_min :f32, a_max :f32, b_min :f32, b_max :f32) -> bool {
	!(a_min > b_max || b_min > a_max)
}

fn overlap_fn<T, F :Fn(T) -> f32>(f :F, a_min :T, a_max :T, b_min :T, b_max :T) -> bool {
	overlap(f(a_min), f(a_max), f(b_min), f(b_max))
}

pub fn collide(player_pos :Vector3<f32>, pos :Vector3<isize>) -> Option<()> {
	let pos = pos.map(|v| v as f32);
	let player_colb_extent = Vector3::new(0.35, 0.35, 0.9);
	let pmin = player_pos - player_colb_extent;
	let pmax = player_pos - player_colb_extent;
	let cube_extent = Vector3::new(0.5, 0.5, 0.5);
	let cmin = pos - cube_extent;
	let cmax = pos + cube_extent;

	let overlap = overlap_fn(|p| p.x, pmin, pmax, cmin, cmax) &&
		overlap_fn(|p| p.y, pmin, pmax, cmin, cmax) &&
		overlap_fn(|p| p.z, pmin, pmax, cmin, cmax);

	if overlap {
		Some(())
	} else {
		None
	}
}
