use glium::{Surface, VertexBuffer};
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	Section, Layout, HorizontalAlign,
};
use nalgebra::Vector3;
use glium::glutin::event::{KeyboardInput, VirtualKeyCode,
	ElementState, MouseButton};
use glium::glutin::dpi::PhysicalPosition;
use glium_glyph::glyph_brush::GlyphCruncher;
use mimas_server::inventory::{self, SelectableInventory, Stack,
	HUD_SLOT_COUNT};
use mimas_server::crafting::get_matching_recipe;
use mimas_server::game_params::GameParamsHdl;

use mimas_meshgen::{Vertex, TextureId, TextureIdCache};

use assets::UiColors;

pub const IDENTITY :[[f32; 4]; 4] = [
	[1.0, 0.0, 0.0, 0.0f32],
	[0.0, 1.0, 0.0, 0.0],
	[0.0, 0.0, 1.0, 0.0],
	[0.0, 0.0, 0.0, 1.0],
];

fn render_text<'a, 'b>(text :&str, ui_colors :&UiColors,
		display :&glium::Display, program :&glium::Program,
		glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {
	let screen_dims = display.get_framebuffer_dimensions();

	let mut section = Section {
		text,
		bounds : (screen_dims.0 as f32 * 0.14, screen_dims.1 as f32),
		screen_position : (screen_dims.0 as f32 / 2.0, screen_dims.1 as f32 / 2.0),
		layout : Layout::default()
			.h_align(HorizontalAlign::Center),
		color : [0.9, 0.9, 0.9, 1.0],
		.. Section::default()
	};
	let mut mesh_dims = glyph_brush.pixel_bounds(&section).unwrap();
	//mesh_dims.min.x = mesh_dims.min.y.min(section.screen_position.0 as i32);
	mesh_dims.min.y = mesh_dims.min.y.min(section.screen_position.1 as i32);
	//section.screen_position.0 -= mesh_dims.width() as f32 / 2.0;
	section.screen_position.1 -= mesh_dims.height() as f32 / 2.0;
	let border = 4;
	let dims = (mesh_dims.width() + border, mesh_dims.height() + border);
	let vertices = square_mesh(dims, screen_dims, ui_colors.background_color);
	draw_ui_vertices(&vertices, display, program, target);
	glyph_brush.queue(section);
	glyph_brush.draw_queued(display, target);
}

pub fn render_menu<'a, 'b>(ui_colors :&UiColors, display :&glium::Display, program :&glium::Program,
		glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {
	render_text("Menu\nPress esc to continue Game", ui_colors, display, program, glyph_brush, target);
}

pub struct ChatWindow {
	text : String,
}

pub enum ChatWindowEvent {
	CloseChatWindow,
	SendChat,
	None,
}

impl ChatWindow {
	pub fn new() -> Self {
		Self::with_text("".to_owned())
	}
	pub fn with_text(text :String) -> Self {
		ChatWindow {
			text,
		}
	}
	pub fn text(&self) -> &str {
		&self.text
	}
	pub fn render<'a, 'b>(&self, ui_colors :&UiColors, display :&glium::Display,
			program :&glium::Program, glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {
		let text = "Type to chat\n".to_owned() + &self.text;
		render_text(&text, ui_colors, display, program, glyph_brush, target);
	}
	pub fn handle_character(&mut self, input :char) -> ChatWindowEvent {
		if input == '\n' {
			return ChatWindowEvent::SendChat;
		}
		if input == '\x08' {
			// Backspace. Remove last character.
			self.text.pop();
			return ChatWindowEvent::None;
		}
		self.text.push(input);
		ChatWindowEvent::None
	}
	pub fn handle_kinput(&mut self, input :&KeyboardInput) -> ChatWindowEvent {
		match (input.virtual_keycode, input.state) {
			(Some(VirtualKeyCode::Escape), ElementState::Pressed) => {
				ChatWindowEvent::CloseChatWindow
			},
			(Some(VirtualKeyCode::Return), ElementState::Pressed) => {
				ChatWindowEvent::SendChat
			},
			_ => ChatWindowEvent::None,
		}
	}
}

enum LayoutNodeKind {
	Container {
		children :Vec<LayoutNode>,
		horizontal :bool,
	},
	FixedSizeObject {
		id :usize,
		dimensions :(f32, f32),
	},
}

#[derive(Default)]
struct LayoutState {
	dimension_x :Option<f32>,
	dimension_y :Option<f32>,
	offs_relative_x :Option<f32>,
	offs_relative_y :Option<f32>,
	offs_absolute_x :Option<f32>,
	offs_absolute_y :Option<f32>,
}

impl LayoutState {
	fn offs_absolute(&self) -> Option<(f32, f32)> {
		if let (Some(x), Some(y)) = (self.offs_absolute_x, self.offs_absolute_y) {
			Some((x, y))
		} else {
			None
		}
	}
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
enum LayoutProgress {
	Started,
	DimensionsKnown,
	OffsetsKnown,
	Finished,
}

struct LayoutNode {
	kind :LayoutNodeKind,
	progress :LayoutProgress,
	state :LayoutState,
}

fn fmax(a :f32, b :f32) -> f32 {
	if a > b {
		a
	} else {
		b
	}
}

impl LayoutNode {
	fn from_kind(kind :LayoutNodeKind) -> Self {
		Self {
			kind,
			progress : LayoutProgress::Started,
			state : Default::default(),
		}
	}
	fn inv(id :usize,
			slots_total :usize, slots_x :usize, unit :f32) -> Self {
		Self::from_kind(LayoutNodeKind::FixedSizeObject {
			id,
			dimensions : {
				let slots_x = slots_x as f32;
				let craft_height_units = (slots_total as f32 / slots_x).ceil();
				(slots_x * unit * 1.1, craft_height_units * unit * 1.1)
			},
		})
	}
	fn spacer(id :usize, dimensions :(f32, f32)) -> Self {
		Self::from_kind(LayoutNodeKind::FixedSizeObject {
			id,
			dimensions,
		})
	}

	#[allow(unused)]
	fn find_state(&self, for_id :usize) -> Option<&LayoutState> {
		use self::LayoutNodeKind::*;
		match &self.kind {
			Container { ref children, horizontal :_ } => {
				for child in children.iter() {
					if let Some(state) = child.find_state(for_id) {
						return Some(state);
					}
				}
			},
			FixedSizeObject { id, dimensions :_ } => {
				if for_id == *id {
					return Some(&self.state);
				}
			},
		}
		return None;
	}
	fn progress(&self) -> LayoutProgress {
		self.progress
	}
	fn layout(&mut self) {
		use self::LayoutNodeKind::*;

		// Early return so that we don't recurse when finished
		if self.progress == LayoutProgress::Finished {
			return;
		}

		if self.progress == LayoutProgress::Started {
			match &mut self.kind {
				Container { ref mut children, horizontal } => {
					for child in children.iter_mut() {
						child.layout();
					}
					if *horizontal {
						// Horizontal container.
						// Sum over x extent, maximize y extent.
						if self.state.dimension_x.is_none() {
							let dim_x = children.iter()
								.map(|ch| ch.state.dimension_x)
								.try_fold(0.0, |p, v| v.map(|v| p + v));
							self.state.dimension_x = dim_x;
						}
						if self.state.dimension_y.is_none() {
							let dim_y = children.iter()
								.map(|ch| ch.state.dimension_y)
								.try_fold(0.0, |p, v| v.map(|v| fmax(p, v)));
							self.state.dimension_y = dim_y;
						}
					} else {
						// Vertical container.
						// Maximize x extent, sum over y extent.
						if self.state.dimension_x.is_none() {
							let dim_x = children.iter()
								.map(|ch| ch.state.dimension_x)
								.try_fold(0.0, |p, v| v.map(|v| fmax(p, v)));
							self.state.dimension_x = dim_x;
						}
						if self.state.dimension_y.is_none() {
							let dim_y = children.iter()
								.map(|ch| ch.state.dimension_y)
								.try_fold(0.0, |p, v| v.map(|v| p + v));
							self.state.dimension_y = dim_y;
						}
					}
				},
				FixedSizeObject { id :_, dimensions } => {
					if self.state.dimension_x.is_none() {
						self.state.dimension_x = Some(dimensions.0);
					}
					if self.state.dimension_y.is_none() {
						self.state.dimension_y = Some(dimensions.1);
					}
				},
			}
			if self.state.dimension_x.is_some()
					&& self.state.dimension_y.is_some() {
				self.progress = LayoutProgress::DimensionsKnown;
			}
		}

		if self.progress == LayoutProgress::DimensionsKnown {
			match &mut self.kind {
				Container { ref mut children, horizontal } => {
					if *horizontal {
						// Horizontal container.
						// Set relative x offsets to sum, y offsets to zero.
						let mut sum = 0.0;
						for child in children.iter_mut() {
							child.state.offs_relative_x = Some(sum);
							child.state.offs_relative_y = Some(0.0);
							// unwrap is safe due to algorithm
							sum += child.state.dimension_x.unwrap();
						}
					} else {
						// Vertical container.
						// Set relative x offsets to zero, y offsets to sum.
						let mut sum = 0.0;
						for child in children.iter_mut() {
							child.state.offs_relative_x = Some(0.0);
							child.state.offs_relative_y = Some(sum);
							// unwrap is safe due to algorithm
							sum += child.state.dimension_y.unwrap();
						}
					}
				},
				FixedSizeObject { id :_, dimensions :_ } => {
					self.state.offs_relative_x = Some(0.0);
					self.state.offs_relative_y = Some(0.0);
				},
			}
			self.progress = LayoutProgress::OffsetsKnown;
		}

		if self.progress == LayoutProgress::OffsetsKnown {
			if let (Some(offs_absolute_x), Some(offs_absolute_y)) =
					(self.state.offs_absolute_x, self.state.offs_absolute_y) {
				match &mut self.kind {
					Container { ref mut children, horizontal :_ } => {
						for child in children.iter_mut() {
							// unwrap is safe due to algorithm
							let offs_relative_x = child.state.offs_relative_x.unwrap();
							child.state.offs_absolute_x = Some(offs_absolute_x + offs_relative_x);
							// unwrap is safe due to algorithm
							let offs_relative_y = child.state.offs_relative_y.unwrap();
							child.state.offs_absolute_y = Some(offs_absolute_y + offs_relative_y);
							child.layout();
						}
					},
					// Nothing to do for fixed size object
					FixedSizeObject { id :_, dimensions :_ } => (),
				}
				self.progress = LayoutProgress::Finished;
			}
		}
	}
	fn for_each_offsets(&self, f :&mut impl FnMut(usize, Option<(f32, f32)>)) {
		use self::LayoutNodeKind::*;
		match &self.kind {
			Container { ref children, horizontal :_ } => {
				for child in children.iter() {
					child.for_each_offsets(f);
				}
			},
			FixedSizeObject { id, dimensions :_ } => {
				f(*id, self.state.offs_absolute());
			},
		}
	}
}

const CRAFTING_ID :usize = 0;
const CRAFTING_OUTPUT_ID :usize = 1;
const NORMAL_INV_ID :usize = 2;

const SPACER_ID :usize = 999;

pub struct InventoryMenu {
	params :GameParamsHdl,
	invs :[SelectableInventory; 3],
	last_mouse_pos :Option<PhysicalPosition<f64>>,
	mouse_input_ev :Option<(ElementState, MouseButton)>,
	from_pos : Option<(usize, usize)>,
}

impl InventoryMenu {
	pub fn new(params :GameParamsHdl,
			inv :SelectableInventory,
			craft_inv :SelectableInventory) -> Self {
		let output_inv = SelectableInventory::from_stacks(vec![Stack::Empty].into_boxed_slice());
		let invs = [craft_inv, output_inv, inv];
		Self {
			params,
			invs,
			last_mouse_pos : None,
			mouse_input_ev : None,
			from_pos : None,
		}
	}
	pub fn inventory(&self) -> &SelectableInventory {
		&self.invs[NORMAL_INV_ID]
	}
	pub fn craft_inv(&self) -> &SelectableInventory {
		&self.invs[CRAFTING_ID]
	}
	pub fn handle_mouse_moved(&mut self, pos :PhysicalPosition<f64>)  {
		self.last_mouse_pos = Some(pos);
	}
	pub fn handle_mouse_input(&mut self, state :ElementState, button :MouseButton) {
		self.mouse_input_ev = Some((state, button));
	}
	fn update_craft_output_inv(&mut self) {
		let recipe = get_matching_recipe(&self.invs[CRAFTING_ID], &self.params);
		let stack = recipe
			.map(|r| r.output)
			.unwrap_or(Stack::Empty);
		let stacks = vec![stack].into_boxed_slice();
		self.invs[CRAFTING_OUTPUT_ID] = SelectableInventory::from_stacks(stacks);
	}
	pub fn render<'a, 'b>(&mut self,
			ui_colors :&UiColors, tid_cache :&TextureIdCache,
			display :&glium::Display, program :&glium::Program,
			glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {

		let screen_dims = display.get_framebuffer_dimensions();

		let unit = unit_from_screen_dims(screen_dims.0);

		const SLOT_COUNT_X :usize = 8;
		const CRAFT_SLOT_COUNT_X :usize = 3;

		let slot_counts_x :&[usize] = &[
			CRAFT_SLOT_COUNT_X,
			1,
			SLOT_COUNT_X,
		];

		macro_rules! inv {
			($id:expr) => {
				LayoutNode::inv($id, self.invs[$id].stacks().len(),
					slot_counts_x[$id], unit)
			};
		}
		let mut layout = LayoutNode::from_kind(LayoutNodeKind::Container {
			horizontal : false,
			children : vec![
				LayoutNode::from_kind(LayoutNodeKind::Container {
					horizontal : true,
					children : vec![
						inv!(CRAFTING_ID),
						LayoutNode::spacer(SPACER_ID, (0.1 * unit * 1.1, 0.1 * unit * 1.1)),
						inv!(CRAFTING_OUTPUT_ID),
					],
				}),
				LayoutNode::spacer(SPACER_ID, (0.1 * unit * 1.1, 0.1 * unit * 1.1)),
				inv!(NORMAL_INV_ID),
			],
		});
		let mouse_pos = self.last_mouse_pos.map(|pos|(pos.x as f32, pos.y as f32));
		let hover_idx = render_inventories(&self.params,
			ui_colors, tid_cache, display, program, glyph_brush, target,
			&mut layout, slot_counts_x, &self.invs, mouse_pos,
			self.from_pos);

		let mut swap_command = None;

		// TODO this is hacky, we change state in RENDERING code!!
		let input_ev = self.mouse_input_ev.take();
		// TODO this is hacky, we change state in RENDERING code!!
		if let (Some((state, button)), Some(hv)) = (input_ev, hover_idx) {
			if state == ElementState::Released {
				if let Some(from_pos) = self.from_pos {
					if button == MouseButton::Left {
						self.from_pos = None;
					}
					swap_command = Some((from_pos, hv, button));
				} else {
					if hv.0 == CRAFTING_OUTPUT_ID {
						// If we click onto the crafting output menu,
						// add the output to the inventory immediately.
						// Only do something if there is something to craft
						if self.invs[CRAFTING_OUTPUT_ID].stacks()[0] != Stack::Empty {
							// TODO figure out something for the remainder stack
							self.invs[NORMAL_INV_ID].put(self.invs[CRAFTING_OUTPUT_ID].stacks()[0]);
							// Reduce inputs.
							for st in self.invs[CRAFTING_ID].stacks_mut().iter_mut() {
								st.take_n(1);
							}
						}
					} else {
						self.from_pos = Some(hv);
					}
				}
			}
		}

		// TODO this is hacky, we change state in RENDERING code!!
		if let Some((from_pos, to_pos, button)) = swap_command {
			if to_pos.0 == CRAFTING_OUTPUT_ID {
				// Putting into the crafting menu is not possible
			} else {
				let maybe_only_move = match button {
					MouseButton::Left => Some(false),
					MouseButton::Right => Some(true),
					_ => None,
				};
				if let Some(only_move) = maybe_only_move {
					inventory::merge_or_move(
						&mut self.invs,
						from_pos, to_pos, only_move);
				}
			}
		}

		// TODO this is hacky, we change state in RENDERING code!!
		self.update_craft_output_inv();
	}
}

pub struct SwapCommand {
	pub from_pos :(usize, usize),
	pub to_pos :(usize, usize),
	// Whether to only move or to
	// also try merging/swapping
	pub only_move :bool,
}


pub struct ChestMenu {
	params :GameParamsHdl,
	invs :[SelectableInventory; 2],
	chest_pos :Vector3<isize>,
	last_mouse_pos :Option<PhysicalPosition<f64>>,
	mouse_input_ev :Option<(ElementState, MouseButton)>,
	from_pos :Option<(usize, usize)>,
	hover_idx :Option<(usize, usize)>,
}

impl ChestMenu {
	pub fn new(params :GameParamsHdl,
			inv :SelectableInventory,
			chest_inv :SelectableInventory,
			chest_pos :Vector3<isize>) -> Self {
		let invs = [chest_inv, inv];
		Self {
			params,
			invs,
			chest_pos,
			last_mouse_pos : None,
			mouse_input_ev : None,
			from_pos : None,
			hover_idx : None,
		}
	}
	pub fn inventory(&self) -> &SelectableInventory {
		&self.invs[CRAFTING_OUTPUT_ID]
	}
	pub fn chest_inv(&self) -> &SelectableInventory {
		&self.invs[CRAFTING_ID]
	}
	pub fn chest_pos(&self) -> Vector3<isize> {
		self.chest_pos
	}
	pub fn handle_mouse_moved(&mut self, pos :PhysicalPosition<f64>)  {
		self.last_mouse_pos = Some(pos);
	}
	pub fn handle_mouse_input(&mut self, state :ElementState, button :MouseButton) {
		self.mouse_input_ev = Some((state, button));
	}
	pub fn render<'a, 'b>(&mut self,
			ui_colors :&UiColors, tid_cache :&TextureIdCache,
			display :&glium::Display, program :&glium::Program,
			glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {

		let screen_dims = display.get_framebuffer_dimensions();

		let unit = unit_from_screen_dims(screen_dims.0);

		const SLOT_COUNT_X :usize = 8;

		let slot_counts_x :&[usize] = &[
			SLOT_COUNT_X,
			SLOT_COUNT_X,
		];

		macro_rules! inv {
			($id:expr) => {
				LayoutNode::inv($id, self.invs[$id].stacks().len(),
					slot_counts_x[$id], unit)
			};
		}
		let mut layout = LayoutNode::from_kind(LayoutNodeKind::Container {
			horizontal : false,
			children : vec![
				inv!(CRAFTING_ID),
				LayoutNode::spacer(SPACER_ID, (0.1 * unit * 1.1, 0.1 * unit * 1.1)),
				inv!(CRAFTING_OUTPUT_ID),
			],
		});
		let mouse_pos = self.last_mouse_pos.map(|pos|(pos.x as f32, pos.y as f32));
		let hover_idx = render_inventories(&self.params,
			ui_colors, tid_cache, display, program, glyph_brush, target,
			&mut layout, slot_counts_x, &self.invs, mouse_pos,
			self.from_pos);

		// TODO this is hacky, we change state in RENDERING code!!
		self.hover_idx = hover_idx;
	}

	pub fn check_movement(&mut self) -> Option<SwapCommand> {
		let mut swap_command = None;
		let input_ev = self.mouse_input_ev.take();
		if let (Some((state, button)), Some(hv)) = (input_ev, self.hover_idx.take()) {
			if state == ElementState::Released {
				if let Some(from_pos) = self.from_pos {
					if button == MouseButton::Left {
						self.from_pos = None;
					}
					swap_command = Some((from_pos, hv, button));
				} else {
					self.from_pos = Some(hv);
				}
			}
		}

		if let Some((from_pos, to_pos, button)) = swap_command {
			let maybe_only_move = match button {
				MouseButton::Left => Some(false),
				MouseButton::Right => Some(true),
				_ => None,
			};
			if let Some(only_move) = maybe_only_move {
				inventory::merge_or_move(
					&mut self.invs,
					from_pos, to_pos, only_move);
				return Some(SwapCommand {
					from_pos,
					to_pos,
					only_move,
				});
			}
		}
		None
	}
}

fn render_inventories<'a, 'b>(
		params :&GameParamsHdl,
		ui_colors :&UiColors,
		tid_cache :&TextureIdCache,
		display :&glium::Display, program :&glium::Program,
		glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame,
		layout :&mut LayoutNode,
		slot_counts_x :&[usize],
		invs :&[SelectableInventory],
		mouse_pos :Option<(f32, f32)>,
		from_pos :Option<(usize, usize)>
		) -> Option<(usize, usize)> {
	let screen_dims = display.get_framebuffer_dimensions();

	let unit = unit_from_screen_dims(screen_dims.0);

	layout.layout();

	let width = layout.state.dimension_x.expect("width expected") + 0.1 * unit;
	let height = layout.state.dimension_y.expect("height expected") + 0.1 * unit;

	layout.state.offs_absolute_x = Some(0.0);
	layout.state.offs_absolute_y = Some(0.0);

	layout.layout();
	assert_eq!(layout.progress(), LayoutProgress::Finished);

	let mut vertices = Vec::new();

	// Background
	let dims = (width as i32, height as i32);
	let mesh_x = -(width / 2.0) as i32;
	let mesh_y = -(height / 2.0) as i32;
	vertices.extend_from_slice(&square_mesh_xy(mesh_x, mesh_y,
		dims, screen_dims, ui_colors.background_color));

	let mut hover_idx = None;

	let convert = |scalar, dim| (scalar * 2.0) as i32 - dim as i32;

	layout.for_each_offsets(&mut |inv_id :usize, offs :Option<_>| {
		if inv_id == SPACER_ID {
			return;
		}
		let offs = offs.unwrap();
		let slots_x = slot_counts_x[inv_id];
		vertices.extend_from_slice(&inventory_slots_mesh(
			&invs[inv_id],
			invs[inv_id].stacks().len(),
			slots_x,
			unit,
			offs,
			width,
			screen_dims,
			|i, mesh_x, mesh_y| { // texture_fn
				let dims = (unit as i32, unit as i32);
				let hovering = mouse_pos.map(|pos| {
						(mesh_x ..= (mesh_x + dims.0)).contains(&convert(pos.0, screen_dims.0)) &&
						(mesh_y ..= (mesh_y + dims.1)).contains(&-convert(pos.1, screen_dims.1))
					})
					.unwrap_or(false);
				if hovering {
					hover_idx = Some((inv_id, i));
				}
				if from_pos == Some((inv_id, i)) {
					ui_colors.selected_slot_color
				} else if hovering {
					ui_colors.hovered_slot_color
				} else {
					ui_colors.slot_color
				}
			},
			|line| { // mesh_y_fn
				(height / 2.0 - (unit * 1.1 * (line + 1) as f32)) as i32
			},
			|line| { // text_y_fn
				(screen_dims.1 as f32 - height / 2.0
					+ unit * 1.1 * line as f32 + unit * 0.1) * 0.5 + offs.1 * 0.5
			},
			glyph_brush,
			tid_cache,
			params,
		));
	});

	draw_ui_vertices(&vertices, display, program, target);
	glyph_brush.draw_queued(display, target);

	hover_idx
}

fn unit_from_screen_dims(screen_dim_x :u32) -> f32 {
	(screen_dim_x as f32 / 15.0 * 2.0).min(128.0)
}

fn inventory_slots_mesh<'a, 'b>(inv :&SelectableInventory,
		slot_count :usize,
		slot_count_x :usize,
		unit :f32,
		offsets :(f32, f32),
		ui_width :f32,
		screen_dims :(u32, u32),
		mut texture_fn :impl FnMut(usize, i32, i32) -> TextureId,
		mesh_y_fn :impl Fn(usize) -> i32,
		text_y_fn :impl Fn(usize) -> f32,
		glyph_brush :&mut GlyphBrush<'a, 'b>,
		tid_cache :&TextureIdCache,
		params :&GameParamsHdl) -> Vec<Vertex> {
	let mut vertices = Vec::new();
	for i in 0 .. slot_count {
		let col = i % slot_count_x;
		let line = i / slot_count_x;
		let dims = (unit as i32, unit as i32);
		let mesh_x = offsets.0 as i32 +
			(-ui_width / 2.0 + (unit * 1.1 * col as f32) + unit * 0.1) as i32;
		let mesh_y = -offsets.1 as i32 + mesh_y_fn(line);
		let tx = texture_fn(i, mesh_x, mesh_y);
		vertices.extend_from_slice(&square_mesh_xy(mesh_x, mesh_y,
			dims, screen_dims, tx));
		let content = inv.stacks().get(i)
			.and_then(|s| s.content());
		// First check if there is an icon for the block, if yes
		let mut icon_found = false;
		let icon = content.and_then(|c| tid_cache.get_inv_texture_id(&c.0));
		if let Some(icon) = icon {
			vertices.extend_from_slice(&square_mesh_xy(mesh_x, mesh_y,
				dims, screen_dims, icon));
			icon_found = true;
		} else if let Some(content) = content {
			if let Some(texture_ids) = tid_cache.get_bl_tex_ids(&content.0) {
				push_block_mesh_xy(&mut vertices, mesh_x, mesh_y,
					dims, screen_dims, texture_ids);
				icon_found = true;
			}
		}
		let content = inv.stacks()
			.get(i)
			.unwrap_or(&Stack::Empty);
		let text = if let Stack::Content { item, count } = content {
			if icon_found {
				format!("{}", count)
			} else {
				format!("{} ({})", params.block_display_name(*item), count)
			}
		} else {
			String::from("")
		};
		let text_x = (screen_dims.0 as f32 - ui_width / 2.0
			+ unit * 1.1 * col as f32 + unit * 0.1) * 0.5
			+ offsets.0 * 0.5;
		let section = Section {
			text : &text,
			bounds : (unit / 2.0, unit / 2.0),
			screen_position : (text_x, text_y_fn(line)),
			layout : Layout::default()
				.h_align(HorizontalAlign::Left),
			color : [0.9, 0.9, 0.9, 1.0],
			.. Section::default()
		};
		glyph_brush.queue(section);
	}
	vertices
}

pub fn render_inventory_hud<'a, 'b>(inv :&SelectableInventory,
		ui_colors :&UiColors, tid_cache :&TextureIdCache,
		display :&glium::Display, program :&glium::Program,
		glyph_brush :&mut GlyphBrush<'a, 'b>, gm_params :&GameParamsHdl,
		target :&mut glium::Frame) {

	let screen_dims = display.get_framebuffer_dimensions();

	let unit = unit_from_screen_dims(screen_dims.0);

	const SLOT_COUNT_F32 :f32 = HUD_SLOT_COUNT as f32;

	let hud_width = SLOT_COUNT_F32 * unit * 1.10 + 0.1 * unit;
	let hud_height = unit * 1.10;

	let mut vertices = Vec::new();

	// Background
	let dims = (hud_width as i32,
		hud_height as i32);
	let mesh_x = -(hud_width / 2.0) as i32;
	let mesh_y = -(screen_dims.1 as i32) + (hud_height * 0.10) as i32;
	vertices.extend_from_slice(&square_mesh_xy(mesh_x, mesh_y,
		dims, screen_dims, ui_colors.background_color));

	// Item slots
	vertices.extend_from_slice(&inventory_slots_mesh(
		inv,
		HUD_SLOT_COUNT,
		HUD_SLOT_COUNT,
		unit,
		(0.0, screen_dims.1 as f32),
		hud_width,
		screen_dims,
		|i, _mesh_x, _mesh_y| { // texture_fn
			if Some(i) == inv.selection() {
				ui_colors.selected_slot_color
			} else {
				ui_colors.slot_color
			}
		},
		|_line| { // mesh_y_fn
			(hud_height * 0.10) as i32
		},
		|_line| { // text_y_fn
			screen_dims.1 as f32 - hud_height * 0.5
		},
		glyph_brush,
		tid_cache,
		&gm_params,
	));

	draw_ui_vertices(&vertices, display, program, target);
	glyph_brush.draw_queued(display, target);
}

fn draw_ui_vertices<'a, 'b>(vertices :&[Vertex],
		display :&glium::Display, program :&glium::Program,
		target :&mut glium::Frame) {
	let uniforms = uniform! {
		vmatrix : IDENTITY,
		pmatrix : IDENTITY,
		fog_near_far : [40.0f32, 60.0]
	};
	let params = glium::draw_parameters::DrawParameters {
		/*depth : glium::Depth {
			test : glium::draw_parameters::DepthTest::IfLess,
			write : true,
			.. Default::default()
		},
		backface_culling : glium::draw_parameters::BackfaceCullingMode::CullCounterClockwise,*/
		blend :glium::Blend::alpha_blending(),
		//polygon_mode : glium::draw_parameters::PolygonMode::Line,
		.. Default::default()
	};

	let vbuff = VertexBuffer::new(display, &vertices).unwrap();
	target.draw(&vbuff,
			&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
			&program, &uniforms, &params).unwrap();
}

pub fn square_mesh(mesh_dims :(i32, i32), framebuffer_dims :(u32, u32), tx :TextureId) -> Vec<Vertex> {
	let size_x = (mesh_dims.0 as f32) / (framebuffer_dims.0 as f32);
	let size_y = (mesh_dims.1 as f32) / (framebuffer_dims.1 as f32);

	let x_min = -size_x;
	let y_min = -size_y;
	let x_max = size_x;
	let y_max = size_y;

	square_mesh_frac_limits(x_min, y_min, x_max, y_max, tx)
}

pub fn square_mesh_xy(mesh_x :i32, mesh_y :i32,
		mesh_dims :(i32, i32), framebuffer_dims :(u32, u32),
		tx :TextureId) -> Vec<Vertex> {
	let mesh_x = (mesh_x as f32) / (framebuffer_dims.0 as f32);
	let mesh_y = (mesh_y as f32) / (framebuffer_dims.1 as f32);

	let size_x = (mesh_dims.0 as f32) / (framebuffer_dims.0 as f32);
	let size_y = (mesh_dims.1 as f32) / (framebuffer_dims.1 as f32);

	let x_min = mesh_x;
	let y_min = mesh_y;
	let x_max = mesh_x + size_x;
	let y_max = mesh_y + size_y;

	square_mesh_frac_limits(x_min, y_min, x_max, y_max, tx)
}

/// Creates a square mesh from limits given in fractions of screen size
pub fn square_mesh_frac_limits(
		x_min :f32, y_min :f32, x_max :f32, y_max :f32,
		tx :TextureId) -> Vec<Vertex> {
	let mut vertices = Vec::new();

	let z = 0.2;
	let tex_ind = tx.0;

	vertices.push(Vertex {
		position : [x_min, y_min, z],
		tex_pos : [0.0, 0.0],
		tex_ind,
		normal : [0.0, 1.0, 0.0],
	});
	vertices.push(Vertex {
		position : [x_max, y_min, z],
		tex_pos : [1.0, 0.0],
		tex_ind,
		normal : [0.0, 1.0, 0.0],
	});
	vertices.push(Vertex {
		position : [x_max, y_max, z],
		tex_pos : [1.0, 1.0],
		tex_ind,
		normal : [0.0, 1.0, 0.0],
	});

	vertices.push(Vertex {
		position : [x_max, y_max, z],
		tex_pos : [1.0, 1.0],
		tex_ind,
		normal : [0.0, 1.0, 0.0],
	});
	vertices.push(Vertex {
		position : [x_min, y_max, z],
		tex_pos : [0.0, 1.0],
		tex_ind,
		normal : [0.0, 1.0, 0.0],
	});
	vertices.push(Vertex {
		position : [x_min, y_min, z],
		tex_pos : [0.0, 0.0],
		tex_ind,
		normal : [0.0, 1.0, 0.0],
	});
	vertices
}

fn push_block_mesh_xy(vertices :&mut Vec<Vertex>, mesh_x :i32, mesh_y :i32,
		mesh_dims :(i32, i32), framebuffer_dims :(u32, u32),
		texture_ids :mimas_meshgen::BlockTextureIds) {
	use nalgebra::{Translation3, Point3, Isometry3, Orthographic3};

	let offs_x = mesh_dims.0 as f32 * 0.5;
	let offs_y = mesh_dims.1 as f32 * 0.5;
	let mesh_x = (mesh_x as f32 + offs_x) / (framebuffer_dims.0 as f32);
	let mesh_y = (mesh_y as f32 + offs_y) / (framebuffer_dims.1 as f32);

	let mut vertices_to_rotate :Vec<Vertex> = vec![];
	mimas_meshgen::push_block(&mut vertices_to_rotate,
		[1.0, 1.0, 1.0],
		texture_ids, 1.0, |_| false);
	let m = Isometry3::look_at_rh(&(Point3::origin()),
		&(Point3::origin() + Vector3::x() + Vector3::y() + Vector3::z()), &Vector3::z());

	let translation = Translation3::new(mesh_x, mesh_y, 0.0);

	let perspective = {
		let sc = 13.0;
		let left = -sc;
		let right = sc;
		let bottom = -sc;
		let top = sc;
		let znear = 0.5;
		let zfar = 200.0;
		Orthographic3::new(left, right, bottom, top, znear, zfar)
	};

	vertices_to_rotate.iter_mut().for_each(|v| {
		let p :Point3<f32> = v.position.into();
		let p = m * p;
		let p = perspective.project_point(&p);
		let p = translation * p;
		v.position = [p.x, p.y, p.z];
		// TODO also change the normal
	});
	vertices.extend_from_slice(&vertices_to_rotate);
}
