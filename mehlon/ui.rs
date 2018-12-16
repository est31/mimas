use glium::{Surface, VertexBuffer};
use glium_glyph::GlyphBrush;
use glium_glyph::glyph_brush::{
	Section, Layout, HorizontalAlign,
	SectionText, SectionGeometry,
	FontMap, FontId,
};
use glium_glyph::glyph_brush::rusttype as rt;
use glium_glyph::glyph_brush::GlyphPositioner;

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
	let f = 1.0 / 2.0 - 0.02;
	let section = Section {
		text : "Menu\nPress esc to continue Game",
		bounds : (screen_dims.0 as f32 * 0.14, screen_dims.1 as f32),
		screen_position : (screen_dims.0 as f32 * f, screen_dims.1 as f32 * f),
		layout : Layout::default()
			.h_align(HorizontalAlign::Center),
		color : [0.9, 0.9, 0.9, 1.0],
		.. Section::default()
	};
	let mesh_dims = get_section_bounding_box(&section, &glyph_brush).unwrap();
	let dims = (mesh_dims.width(), mesh_dims.height());
	let vertices = square_mesh(dims, screen_dims);
	let vbuff = VertexBuffer::new(display, &vertices).unwrap();
	target.draw(&vbuff,
			&glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
			&program, &uniforms, &params).unwrap();
	glyph_brush.queue(section);
	glyph_brush.draw_queued(display, target);
}

fn get_section_bounding_box<'a, 'b>(section :&Section, glyph_brush :&GlyphBrush<'a, 'b>) -> Option<rt::Rect<i32>> {
	let geom = SectionGeometry {
		screen_position : section.screen_position,
		bounds : section.bounds,
	};
	let section_text = SectionText {
		text : section.text,
		scale : section.scale,
		color : section.color,
		font_id : section.font_id,
	};

	// https://github.com/alexheretic/glyph-brush/pull/46
	struct FontHack<'a>(&'a[rt::Font<'a>]);
	impl<'font> FontMap<'font> for FontHack<'font> {
		#[inline]
		fn font(&self, i :FontId) -> &rt::Font<'font> {
			&self.0[i.0]
		}
	}

	let fonts :&[rt::Font<'_>] = &glyph_brush.fonts();
	let boxes = section.layout.calculate_glyphs(&FontHack(fonts), &geom, &[section_text])
		.iter()
		.filter_map(|v| {
			v.0.pixel_bounding_box()
				.map(|mut b| {
					let p = v.0.position();
					b.min.x += p.x as i32;
					b.min.y += p.y as i32;
					b.max.x += p.x as i32;
					b.max.y += p.y as i32;
					b
				})
		})
		.collect::<Vec<_>>();
	let min_x = boxes.iter().map(|v|v.min.x).min()?;
	let min_y = boxes.iter().map(|v|v.min.y).min()?;
	let max_x = boxes.iter().map(|v|v.max.x).max()?;
	let max_y = boxes.iter().map(|v|v.max.y).max()?;
	Some(rt::Rect {
		min : rt::Point {
			x : min_x,
			y : min_y,
		},
		max : rt::Point {
			x : max_x,
			y : max_y,
		},
	})
}

fn square_mesh(mesh_dims :(i32, i32), framebuffer_dims :(u32, u32)) -> Vec<Vertex> {
	const COLOR :[f32; 4] = [0.4, 0.4, 0.4, 0.85];
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
		position : [x_min, y_min, z],
		color : COLOR,
	});
	vertices
}
