use nalgebra::Vector3;

pub struct VoxelWalker {
	first : bool,
	start :Vector3<f32>,
	pos :Vector3<f32>,
	direction :Vector3<f32>,
}

fn fmin(a: f32, b :f32) -> f32 {
	if a < b {
		a
	} else {
		b
	}
}

impl VoxelWalker {
	pub fn new(start :Vector3<f32>, direction :Vector3<f32>) -> Self {
		VoxelWalker {
			first : true,
			start,
			pos : start,
			direction,
		}
	}
	fn peek_next(&self) -> Vector3<f32> {
		let nx = next_for_dim(self.pos.x, self.direction.x);
		let ny = next_for_dim(self.pos.y, self.direction.y);
		let nz = next_for_dim(self.pos.z, self.direction.z);
		let factor = fmin(nx, fmin(ny, nz));
		self.pos + self.direction * factor
	}
}

/// Returns the minimal m such that
/// (off + dir * m).floor() - off.floor() > 0.
fn next_for_dim(off :f32, dir :f32) -> f32 {
	let off_floor = off.floor();
	let off_next = if dir < 0.0 {
		off_floor - 0.00001
	} else {
		off_floor + 1.0
	};
	if dir == 0.0 {
		return std::f32::INFINITY;
	}
	let m = (off_next - off) / dir + 0.001;
	m
}

#[cfg(test)]
#[test]
fn test_next_for_dim() {
	const SEARCH_LIMIT :f32 = 100.0;
	const SEARCH_STEP :f32 = 0.001;
	fn next_for_dim_slow(off :f32, dir :f32) -> f32 {
		let mut mult = 0.0;
		while off.floor() == (off + mult * dir).floor() {
			mult += SEARCH_STEP;
			if mult > SEARCH_LIMIT {
				return std::f32::INFINITY;
			}
		}
		mult
	}
	let step = 0.01;
	let mut off = 0.0f32;//-5.0f32;
	let mut dir = -20.0;
	while off < 5.0 {
		if off == off.floor() {
			off += step;
			continue;
		}
		while dir < 20.0 {
			let nfd = next_for_dim(off, dir);
			let nfd_s = next_for_dim_slow(off, dir);
			if nfd > SEARCH_LIMIT {
				assert_eq!(nfd_s, std::f32::INFINITY);
			} else {
				let dist = (nfd - nfd_s).abs();
				assert!(dist < SEARCH_STEP * 2,
					"off: {}, dir: {}, nfd: {}, ndfs: {}",
					off, dir, nfd, nfd_s);
			}
			dir += step;
		}
		off += step;
	}

}

impl Iterator for VoxelWalker {
	type Item = (Vector3<f32>, Vector3<f32>);
	fn next(&mut self) -> Option<Self::Item> {
		if self.first {
			self.first = false;
			return Some((self.pos, self.pos));
		}
		let next_pos = self.peek_next();
		const SELECTION_RANGE :f32 = 10.0;
		if (next_pos - self.start).norm() < SELECTION_RANGE {
			let old_pos = self.pos;
			self.pos = next_pos;
			Some((old_pos, self.pos))
		} else {
			None
		}
	}
}
