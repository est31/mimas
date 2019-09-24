use mehlon_server::StrErr;
use mehlon_server::game_params::{GameParamsHdl, DrawStyle};

use std::fs::File;
use std::io::Read;

use glium::texture::SrgbTexture2dArray;
use glium::texture::RawImage2d;
use glium::backend::Facade;
use image::{RgbaImage, Pixel};

use sha2::{Sha256, Digest};

use mehlon_meshgen::{TextureId, BlockTextureIds};

pub struct Assets {
	assets :Vec<(Vec<f32>, (u32, u32))>,
}

fn load_image_inner(game_params :&GameParamsHdl, path :&str) -> Result<RgbaImage, StrErr> {
	let mut file = File::open(&path)?;
	let mut buf = Vec::new();
	file.read_to_end(&mut buf)?;

	let found_hash = {
		let mut hasher = Sha256::new();
		hasher.input(&buf);
		hasher.result().as_slice().to_owned()
	};
	let expected_hash = game_params.texture_hashes.get(&path.to_owned());
	if let Some(expected_hash) = expected_hash {
		if found_hash.as_slice() != expected_hash.as_slice() {
			Err(format!("Hash mismatch for texture {}", path))?
		}
	} else {
		Err(format!("Couldn't find hash for texture {}", path))?
	}

	let img = image::load_from_memory(&buf)?;
	Ok(img.to_rgba())
}

fn load_image(game_params :&GameParamsHdl, path :&str) -> Result<(Vec<f32>, (u32, u32)), StrErr> {
	let mut imgs_iter = path.split("^")
		.map(|p| load_image_inner(game_params, p));
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
	pub fn add_draw_style(&mut self, game_params :&GameParamsHdl,
			ds :&DrawStyle) -> BlockTextureIds {
		match ds {
			DrawStyle::Colored(color) => {
				let id = self.add_color(*color);
				let id_h = self.add_color(mehlon_meshgen::colorh(*color));
				BlockTextureIds::new_tb(id, id_h)
			},
			DrawStyle::Texture(path) => {
				let asset = load_image(game_params, path)
					.expect("couldn't load image");
				let id = self.add_asset(asset);
				BlockTextureIds::uniform(id)
			},
			DrawStyle::TextureSidesTop(path_s, path_tb) => {
				let image_s = load_image(game_params, path_s)
					.expect("couldn't load image");
				let image_tb = load_image(game_params, path_tb)
					.expect("couldn't load image");
				let id_s = self.add_asset(image_s);
				let id_tb = self.add_asset(image_tb);
				BlockTextureIds::new_tb(id_tb, id_s)
			},
			DrawStyle::TextureSidesTopBottom(path_s, path_t, path_b) => {
				let image_s = load_image(game_params, path_s)
					.expect("couldn't load image");
				let image_t = load_image(game_params, path_t)
					.expect("couldn't load image");
				let image_b = load_image(game_params, path_b)
					.expect("couldn't load image");
				let id_s = self.add_asset(image_s);
				let id_t = self.add_asset(image_t);
				let id_b = self.add_asset(image_b);
				BlockTextureIds::new(id_s, id_t, id_b)
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
