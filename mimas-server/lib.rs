#![forbid(unsafe_code)]

extern crate anyhow;
extern crate noise;
extern crate nalgebra;
extern crate rand_pcg;
extern crate rand;
#[macro_use]
extern crate serde_derive;
#[macro_use]
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
extern crate srp;
extern crate sha2;

extern crate rusqlite;
extern crate libsqlite3_sys;
extern crate byteorder;
extern crate flate2;
extern crate base64;

pub mod map;
pub mod mapgen;
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
pub mod server;

pub use server::{ClientToServerMsg, ServerToClientMsg, Server, btchn};
pub(crate) use server::btpic;

pub use anyhow::Error as StrErr;
/*
#[derive(Debug)]
pub struct StrErr(String);

impl<T :Display> From<T> for StrErr {
	fn from(v :T) -> Self {
		StrErr(format!("{}", v))
	}
}*/
