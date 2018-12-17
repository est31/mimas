use glium::{Surface, VertexBuffer};
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	Section, Layout, HorizontalAlign,
};
use glium_glyph::glyph_brush::GlyphCruncher;

use mehlon_meshgen::Vertex;

pub fn render_menu<'a, 'b>(display :&glium::Display, program :&glium::Program, glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {
	let screen_dims = display.get_framebuffer_dimensions();
	const IDENTITY :[[f32; 4]; 4] = [
		[1.0, 0.0, 0.0, 0.0f32],
		[0.0, 1.0, 0.0, 0.0],
		[0.0, 0.0, 1.0, 0.0],
		[0.0, 0.0, 0.0, 1.0],
	];
	let uniforms = uniform! {
		vmatrix : IDENTITY,
		pmatrix : IDENTITY
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
	let f = 1.0 / 2.0 ;//- 0.02;
	let mut section = Section {
		text : "Menu\nPress esc to continue Game",
		bounds : (screen_dims.0 as f32 * 0.14, screen_dims.1 as f32),
		screen_position : (screen_dims.0 as f32 * f, screen_dims.1 as f32 * f),
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
	let dims = (mesh_dims.width(), mesh_dims.height());
	let vertices = square_mesh(dims, screen_dims, BACKGROUND_COLOR);
	let vbuff = VertexBuffer::new(display, &vertices).unwrap();
	target.draw(&vbuff,
			&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
			&program, &uniforms, &params).unwrap();
	glyph_brush.queue(section);
	glyph_brush.draw_queued(display, target);
}

const BACKGROUND_COLOR :[f32; 4] = [0.4, 0.4, 0.4, 0.85];

pub fn square_mesh(mesh_dims :(i32, i32), framebuffer_dims :(u32, u32), color :[f32; 4]) -> Vec<Vertex> {
	let mut vertices = Vec::new();

	let size_x = (mesh_dims.0 as f32) / (framebuffer_dims.0 as f32);
	let size_y = (mesh_dims.1 as f32) / (framebuffer_dims.1 as f32);

	let x_min = -size_x;
	let y_min = -size_y;
	let x_max = size_x;
	let y_max = size_y;
	let z = 0.2;

	vertices.push(Vertex {
		position : [x_min, y_min, z],
		color,
	});
	vertices.push(Vertex {
		position : [x_max, y_min, z],
		color,
	});
	vertices.push(Vertex {
		position : [x_max, y_max, z],
		color,
	});
	vertices.push(Vertex {
		position : [x_max, y_max, z],
		color,
	});
	vertices.push(Vertex {
		position : [x_min, y_max, z],
		color,
	});
	vertices.push(Vertex {
		position : [x_min, y_min, z],
		color,
	});
	vertices
}
