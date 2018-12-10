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
extern crate mehlon_server;
extern crate mehlon_meshgen;

mod map;
mod client;

use glium::glutin;
use client::Game;

use std::thread;
use mehlon_server::Server;
use mehlon_server::generic_net::{TcpClientConn, TcpServerSocket};

fn main() {
	let mut events_loop = glutin::EventsLoop::new();
	let client_conn = TcpClientConn::from_socket_addr("127.0.0.1:7700");
	let mut game = Game::new(&events_loop, client_conn);

	let start_server = false;
	if start_server {
		let server_socket = TcpServerSocket::new();
		thread::spawn(move || {
			let mut server = Server::new(server_socket);
			server.run_loop();
		});
	}
	game.run_loop(&mut events_loop);
}
