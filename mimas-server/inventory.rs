use crate::map::MapBlock;
use std::num::NonZeroU16;
use std::io::Read;
use byteorder::{ReadBytesExt, WriteBytesExt, BigEndian};
use nalgebra::Vector3;
use crate::game_params::{NameIdMap, Id};
use crate::StrErr;

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
	pub fn take_n(&mut self, n :u16) -> (Stack, bool) {
		let mut emptied = false;
		let stack_taken = match self {
			Stack::Empty => Stack::Empty,
			Stack::Content { item, count, } => {
				let item = *item;
				let new_count = count.get().saturating_sub(n);
				let items_removed = count.get() - new_count;
				let new_count_nonzero = NonZeroU16::new(new_count);
				if let Some(new_count) = new_count_nonzero {
					*count = new_count;
				} else {
					*self = Stack::Empty;
					emptied = true;
				}
				Stack::with(item, items_removed)
			},
		};
		(stack_taken, emptied)
	}
	pub fn take_one(&mut self) -> Option<(MapBlock, bool)> {
		let (stack_taken, emptied) = self.take_n(1);
		stack_taken.content().map(|(c, _n)| (c, emptied))
	}
}

// Stack size limit
const STACK_SIZE_LIMIT :u16 = 60;

pub const HUD_SLOT_COUNT :usize = 8;

impl SelectableInventory {
	pub fn new() -> Self {
		Self::empty_with_size(16)
	}
	pub fn empty_with_size(size :usize) -> Self {
		Self::from_stacks(vec![Stack::Empty; size].into_boxed_slice())
	}
	pub fn from_stacks(stacks :Box<[Stack]>) -> Self {
		Self {
			selection : None,
			stacks,
		}
	}
	pub fn crafting_inv() -> Self {
		Self::from_stacks(vec![Stack::Empty; 9].into_boxed_slice())
	}
	pub fn is_empty(&self) -> bool {
		self.stacks.iter().all(Stack::is_empty)
	}
	pub fn get_selected(&self) -> Option<MapBlock> {
		self.selection.and_then(|idx| {
			self.stacks[idx].content().map(|(it, _count)| it)
		})
	}
	pub fn get_sel_idx_and_content(&self) -> Option<(usize, MapBlock)> {
		self.selection.and_then(|idx| {
			self.stacks[idx].content().map(|(it, _count)| (idx, it))
		})
	}
	pub fn take_selected(&mut self) -> Option<MapBlock> {
		self.selection.and_then(|idx| {
			self.stacks[idx].take_one().map(|(it, _emptied)| it)
		})
	}
	pub fn rotate(&mut self, forwards :bool) {
		let selection = self.selection.take().unwrap_or(0);
		let stack_count = self.stacks.len().min(HUD_SLOT_COUNT);
		let idx = if forwards {
			(selection + 1) % stack_count
		} else {
			(stack_count + selection - 1) % stack_count
		};
		self.selection = Some(idx);
	}
	pub fn put(&mut self, stack :Stack) -> Stack {
		let mut stack = stack;
		let selection = self.selection.unwrap_or(0);
		let stack_count = self.stacks.len();
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
			stack = new_stack;
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
	pub fn stacks_mut(&mut self) -> &mut Box<[Stack]> {
		&mut self.stacks
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
				.map(|(b, cnt)| (b.id(), cnt))
				.unwrap_or((0, 0)); // id doesn't matter if count is 0
			res.write_u8(id).unwrap();
			res.write_u16::<BigEndian>(count).unwrap();
		}
	}
	pub fn deserialize(buf :&[u8], m :&NameIdMap) -> Result<Self, StrErr> {
		Self::deserialize_rdr(buf, m)
	}

	pub fn deserialize_rdr(mut rdr :impl Read, m :&NameIdMap) -> Result<Self, StrErr> {
		let version = rdr.read_u8()?;
		if version != 0 {
			// The version is too recent
			Err(format!("Unsupported serialized inventory version {}", version))?;
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
				let item = m.mb_from_id(item_id)
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

pub fn merge_or_swap(invs :&mut [impl InvRef],
		from :(usize, usize), to :(usize, usize)) {
	if from == to {
		return;
	}
	let stack_from = invs[from.0].as_mut().stacks[from.1];
	let new_stack = invs[to.0].as_mut().stacks[to.1]
		.put(stack_from, false, STACK_SIZE_LIMIT);
	if stack_from != new_stack {
		// Partial merge successful
		invs[from.0].as_mut().stacks[from.1] = new_stack;
	} else {
		// Merging wasn't possible, fall back to swap
		let tmp = invs[to.0].as_mut().stacks[to.1];
		invs[to.0].as_mut().stacks[to.1] = invs[from.0].as_mut().stacks[from.1];
		invs[from.0].as_mut().stacks[from.1] = tmp;
	}
}

pub fn move_n_if_possible(invs :&mut [impl InvRef],
		from :(usize, usize), to :(usize, usize), count :u16) {
	let stack_from = invs[from.0].as_mut().stacks[from.1].take_n(count).0;
	let new_stack = invs[to.0].as_mut().stacks[to.1]
		.put(stack_from, true, STACK_SIZE_LIMIT);
	// Put back any residue
	invs[from.0].as_mut().stacks[from.1].put(new_stack, true, STACK_SIZE_LIMIT);
}

pub trait InvRef {
	fn as_mut(&mut self) -> &mut SelectableInventory;
}

impl InvRef for SelectableInventory {
	fn as_mut(&mut self) -> &mut SelectableInventory {
		self
	}
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InventoryPos {
	stack_pos :usize,
	/// None if the local player's inventory.
	/// Some(pos) if chest at given position.
	location :Option<Vector3<isize>>,
}
