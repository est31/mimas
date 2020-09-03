use crate::inventory::{SelectableInventory, Stack};
use crate::game_params::GameParams;
use crate::map::MapBlock;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Recipe {
	pub inputs :Vec<Option<MapBlock>>,
	pub output :Stack,
}

impl Recipe {
	fn matches(&self,
			inv :&SelectableInventory) -> bool {
		if inv.stacks().len() < self.inputs.len() {
			return false;
		}
		// TODO we recompute this sqrt here every time.
		// might be smarter to cache it?
		let inv_size_sqrt = (inv.stacks().len() as f32).sqrt() as usize;
		let recipe_size_sqrt = (self.inputs.len() as f32).sqrt() as usize;
		let size_sqrt_diff = inv_size_sqrt - recipe_size_sqrt;
		// Try all possible offsets
		for offs_line in 0 ..= size_sqrt_diff {
			for offs_col in 0 ..= size_sqrt_diff {
				let matches = inv.stacks().iter()
					.enumerate()
					.all(|(i, stack)| {
						let stc = stack.content().map(|(m, _c)| m);
						let line = i / inv_size_sqrt;
						let col = i % inv_size_sqrt;
						let line_recipe = line.checked_sub(offs_line);
						let col_recipe = col.checked_sub(offs_col);
						if let (Some(line_recipe), Some(col_recipe)) = (line_recipe, col_recipe) {
							if (line_recipe < recipe_size_sqrt) && (col_recipe < recipe_size_sqrt) {
								let recipe_idx = line_recipe * recipe_size_sqrt + col_recipe;
								return stc == self.inputs[recipe_idx];
							}
						}
						// If we are outside the recipe, the inventory needs to be empty
						stc == None
					});
				// If there is a match for this offset,
				// return a match for the recipe
				if matches {
					return true;
				}
			}
		}
		// No offset found at which there was a match
		return false;
	}
}

pub fn get_matching_recipe<'p>(inv :&SelectableInventory, params :&'p GameParams)
		-> Option<&'p Recipe> {
	params.recipes.iter().find(|r| r.matches(inv))
}
