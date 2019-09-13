use mehlon_server::StrErr;

use glium::texture::texture2d_array::Texture2dArray;
use glium::texture::RawImage2d;
use glium::backend::Facade;

use mehlon_meshgen::TextureId;

pub struct Assets {
	assets :Vec<[f32; 4]>,
}

impl Assets {
	pub fn new() -> Self {
		Self {
			assets : Vec::new(),
		}
	}
	pub fn add_texture(&mut self, color :[f32; 4]) -> TextureId {
		let id = self.assets.len();
		self.assets.push(color);
		TextureId(id as u16)
	}
	pub fn into_texture_array<F: Facade>(self,
			facade :&F) -> Result<Texture2dArray, StrErr> {
		let imgs = self.assets.iter()
			.map(|col| {
				let pixels = col.iter()
					.chain(col.iter())
					.chain(col.iter())
					.chain(col.iter())
					.map(|v| *v)
					.collect::<Vec<_>>();
				RawImage2d::from_raw_rgba(pixels, (2, 2))
			})
			.collect::<Vec<_>>();
		let res = Texture2dArray::new(facade, imgs)?;
		Ok(res)
	}
}

pub struct UiColors {
	pub background_color :TextureId,
	pub slot_color :TextureId,
	pub selected_slot_color :TextureId,
	pub hovered_slot_color :TextureId,
	pub block_selection_color :TextureId,
	pub crosshair_color :TextureId,
	pub color_body :TextureId,
	pub color_head :TextureId,
}

impl UiColors {
	pub fn new(assets :&mut Assets) -> Self {
		Self {
			background_color : assets.add_texture([0.4, 0.4, 0.4, 0.85]),
			slot_color : assets.add_texture([0.5, 0.5, 0.5, 0.85]),
			selected_slot_color : assets.add_texture([0.3, 0.3, 0.3, 0.85]),
			hovered_slot_color : assets.add_texture([0.8, 0.8, 0.8, 0.85]),
			block_selection_color : assets.add_texture([0.0, 0.0, 0.3, 0.5]),
			crosshair_color : assets.add_texture([0.8, 0.8, 0.8, 0.85]),
			color_body : assets.add_texture([0.3, 0.3, 0.5, 1.0]),
			color_head : assets.add_texture([0.94, 0.76, 0.49, 1.0]),
		}
	}
}
