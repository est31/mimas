#![forbid(unsafe_code)]

extern crate mehlon_server;

use mehlon_server::{Server, ServerToClientMsg, ClientToServerMsg};
use mehlon_server::generic_net::{TcpClientConn, TcpServerSocket, NetworkClientConn};

fn main() {
	let server_socket = TcpServerSocket::new();
	let mut server = Server::new(server_socket);
	server.run_loop();
}
