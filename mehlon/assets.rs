use mehlon_server::StrErr;
use mehlon_server::game_params::DrawStyle;

use glium::texture::SrgbTexture2dArray;
use glium::texture::RawImage2d;
use glium::backend::Facade;
use image::{RgbaImage, Pixel};

use mehlon_meshgen::{TextureId, BlockTextureIds};

pub struct Assets {
	assets :Vec<(Vec<f32>, (u32, u32))>,
}

fn load_image_inner(path :&str) -> Result<RgbaImage, StrErr> {
	let img = image::open(path)?;
	Ok(img.to_rgba())
}

fn load_image(path :&str) -> Result<(Vec<f32>, (u32, u32)), StrErr> {
	let mut imgs_iter = path.split("^")
		.map(|p| load_image_inner(p));
	let mut image = imgs_iter.next().ok_or("No image path specified")??;
	let imgs = imgs_iter.collect::<Result<Vec<_>, StrErr>>()?;
	for overlay in imgs.iter() {
		image.pixels_mut()
			.zip(overlay.pixels())
			.for_each(|(img_pixel, overlay_pixel)| {
				img_pixel.blend(overlay_pixel);
			});
	}
	// Make all textures non-opaque
	// as currently, mesh generation can't deal
	// with opaque textures.
	image.pixels_mut()
		.for_each(|px| px.0[3] = 255);
	let dimensions = image.dimensions();
	let buf = image.into_raw()
		.into_iter()
		.map(|v| {
			v as f32 / u8::max_value() as f32
		})
		.collect::<Vec<_>>();
	Ok((buf, dimensions))
}

impl Assets {
	pub fn new() -> Self {
		Self {
			assets : Vec::new(),
		}
	}
	fn add_asset(&mut self, asset :(Vec<f32>, (u32, u32))) -> TextureId {
		let id = self.assets.len();
		self.assets.push(asset);
		TextureId(id as u16)
	}
	pub fn add_draw_style(&mut self, ds :&DrawStyle) -> BlockTextureIds {
		match ds {
			DrawStyle::Colored(color) => {
				let id = self.add_color(*color);
				let id_h = self.add_color(mehlon_meshgen::colorh(*color));
				BlockTextureIds::new(id, id_h)
			},
			DrawStyle::Texture(path) => {
				let asset = load_image(path).expect("couldn't load image");
				let id = self.add_asset(asset);
				BlockTextureIds::uniform(id)
			},
			DrawStyle::TextureSidesTop(path_s, path_tb) => {
				let image_s = load_image(path_s).expect("couldn't load image");
				let image_tb = load_image(path_tb).expect("couldn't load image");
				let id_s = self.add_asset(image_s);
				let id_tb = self.add_asset(image_tb);
				BlockTextureIds::new(id_tb, id_s)
			},
		}
	}
	pub fn add_color(&mut self, color :[f32; 4]) -> TextureId {
		let pixels = std::iter::repeat(color.iter())
			.take(256)
			.flatten()
			.map(|v| *v)
			.collect::<Vec<_>>();
		self.add_asset((pixels, (16, 16)))
	}
	pub fn into_texture_array<F: Facade>(self,
			facade :&F) -> Result<SrgbTexture2dArray, StrErr> {
		let imgs = self.assets.into_iter()
			.map(|(pixels, dimensions)| {
				RawImage2d::from_raw_rgba_reversed(&pixels, dimensions)
			})
			.collect::<Vec<_>>();
		let res = SrgbTexture2dArray::new(facade, imgs)?;
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
			background_color : assets.add_color([0.4, 0.4, 0.4, 0.85]),
			slot_color : assets.add_color([0.5, 0.5, 0.5, 0.85]),
			selected_slot_color : assets.add_color([0.3, 0.3, 0.3, 0.85]),
			hovered_slot_color : assets.add_color([0.8, 0.8, 0.8, 0.85]),
			block_selection_color : assets.add_color([0.0, 0.0, 0.3, 0.5]),
			crosshair_color : assets.add_color([0.8, 0.8, 0.8, 0.85]),
			color_body : assets.add_color([0.3, 0.3, 0.5, 1.0]),
			color_head : assets.add_color([0.94, 0.76, 0.49, 1.0]),
		}
	}
}
