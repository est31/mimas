use inventory::SelectableInventory;
use map::MapBlock;

pub struct Recipe {
	pub inputs :&'static [Option<MapBlock>],
	pub output : (MapBlock, u16),
}

impl Recipe {
	fn matches(&self,
			inv :&SelectableInventory) -> bool {
		if inv.stacks().len() != self.inputs.len() {
			return false;
		}
		inv.stacks().iter()
			.zip(self.inputs.iter())
			.all(|(other, ours)| {
				let otc = other.content()
					.map(|(m, _c)| m);
				otc == *ours
			})
	}
}

static RECIPES :&[Recipe] = &[
	Recipe {
		inputs : &[
			Some(MapBlock::Tree),
			None,
			None,
			None,
			None,
			None,
			None,
			None,
			None,
		],
		output : (MapBlock::Wood, 4),
	},
];

pub fn get_matching_recipe(inv :&SelectableInventory)
		-> Option<&'static Recipe> {
	RECIPES.iter().find(|r| r.matches(inv))
}
