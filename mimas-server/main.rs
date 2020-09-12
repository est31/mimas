#![forbid(unsafe_code)]

extern crate mimas_server;
extern crate structopt;

use mimas_server::{Server, StrErr};
//use mimas_server::generic_net::TcpServerSocket;
use mimas_server::quic_net::QuicServerSocket;
use mimas_server::config::load_config;

use structopt::StructOpt;

/// Mimas server
#[derive(StructOpt, Debug)]
#[structopt(name = "mimas")]
struct Options {
	/// Set the listen address
	#[structopt(long = "listen")]
	listen_addr :Option<String>,
}

fn main() -> Result<(), StrErr> {
	let options = Options::from_args();

	let server_socket = if let Some(addr) = options.listen_addr {
		let addr = addr.parse().expect("couldn't parse address");
		QuicServerSocket::with_socket_addr(&addr)?
	} else {
		QuicServerSocket::new()?
	};
	println!("Listening on {}", server_socket.listen_addr());
	let config = load_config();
	let mut server = Server::new(server_socket, false, config);
	server.run_loop();

	Ok(())
}
