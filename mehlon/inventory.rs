use mehlon_server::map::MapBlock;

pub struct SelectableInventory {
	item :MapBlock,
}

impl SelectableInventory {
	pub fn new() -> Self {
		Self {
			item : MapBlock::Wood,
		}
	}
	pub fn get_selected(&self) -> Option<MapBlock> {
		Some(self.item)
	}
	pub fn rotate(&mut self, forwards :bool) {
		fn rot(mb :MapBlock) -> MapBlock {
				use mehlon_server::map::MapBlock::*;
				match mb {
						Water => Ground,
						Ground => Sand,
						Sand => Wood,
						Wood => Stone,
						Stone => Leaves,
						Leaves => Tree,
						Tree => Cactus,
						Cactus => Coal,
						Coal => Water,
						_ => unreachable!(),
				}
		}
		if forwards {
			self.item = rot(self.item);
		} else {
			for _ in 0 .. 8 {
				self.item = rot(self.item);
			}
		}
	}
	pub fn put_item(&mut self, _item :MapBlock) {
		// TODO actually put it into the inventory
	}
}
