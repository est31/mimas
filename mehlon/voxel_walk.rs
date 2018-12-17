use nalgebra::Vector3;

pub struct VoxelWalker {
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

fn next_for_dim(off :f32, dir :f32) -> f32 {
	let step = 0.01;
	let mut mult = 0.0;
	while off.floor() - (off + mult * dir).floor() == 0.0 {
		mult += step;
		if mult > 100.0 {
			return mult;
		}
	}
	mult
}

impl Iterator for VoxelWalker {
	type Item = (Vector3<f32>, Vector3<f32>);
	fn next(&mut self) -> Option<Self::Item> {
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
