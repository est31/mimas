#![forbid(unsafe_code)]

extern crate nalgebra;
extern crate ncollide3d;
extern crate nphysics3d;
#[macro_use]
extern crate glium;
extern crate winit;
extern crate glium_glyph;
extern crate line_drawing;
extern crate num_traits;
extern crate frustum_query;
extern crate rand_pcg;
extern crate rand;
extern crate structopt;

extern crate mehlon_server;
extern crate mehlon_meshgen;

mod client;
mod ui;
mod voxel_walk;

use glium::glutin;
use client::Game;

use structopt::StructOpt;

use std::thread;
use mehlon_server::Server;
use mehlon_server::generic_net::{TcpClientConn, MpscServerSocket, NetworkClientConn};

/// Mehlon client
#[derive(StructOpt, Debug)]
#[structopt(name = "mehlon")]
struct Options {
	/// Connect to the given server
	#[structopt(long = "connect")]
	connect :Option<String>,
}

fn main() {

	let options = Options::from_args();

	let client_conn :Box<dyn NetworkClientConn>= if let Some(addr) = options.connect.clone() {
		let client_conn = TcpClientConn::from_socket_addr(addr);
		Box::new(client_conn)
	} else {
		let (server_socket, client_conn) = MpscServerSocket::new();
		thread::spawn(move || {
			let mut server = Server::new(server_socket);
			server.run_loop();
		});
		Box::new(client_conn)
	};

	let mut events_loop = glutin::EventsLoop::new();
	let mut game = Game::new(&events_loop, client_conn);

	game.run_loop(&mut events_loop);
}
