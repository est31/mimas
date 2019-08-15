use glium::{Surface, VertexBuffer};
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	Section, Layout, HorizontalAlign,
};
use glium::glutin::{KeyboardInput, VirtualKeyCode, ElementState,
	MouseButton, dpi::LogicalPosition};
use glium_glyph::glyph_brush::GlyphCruncher;
use mehlon_server::inventory::{SelectableInventory, Stack,
	HUD_SLOT_COUNT};
use mehlon_server::crafting::get_matching_recipe;
use mehlon_server::game_params::GameParamsHdl;

use mehlon_meshgen::Vertex;

pub const IDENTITY :[[f32; 4]; 4] = [
	[1.0, 0.0, 0.0, 0.0f32],
	[0.0, 1.0, 0.0, 0.0],
	[0.0, 0.0, 1.0, 0.0],
	[0.0, 0.0, 0.0, 1.0],
];

fn render_text<'a, 'b>(text :&str, display :&glium::Display, program :&glium::Program, glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {
	let screen_dims = display.get_framebuffer_dimensions();

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
	let vertices = square_mesh(dims, screen_dims, BACKGROUND_COLOR);
	let vbuff = VertexBuffer::new(display, &vertices).unwrap();
	target.draw(&vbuff,
			&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
			&program, &uniforms, &params).unwrap();
	glyph_brush.queue(section);
	glyph_brush.draw_queued(display, target);
}

pub fn render_menu<'a, 'b>(display :&glium::Display, program :&glium::Program, glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {
	render_text("Menu\nPress esc to continue Game", display, program, glyph_brush, target);
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
	pub fn render<'a, 'b>(&self, display :&glium::Display, program :&glium::Program,
			glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {
		let text = "Type to chat\n".to_owned() + &self.text;
		render_text(&text, display, program, glyph_brush, target);
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

pub struct InventoryMenu {
	params :GameParamsHdl,
	inv :SelectableInventory,
	craft_inv :SelectableInventory,
	last_mouse_pos :Option<LogicalPosition>,
	mouse_input_ev :Option<(ElementState, MouseButton)>,
	from_pos : Option<(usize, usize)>,
}

impl InventoryMenu {
	pub fn new(params :GameParamsHdl,
			inv :SelectableInventory,
			craft_inv :SelectableInventory) -> Self {
		Self {
			params,
			inv,
			craft_inv,
			last_mouse_pos : None,
			mouse_input_ev : None,
			from_pos : None,
		}
	}
	pub fn inventory(&self) -> &SelectableInventory {
		&self.inv
	}
	pub fn craft_inv(&self) -> &SelectableInventory {
		&self.craft_inv
	}
	pub fn handle_mouse_moved(&mut self, pos :LogicalPosition)  {
		self.last_mouse_pos = Some(pos);
	}
	pub fn handle_mouse_input(&mut self, state :ElementState, button :MouseButton) {
		self.mouse_input_ev = Some((state, button));
	}
	fn craft_output_inv(&self) -> SelectableInventory {
		let recipe = get_matching_recipe(&self.craft_inv, &self.params);
		let stack = recipe
			.map(|r| Stack::with(r.output.0, r.output.1))
			.unwrap_or(Stack::Empty);
		let stacks = vec![stack].into_boxed_slice();
		SelectableInventory::from_stacks(stacks)
	}
	pub fn render<'a, 'b>(&mut self,
			display :&glium::Display, program :&glium::Program,
			glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {

		let screen_dims = display.get_framebuffer_dimensions();

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
		//let mut mesh_dims = glyph_brush.pixel_bounds(&section).unwrap();
		//mesh_dims.min.x = mesh_dims.min.y.min(section.screen_position.0 as i32);
		//mesh_dims.min.y = mesh_dims.min.y.min(section.screen_position.1 as i32);

		let unit = unit_from_screen_dims(screen_dims.0);

		const SLOT_COUNT_X :usize = 8;
		const SLOT_COUNT_X_F32 :f32 = SLOT_COUNT_X as f32;

		const CRAFT_SLOT_COUNT_X :usize = 3;
		const CRAFT_SLOT_COUNT_X_F32 :f32 = CRAFT_SLOT_COUNT_X as f32;

		let width = SLOT_COUNT_X_F32 * unit * 1.10 + 0.1 * unit;
		let inv_height_units = (self.craft_inv.stacks().len() as f32 / SLOT_COUNT_X_F32).ceil();
		let craft_height_units = (self.craft_inv.stacks().len() as f32 / CRAFT_SLOT_COUNT_X_F32).ceil();
		let height_units = inv_height_units + craft_height_units;
		let height = height_units * unit * 1.1 + 0.1 * unit;

		let mut vertices = Vec::new();

		// Background
		let dims = (width as i32, height as i32);
		let mesh_x = -(width / 2.0) as i32;
		let mesh_y = -(height / 2.0) as i32;
		vertices.extend_from_slice(&square_mesh_xy(mesh_x, mesh_y,
			dims, screen_dims, BACKGROUND_COLOR));

		const SLOT_COLOR :[f32; 4] = [0.5, 0.5, 0.5, 0.85];
		const SELECTED_SLOT_COLOR :[f32; 4] = [0.3, 0.3, 0.3, 0.85];
		const HOVERED_SLOT_COLOR :[f32; 4] = [0.8, 0.8, 0.8, 0.85];

		let mut hover_idx = None;

		let convert = |scalar, dim| (scalar * 2.0) as i32 - dim as i32;

		const CRAFTING_ID :usize = 0;
		const CRAFTING_OUTPUT_ID :usize = 1;
		const NORMAL_INV_ID :usize = 2;

		// Crafting inventory slots
		vertices.extend_from_slice(&inventory_slots_mesh(
			&self.craft_inv,
			self.craft_inv.stacks().len(),
			CRAFT_SLOT_COUNT_X,
			unit,
			(0, 0),
			width,
			screen_dims,
			|i, mesh_x, mesh_y| { // color_fn
				let dims = (unit as i32, unit as i32);
				let hovering = self.last_mouse_pos
					.map(|pos| {
						(mesh_x ..= (mesh_x + dims.0)).contains(&convert(pos.x, screen_dims.0)) &&
						(mesh_y ..= (mesh_y + dims.1)).contains(&-convert(pos.y, screen_dims.1))
					})
					.unwrap_or(false);
				if hovering {
					hover_idx = Some((CRAFTING_ID, i));
				}
				if self.from_pos == Some((CRAFTING_ID, i)) {
					SELECTED_SLOT_COLOR
				} else if hovering {
					HOVERED_SLOT_COLOR
				} else {
					SLOT_COLOR
				}
			},
			|line| { // mesh_y_fn
				(height / 2.0 - (unit * 1.1 * (line + 1) as f32)) as i32
			},
			|line| { // text_y_fn
				(screen_dims.1 as f32 - height / 2.0
					+ unit * 1.1 * line as f32 + unit * 0.1) * 0.5
			},
			glyph_brush,
			&self.params,
		));

		let mut craft_output_inv = self.craft_output_inv();
		vertices.extend_from_slice(&inventory_slots_mesh(
			&craft_output_inv,
			craft_output_inv.stacks().len(),
			CRAFT_SLOT_COUNT_X,
			unit,
			((width / 2.0) as i32, 0),
			width,
			screen_dims,
			|i, mesh_x, mesh_y| { // color_fn
				let dims = (unit as i32, unit as i32);
				let hovering = self.last_mouse_pos
					.map(|pos| {
						(mesh_x ..= (mesh_x + dims.0)).contains(&convert(pos.x, screen_dims.0)) &&
						(mesh_y ..= (mesh_y + dims.1)).contains(&-convert(pos.y, screen_dims.1))
					})
					.unwrap_or(false);
				if hovering {
					hover_idx = Some((CRAFTING_OUTPUT_ID, i));
				}
				if self.from_pos == Some((CRAFTING_OUTPUT_ID, i)) {
					SELECTED_SLOT_COLOR
				} else if hovering {
					HOVERED_SLOT_COLOR
				} else {
					SLOT_COLOR
				}
			},
			|line| { // mesh_y_fn
				(height / 2.0 - (unit * 1.1 * (line + 1) as f32)) as i32
			},
			|line| { // text_y_fn
				(screen_dims.1 as f32 - height / 2.0
					+ unit * 1.1 * line as f32 + unit * 0.1) * 0.5
			},
			glyph_brush,
			&self.params,
		));

		// Inventory slots
		vertices.extend_from_slice(&inventory_slots_mesh(
			&self.inv,
			self.inv.stacks().len(),
			SLOT_COUNT_X,
			unit,
			(0, -(craft_height_units * 1.1 * unit) as i32),
			width,
			screen_dims,
			|i, mesh_x, mesh_y| { // color_fn
				let dims = (unit as i32, unit as i32);
				let hovering = self.last_mouse_pos
					.map(|pos| {
						(mesh_x ..= (mesh_x + dims.0)).contains(&convert(pos.x, screen_dims.0)) &&
						(mesh_y ..= (mesh_y + dims.1)).contains(&-convert(pos.y, screen_dims.1))
					})
					.unwrap_or(false);
				if hovering {
					hover_idx = Some((NORMAL_INV_ID, i));
				}
				if self.from_pos == Some((NORMAL_INV_ID, i)) {
					SELECTED_SLOT_COLOR
				} else if hovering {
					HOVERED_SLOT_COLOR
				} else {
					SLOT_COLOR
				}
			},
			|line| { // mesh_y_fn
				(height / 2.0 - (unit * 1.1 * (line + 1) as f32)) as i32
			},
			|line| { // text_y_fn
				(craft_height_units * 1.1 * unit) / 2.0 +
				(screen_dims.1 as f32 - height / 2.0
					+ unit * 1.1 * line as f32 + unit * 0.1) * 0.5
			},
			glyph_brush,
			&self.params,
		));

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
						// TODO figure out something for the remainder stack
						self.inv.put(craft_output_inv.stacks()[0]);
						// Reduce inputs.
						for st in self.craft_inv.stacks_mut().iter_mut() {
							st.take_n(1);
						}
					} else {
						self.from_pos = Some(hv);
					}
				}
			}
		}

		// TODO this is hacky, we change state in RENDERING code!!
		if let Some((from_pos, to_pos, button)) = swap_command {
			let invs = &mut [&mut self.craft_inv, &mut craft_output_inv,
				&mut self.inv];
			if to_pos.0 == CRAFTING_OUTPUT_ID {
				// Putting into the crafting menu is not possible
			} else {
				if button == MouseButton::Left {
					SelectableInventory::merge_or_swap(
						invs,
						from_pos, to_pos);
				}
				if button == MouseButton::Right {
					SelectableInventory::move_n_if_possible(
						invs,
						from_pos, to_pos, 1);
				}
			}
		}

		let vbuff = VertexBuffer::new(display, &vertices).unwrap();
		target.draw(&vbuff,
				&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
				&program, &uniforms, &params).unwrap();
		glyph_brush.draw_queued(display, target);
	}
}

fn unit_from_screen_dims(screen_dim_x :u32) -> f32 {
	(screen_dim_x as f32 / 15.0 * 2.0).min(128.0)
}

fn inventory_slots_mesh<'a, 'b>(inv :&SelectableInventory,
		slot_count :usize,
		slot_count_x :usize,
		unit :f32,
		offsets :(i32, i32),
		ui_width :f32,
		screen_dims :(u32, u32),
		mut color_fn :impl FnMut(usize, i32, i32) -> [f32; 4],
		mesh_y_fn :impl Fn(usize) -> i32,
		text_y_fn :impl Fn(usize) -> f32,
		glyph_brush :&mut GlyphBrush<'a, 'b>,
		params :&GameParamsHdl) -> Vec<Vertex> {
	let mut vertices = Vec::new();
	for i in 0 .. slot_count {
		let col = i % slot_count_x;
		let line = i / slot_count_x;
		let dims = (unit as i32, unit as i32);
		let mesh_x = offsets.0 +
			(-ui_width / 2.0 + (unit * 1.1 * col as f32) + unit * 0.1) as i32;
		let mesh_y = offsets.1 + mesh_y_fn(line);
		let color = color_fn(i, mesh_x, mesh_y);
		vertices.extend_from_slice(&square_mesh_xy(mesh_x, mesh_y,
			dims, screen_dims, color));
		let content = inv.stacks()
			.get(i)
			.unwrap_or(&Stack::Empty);
		let text = if let Stack::Content { item, count } = content {
			format!("{} ({})", params.block_display_name(*item), count)
		} else {
			String::from("")
		};
		let text_x = (screen_dims.0 as f32 - ui_width / 2.0
			+ unit * 1.1 * col as f32 + unit * 0.1) * 0.5
			+ offsets.0 as f32 / 2.0;
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
		display :&glium::Display, program :&glium::Program,
		glyph_brush :&mut GlyphBrush<'a, 'b>, gm_params :&GameParamsHdl,
		target :&mut glium::Frame) {

	let screen_dims = display.get_framebuffer_dimensions();

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
	//let mut mesh_dims = glyph_brush.pixel_bounds(&section).unwrap();
	//mesh_dims.min.x = mesh_dims.min.y.min(section.screen_position.0 as i32);
	//mesh_dims.min.y = mesh_dims.min.y.min(section.screen_position.1 as i32);

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
		dims, screen_dims, BACKGROUND_COLOR));

	const SLOT_COLOR :[f32; 4] = [0.5, 0.5, 0.5, 0.85];
	const SELECTED_SLOT_COLOR :[f32; 4] = [0.8, 0.8, 0.8, 0.85];

	// Item slots
	vertices.extend_from_slice(&inventory_slots_mesh(
		inv,
		HUD_SLOT_COUNT,
		HUD_SLOT_COUNT,
		unit,
		(0, -(screen_dims.1 as i32)),
		hud_width,
		screen_dims,
		|i, _mesh_x, _mesh_y| { // color_fn
			if Some(i) == inv.selection() {
				SELECTED_SLOT_COLOR
			} else {
				SLOT_COLOR
			}
		},
		|_line| { // mesh_y_fn
			(hud_height * 0.10) as i32
		},
		|_line| { // text_y_fn
			screen_dims.1 as f32 - hud_height * 0.5
		},
		glyph_brush,
		&gm_params,
	));

	let vbuff = VertexBuffer::new(display, &vertices).unwrap();
	target.draw(&vbuff,
			&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
			&program, &uniforms, &params).unwrap();
	glyph_brush.draw_queued(display, target);
}

const BACKGROUND_COLOR :[f32; 4] = [0.4, 0.4, 0.4, 0.85];

pub fn square_mesh(mesh_dims :(i32, i32), framebuffer_dims :(u32, u32), color :[f32; 4]) -> Vec<Vertex> {
	let size_x = (mesh_dims.0 as f32) / (framebuffer_dims.0 as f32);
	let size_y = (mesh_dims.1 as f32) / (framebuffer_dims.1 as f32);

	let x_min = -size_x;
	let y_min = -size_y;
	let x_max = size_x;
	let y_max = size_y;

	square_mesh_frac_limits(x_min, y_min, x_max, y_max, color)
}

pub fn square_mesh_xy(mesh_x :i32, mesh_y :i32,
		mesh_dims :(i32, i32), framebuffer_dims :(u32, u32),
		color :[f32; 4]) -> Vec<Vertex> {
	let mesh_x = (mesh_x as f32) / (framebuffer_dims.0 as f32);
	let mesh_y = (mesh_y as f32) / (framebuffer_dims.1 as f32);

	let size_x = (mesh_dims.0 as f32) / (framebuffer_dims.0 as f32);
	let size_y = (mesh_dims.1 as f32) / (framebuffer_dims.1 as f32);

	let x_min = mesh_x;
	let y_min = mesh_y;
	let x_max = mesh_x + size_x;
	let y_max = mesh_y + size_y;

	square_mesh_frac_limits(x_min, y_min, x_max, y_max, color)
}

/// Creates a square mesh from limits given in fractions of screen size
pub fn square_mesh_frac_limits(
		x_min :f32, y_min :f32, x_max :f32, y_max :f32,
		color :[f32; 4]) -> Vec<Vertex> {
	let mut vertices = Vec::new();

	let z = 0.2;

	vertices.push(Vertex {
		position : [x_min, y_min, z],
		color,
		normal :[0.0, 1.0, 0.0],
	});
	vertices.push(Vertex {
		position : [x_max, y_min, z],
		color,
		normal :[0.0, 1.0, 0.0],
	});
	vertices.push(Vertex {
		position : [x_max, y_max, z],
		color,
		normal :[0.0, 1.0, 0.0],
	});
	vertices.push(Vertex {
		position : [x_max, y_max, z],
		color,
		normal :[0.0, 1.0, 0.0],
	});
	vertices.push(Vertex {
		position : [x_min, y_max, z],
		color,
		normal :[0.0, 1.0, 0.0],
	});
	vertices.push(Vertex {
		position : [x_min, y_min, z],
		color,
		normal :[0.0, 1.0, 0.0],
	});
	vertices
}
