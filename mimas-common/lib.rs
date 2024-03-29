#![forbid(unsafe_code)]

extern crate anyhow;
extern crate noise;
extern crate nalgebra;
extern crate rand_pcg;
extern crate rand;
#[macro_use]
extern crate serde_derive;
extern crate serde_big_array;
extern crate bincode;
extern crate twox_hash;
extern crate toml;
extern crate rustls;
extern crate argon2;

extern crate webpki;
extern crate quinn;
extern crate tokio;
extern crate futures;
extern crate rcgen;
extern crate sha2;

extern crate rusqlite;
extern crate libsqlite3_sys;
extern crate byteorder;
extern crate flate2;
extern crate base64;

pub mod map;
pub mod map_storage;
pub mod generic_net;
pub mod quic_net;
pub mod config;
pub mod sqlite_generic;
pub mod local_auth;
pub mod inventory;
pub mod crafting;
pub mod game_params;
pub mod toml_util;
pub mod protocol;
pub mod player;
pub mod schematic;

pub use protocol::{ClientToServerMsg, ServerToClientMsg};
use map::CHUNKSIZE;
use nalgebra::Vector3;

/// Block position to position inside chunk
pub fn btpic(v :Vector3<isize>) -> Vector3<isize> {
	v.map(|v| (v as f32).rem_euclid(CHUNKSIZE as f32) as isize)
}

/// Block position to chunk position
pub fn btchn(v :Vector3<isize>) -> Vector3<isize> {
	fn r(x :isize) -> isize {
		let x = x as f32 / (CHUNKSIZE as f32);
		x.floor() as isize * CHUNKSIZE
	}
	v.map(r)
}
