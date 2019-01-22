#![forbid(unsafe_code)]

extern crate nalgebra;
#[macro_use]
extern crate glium;
extern crate winit;
extern crate glium_glyph;
extern crate num_traits;
extern crate frustum_query;
extern crate rand_pcg;
extern crate rand;
extern crate structopt;

extern crate mehlon_server;
extern crate mehlon_meshgen;

mod client;
mod collide;
mod ui;
mod voxel_walk;

use glium::glutin;
use client::Game;

use structopt::StructOpt;

use std::thread;
use mehlon_server::{Server, StrErr};
use mehlon_server::generic_net::{MpscServerSocket, NetworkClientConn};
use mehlon_server::quic_net::QuicClientConn;
use mehlon_server::config::load_config;

/// Mehlon client
#[derive(StructOpt, Debug)]
#[structopt(name = "mehlon")]
struct Options {
	/// Connect to the given server
	#[structopt(long = "connect")]
	connect :Option<String>,
}

fn main() -> Result<(), StrErr> {

	let options = Options::from_args();
	let config = load_config();

	let client_conn :Box<dyn NetworkClientConn>= if let Some(addr) = options.connect.clone() {
		let client_conn = QuicClientConn::from_socket_addr(addr)?;
		Box::new(client_conn)
	} else {
		let (server_socket, client_conn) = MpscServerSocket::new();
		let config = config.clone();
		thread::spawn(move || {
			let mut server = Server::new(server_socket, config);
			server.run_loop();
		});
		Box::new(client_conn)
	};

	let mut events_loop = glutin::EventsLoop::new();
	let mut game = Game::new(&events_loop, client_conn, config);

	game.run_loop(&mut events_loop);

	Ok(())
}
