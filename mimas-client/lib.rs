#![forbid(unsafe_code)]

extern crate anyhow;
extern crate nalgebra;
#[macro_use]
extern crate glium;
extern crate glium_glyph;
extern crate num_traits;
extern crate frustum_query;
extern crate rand_pcg;
extern crate rand;
extern crate srp;
extern crate sha2;
extern crate image;
extern crate dirs;

extern crate mimas_server;
extern crate mimas_meshgen;

mod assets;
pub mod client;
mod collide;
mod ui;
mod voxel_walk;

use glium::glutin;
