use nalgebra::Vector3;
use line_drawing::{VoxelOrigin, Voxel, WalkVoxels};
use line_drawing::steps::Steps;

pub struct VoxelWalker {
	steps :Steps<Voxel<isize>, WalkVoxels<f32, isize>>,
}

impl VoxelWalker {
	pub fn new(start :Vector3<f32>, direction :Vector3<f32>) -> Self {
		const SELECTION_RANGE :f32 = 10.0;
		let pointing_at_distance = start + direction * SELECTION_RANGE;
		let (dx, dy, dz) = (pointing_at_distance.x, pointing_at_distance.y, pointing_at_distance.z);
		let (px, py, pz) = (start.x, start.y, start.z);
		let steps = WalkVoxels::<f32, isize>::new((px, py, pz),
			(dx, dy, dz), &VoxelOrigin::Center).steps();
		VoxelWalker {
			steps,
		}
	}
}

impl Iterator for VoxelWalker {
	type Item = (Vector3<isize>, Vector3<isize>);
	fn next(&mut self) -> Option<Self::Item> {
		self.steps.next().map(|((xs, ys, zs), (xe, ye, ze))| {
			(Vector3::new(xs, ys, zs), Vector3::new(xe, ye, ze))
		})
	}
}
