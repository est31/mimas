use mehlon_server::map::MapBlock;
use std::num::NonZeroU16;

pub struct SelectableInventory {
	selection :Option<usize>,
	stacks :Box<[Stack]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stack {
	Empty,
	Content {
		item :MapBlock,
		count :NonZeroU16,
	},
}

impl Stack {
	pub fn with(item :MapBlock, count :u16) -> Self {
		if let Some(count) = NonZeroU16::new(count) {
			Stack::Content {
				item,
				count,
			}
		} else {
			Stack::Empty
		}
	}
	pub fn is_empty(&self) -> bool {
		self == &Stack::Empty
	}
	pub fn content(&self) -> Option<(MapBlock, u16)> {
		match self {
			Stack::Empty => None,
			Stack::Content { item, count, } => {
				Some((*item, count.get()))
			},
		}
	}
}

impl SelectableInventory {
	pub fn new() -> Self {
		Self::with_stuff_inside()
	}
	pub fn with_stuff_inside() -> Self {
		use mehlon_server::map::MapBlock::*;
		Self {
			selection : None,
			stacks : vec![
				Stack::with(Water, 1),
				Stack::with(Ground, 1),
				Stack::with(Sand, 1),
				Stack::with(Wood, 1),
				Stack::with(Stone, 1),
				Stack::with(Leaves, 1),
				Stack::with(Tree, 1),
				Stack::with(Cactus, 1),
				Stack::with(Coal, 1),
				].into_boxed_slice(),
		}
	}
	pub fn get_selected(&self) -> Option<MapBlock> {
		self.selection.and_then(|idx| {
			self.stacks[idx].content().map(|(it, _count)| it)
		})
	}
	pub fn rotate(&mut self, forwards :bool) {
		let selection = self.selection.take().unwrap_or(0);
		let stack_count = self.stacks.len();
		for offs in 1 .. stack_count {
			let idx = if forwards {
				(selection + offs) % stack_count
			} else {
				(stack_count + selection - offs) % stack_count
			};
			if !self.stacks[idx].is_empty() {
				// Found non-empty stack to point at
				self.selection = Some(idx);
				break;
			}
		}
	}
	pub fn put(&mut self, _stack :Stack) -> Stack {
		// TODO implement
		Stack::Empty
	}
}
