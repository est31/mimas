use nalgebra::Vector3;

fn fmin(a :f32, b :f32) -> f32 {
	if a < b {
		a
	} else {
		b
	}
}

fn fmax(a :f32, b :f32) -> f32 {
	if a > b {
		a
	} else {
		b
	}
}

fn overlap(a_min :f32, a_max :f32, b_min :f32, b_max :f32) -> Option<f32> {
	if a_min < b_max && b_min < a_max {
		Some(fmin(a_max, b_max) - fmax(a_min, b_min))
	} else {
		None
	}
}

fn overlap_fn<T, F :Fn(T) -> f32>(f :F, a_min :T, a_max :T, b_min :T, b_max :T) -> Option<f32> {
	overlap(f(a_min), f(a_max), f(b_min), f(b_max))
}

pub fn collide(player_pos :Vector3<f32>, pos :Vector3<isize>) -> Option<Vector3<f32>> {
	let pos = pos.map(|v| v as f32);
	let player_colb_extent = Vector3::new(0.35, 0.35, 0.9);
	let pmin = player_pos - player_colb_extent;
	let pmax = player_pos + player_colb_extent;
	let cube_extent = Vector3::new(0.5, 0.5, 0.5);
	let cmin = pos - cube_extent;
	let cmax = pos + cube_extent;

	let overlaps = (
		overlap_fn(|p| p.x, pmin, pmax, cmin, cmax),
		overlap_fn(|p| p.y, pmin, pmax, cmin, cmax),
		overlap_fn(|p| p.z, pmin, pmax, cmin, cmax),
	);
	if let (Some(ox), Some(oy), Some(oz)) = overlaps {
		let normal;
		fn f(a :f32, b :f32) -> f32 {
			if a >= b {
				1.0
			} else {
				-1.0
			}
		}
		if ox < oy && ox < oz {
			normal = Vector3::new(f(player_pos.x, pos.x), 0.0, 0.0);
		} else if oy < oz {
			normal = Vector3::new(0.0, f(player_pos.y, pos.y), 0.0);
		} else {
			normal = Vector3::new(0.0, 0.0, f(player_pos.z, pos.z));
		}
		Some(normal)
	} else {
		None
	}
}
