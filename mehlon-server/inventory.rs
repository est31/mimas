use map::MapBlock;
use std::num::NonZeroU16;
use byteorder::{ReadBytesExt, WriteBytesExt, BigEndian};
use map_storage::{mapblock_to_number, number_to_mapblock};
use StrErr;

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SelectableInventory {
	selection :Option<usize>,
	stacks :Box<[Stack]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
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
	pub fn put(&mut self, other :Stack, allow_empty :bool,
			limit :u16) -> Stack {
		if self.is_empty() {
			if !allow_empty {
				return other;
			}
			*self = other;
			return Stack::Empty;
		}
		if let Stack::Content { item : item2, count : count2, } = other {
			let (item, count) = self.content().unwrap();
			if item == item2 {
				let wanted_count = (count as u32) + (count2.get() as u32);
				let limit_exceeding = wanted_count.saturating_sub(limit as u32);
				*self = Stack::with(item, (wanted_count - limit_exceeding) as u16);
				return Stack::with(item, limit_exceeding as u16);
			}
		}
		return other;
	}
	pub fn take_n(&mut self, n :u16) -> Option<(MapBlock, u16, bool)> {
		match self {
			Stack::Empty => None,
			Stack::Content { item, count, } => {
				let mut emptied = false;
				let item = *item;
				let new_count = count.get().checked_sub(n);
				let items_removed = new_count.unwrap_or(count.get());
				let new_count_nonzero = new_count.and_then(|v| NonZeroU16::new(v));
				if let Some(new_count) = new_count_nonzero {
					*count = new_count;
				} else {
					*self = Stack::Empty;
					emptied = true;
				}
				Some((item, items_removed, emptied))
			},
		}
	}
	pub fn take_one(&mut self) -> Option<(MapBlock, bool)> {
		self.take_n(1).map(|(c, _n, emptied)| (c, emptied))
	}
}

impl SelectableInventory {
	pub fn new() -> Self {
		Self::with_stuff_inside()
	}
	pub fn with_stuff_inside() -> Self {
		use map::MapBlock::*;
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
				Stack::Empty,
				Stack::Empty,
				Stack::Empty,
				].into_boxed_slice(),
		}
	}
	pub fn get_selected(&self) -> Option<MapBlock> {
		self.selection.and_then(|idx| {
			self.stacks[idx].content().map(|(it, _count)| it)
		})
	}
	pub fn take_selected(&mut self) -> Option<MapBlock> {
		self.selection.and_then(|idx| {
			self.stacks[idx].take_one().map(|(it, emptied)| {
				if emptied && self.selection == Some(idx) {
					// Update the selection
					self.rotate(true);
				}
				it
			})
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
	pub fn put(&mut self, stack :Stack) -> Stack {
		let mut stack = stack;
		let selection = self.selection.unwrap_or(0);
		let stack_count = self.stacks.len();
		// Stack size limit
		const STACK_SIZE_LIMIT :u16 = 60;
		let mut last_idx_changed = None;
		// First put into the non-empty stacks
		for offs in 0 .. stack_count {
			let idx = (selection + offs) % stack_count;
			let new_stack = self.stacks[idx].put(stack, false, STACK_SIZE_LIMIT);
			if stack != new_stack {
				last_idx_changed = Some(idx);
			}
			stack = new_stack;
			if stack.is_empty() {
				break;
			}
		}
		// Then put into the possibly empty stacks
		for offs in 0 .. stack_count {
			let idx = (selection + offs) % stack_count;
			let new_stack = self.stacks[idx].put(stack, true, STACK_SIZE_LIMIT);
			if stack != new_stack {
				last_idx_changed = Some(idx);
			}
			if stack.is_empty() {
				break;
			}
		}
		// Set selection if it's none
		if self.selection.is_none() {
			self.selection = last_idx_changed;
		}
		stack
	}
	pub fn selection(&self) -> Option<usize> {
		self.selection
	}
	pub fn stacks(&self) -> &Box<[Stack]> {
		&self.stacks
	}
	pub fn serialize(&self) -> Vec<u8> {
		let mut res = Vec::new();
		self.serialize_to(&mut res);
		res
	}
	pub fn serialize_to(&self, res :&mut Vec<u8>) {
		res.write_u8(0).unwrap();
		let selection_id = self.selection.unwrap_or(0) + 1;
		res.write_u16::<BigEndian>(selection_id as u16).unwrap();
		res.write_u16::<BigEndian>(self.stacks.len() as u16).unwrap();
		for st in self.stacks.iter() {
			let (id, count) = st.content()
				.map(|(b, cnt)| (mapblock_to_number(b), cnt))
				.unwrap_or((0, 0)); // id doesn't matter if count is 0
			res.write_u8(id).unwrap();
			res.write_u16::<BigEndian>(count).unwrap();
		}
	}
	pub fn deserialize(buf :&[u8]) -> Result<Self, StrErr> {
		let mut rdr = buf;
		let version = rdr.read_u8()?;
		if version != 0 {
			// The version is too recent
			Err(format!("Unsupported map chunk version {}", version))?;
		}
		let selection_id = rdr.read_u16::<BigEndian>()?;
		let selection = if selection_id == 0 {
			None
		} else {
			Some((selection_id - 1) as usize)
		};

		let cnt = rdr.read_u16::<BigEndian>()?;
		let mut stacks = Vec::new();
		for _ in 0 .. cnt {
			let item_id = rdr.read_u8()?;
			let count = rdr.read_u16::<BigEndian>()?;
			if let Some(count) = NonZeroU16::new(count) {
				let item = number_to_mapblock(item_id)
					.ok_or_else(|| "invalid item id".to_owned())?;
				stacks.push(Stack::Content {
					item,
					count,
				});
			} else {
				stacks.push(Stack::Empty);
			}
		}
		Ok(Self {
			selection,
			stacks : stacks.into_boxed_slice(),
		})
	}
}
