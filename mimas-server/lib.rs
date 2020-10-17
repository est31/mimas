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

mod game_params;
pub mod server;

pub use server::Server;
