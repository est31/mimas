#![forbid(unsafe_code)]

extern crate anyhow;
extern crate glium;
extern crate structopt;

extern crate mimas_common;
extern crate mimas_client;
extern crate mimas_server;

use glium::glutin;
use mimas_client::client::Game;

use structopt::StructOpt;

use anyhow::Result;
use std::thread;
use mimas_server::Server;
use mimas_common::generic_net::{MpscServerSocket, NetworkClientConn};
use mimas_common::quic_net::QuicClientConn;
use mimas_common::config::load_config;

/// Mimas client
#[derive(StructOpt, Debug)]
#[structopt(name = "mimas")]
struct Options {
	/// Connect to the given server
	#[structopt(long = "connect")]
	connect :Option<String>,

	/// Use the specified nick
	#[structopt(long = "nick")]
	nick :Option<String>,

	/// Use the specified password
	#[structopt(long = "password")]
	pw :Option<String>,
}

fn main() -> Result<()> {

	let options = Options::from_args();
	let config = load_config();
	let mut nick_pw = None;

	let client_conn :Box<dyn NetworkClientConn> = if let Some(addr) = options.connect.clone() {
		let addr = addr.parse().expect("couldn't parse address");
		let client_conn = QuicClientConn::from_socket_addr(&addr)?;
		let nick = options.nick.unwrap_or_else(|| {
			panic!("No nick specified but needed to connect to server.");
		});
		let pw = options.pw.unwrap_or_else(|| {
			panic!("No password specified but needed to connect to server.");
		});
		nick_pw = Some((nick.clone(), pw));
		Box::new(client_conn)
	} else {
		let (server_socket, client_conn) = MpscServerSocket::new();
		let config = config.clone();
		thread::spawn(move || {
			let mut server = Server::new(server_socket, true, config);
			server.run_loop();
		});
		Box::new(client_conn)
	};

	let mut event_loop = glutin::event_loop::EventLoop::new();
	let mut game = Game::new(&event_loop, client_conn, config, nick_pw);

	game.run_loop(&mut event_loop);

	Ok(())
}
