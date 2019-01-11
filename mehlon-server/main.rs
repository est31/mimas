#![forbid(unsafe_code)]

extern crate mehlon_server;
extern crate structopt;

use mehlon_server::Server;
use mehlon_server::generic_net::TcpServerSocket;
use mehlon_server::config::load_config;

use structopt::StructOpt;

/// Mehlon server
#[derive(StructOpt, Debug)]
#[structopt(name = "mehlon")]
struct Options {
	/// Set the listen address
	#[structopt(long = "listen")]
	listen_addr :Option<String>,
}

fn main() {
	let options = Options::from_args();

	let server_socket = if let Some(addr) = options.listen_addr {
		TcpServerSocket::with_socket_addr(addr)
	} else {
		TcpServerSocket::new()
	};
	let config = load_config();
	let mut server = Server::new(server_socket, config);
	server.run_loop();
}
