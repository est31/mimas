use glium::{Surface, VertexBuffer};
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	Section, Layout, HorizontalAlign,
};

use mehlon_meshgen::Vertex;

pub fn render_menu<'a, 'b>(display :&glium::Display,program :&glium::Program, glyph_brush :&mut GlyphBrush<'a, 'b>, target :&mut glium::Frame) {
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
	let vertices = square_mesh();
	let vbuff = VertexBuffer::new(display, &vertices).unwrap();
	target.draw(&vbuff,
			&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
			&program, &uniforms, &params).unwrap();
	let f = 1.0 / 2.0 - 0.02;
	glyph_brush.queue(Section {
		text : "Menu\nPress esc to continue Game",
		bounds : (screen_dims.0 as f32 * 0.14, screen_dims.1 as f32),
		screen_position : (screen_dims.0 as f32 * f, screen_dims.1 as f32 * f),
		layout : Layout::default()
			.h_align(HorizontalAlign::Center),
		color : [0.9, 0.9, 0.9, 1.0],
		.. Section::default()
	});
	glyph_brush.draw_queued(display, target);
}

fn square_mesh() -> Vec<Vertex> {
	const COLOR :[f32; 4] = [0.4, 0.4, 0.4, 0.85];
	let mut vertices = Vec::new();

	let size = 0.15;
	let x_min = -size;
	let y_min = -size;
	let x_max = size;
	let y_max = size;
	let z = 0.2;

	vertices.push(Vertex {
		position : [x_min, x_min, z],
		color : COLOR,
	});
	vertices.push(Vertex {
		position : [x_max, y_min, z],
		color : COLOR,
	});
	vertices.push(Vertex {
		position : [x_max, y_max, z],
		color : COLOR,
	});
	vertices.push(Vertex {
		position : [x_max, y_max, z],
		color : COLOR,
	});
	vertices.push(Vertex {
		position : [x_min, y_max, z],
		color : COLOR,
	});
	vertices.push(Vertex {
		position : [x_min, x_min, z],
		color : COLOR,
	});
	vertices
}
