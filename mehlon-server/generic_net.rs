
use std::sync::mpsc::{channel, Receiver, Sender};
use std::net::{TcpStream, TcpListener, SocketAddr, ToSocketAddrs};
use std::io::{Read, Write};
use std::mem::replace;
use {ClientToServerMsg, ServerToClientMsg};
use bincode::{serialize, deserialize};

pub trait NetworkServerConn {
	fn try_recv(&mut self) -> Option<ClientToServerMsg>;
	fn send(&self, msg :ServerToClientMsg);
}

pub trait NetworkClientConn {
	fn try_recv(&mut self) -> Option<ServerToClientMsg>;
	fn send(&self, msg :ClientToServerMsg);
}

pub struct MpscServerConn {
	pub(crate) stc_s :Sender<ServerToClientMsg>,
	pub(crate) cts_r :Receiver<ClientToServerMsg>,
}

pub struct MpscClientConn {
	pub(crate) stc_r :Receiver<ServerToClientMsg>,
	pub(crate) cts_s :Sender<ClientToServerMsg>,
}

impl NetworkServerConn for MpscServerConn {
	fn try_recv(&mut self) -> Option<ClientToServerMsg> {
		self.cts_r.try_recv().ok()
	}
	fn send(&self, msg :ServerToClientMsg) {
		let _ = self.stc_s.send(msg);
	}
}

impl NetworkClientConn for MpscClientConn {
	fn try_recv(&mut self) -> Option<ServerToClientMsg> {
		self.stc_r.try_recv().ok()
	}
	fn send(&self, msg :ClientToServerMsg) {
		let _ = self.cts_s.send(msg);
	}
}

impl MpscServerConn {
	pub fn new() -> (Self, MpscClientConn) {
		let (stc_s, stc_r) = channel();
		let (cts_s, cts_r) = channel();
		let mpsc_socket = MpscServerConn {
			stc_s,
			cts_r,
		};
		let srv_conn = MpscClientConn {
			stc_r,
			cts_s,
		};
		(mpsc_socket, srv_conn)
	}
}

pub struct TcpServerConn {
	stream :TcpMsgStream,
	addr :SocketAddr,
}
pub struct TcpClientConn {
	stream :TcpMsgStream,
}

const LEN_BYTES :usize = 8;
struct TcpMsgStream {
	len_arr :[u8; LEN_BYTES],
	len_read :usize,
	cached :Vec<u8>,
	cached_count :usize,
	tcp_stream :TcpStream,
}

impl NetworkServerConn for TcpServerConn {
	fn try_recv(&mut self) -> Option<ClientToServerMsg> {
		let msg = self.stream.try_recv_msg()?;
		//println!("server recv: {} {:?}", msg.len(), &msg[..4]);
		Some(deserialize(&msg).unwrap())
	}
	fn send(&self, msg :ServerToClientMsg) {
		//self.stream.send_msg(&serialize(&msg).unwrap());
		let buf = &serialize(&msg).unwrap();
		println!("server send: {} {:?}", buf.len(), &buf[..4]);
		let _ :ServerToClientMsg = deserialize(&buf).unwrap();
		self.stream.send_msg(buf);
	}
}

impl NetworkClientConn for TcpClientConn {
	fn try_recv(&mut self) -> Option<ServerToClientMsg> {
		let msg = self.stream.try_recv_msg()?;
		println!("client recv: {} {:?}", msg.len(), &msg[..4]);
		Some(deserialize(&msg).unwrap())
	}
	fn send(&self, msg :ClientToServerMsg) {
		//self.stream.send_msg(&serialize(&msg).unwrap());
		let buf = &serialize(&msg).unwrap();
		//println!("client send: {} {:?}", buf.len(), &buf[..4]);
		let _ :ClientToServerMsg = deserialize(&buf).unwrap();
		self.stream.send_msg(buf);
	}
}

impl TcpMsgStream {
	fn from_tcp_stream(tcp_stream :TcpStream) -> Self {
		TcpMsgStream {
			len_arr : [0; LEN_BYTES],
			len_read : 0,
			cached : Vec::new(),
			cached_count : 0,
			tcp_stream,
		}
	}
	fn send_msg(&self, buf :&[u8]) {
		// Set it to blocking mode for the duration of the write
		// We don't support partial writing yet.
		self.tcp_stream.set_nonblocking(false);
		(&self.tcp_stream).write_all(&(buf.len() as u64).to_be_bytes()).unwrap();
		(&self.tcp_stream).write_all(buf).unwrap();
		(&self.tcp_stream).flush().unwrap();
	}
	fn try_recv_msg(&mut self) -> Option<Vec<u8>> {
		// Set it to nonblocking mode because we do support partial receiving of data.
		self.tcp_stream.set_nonblocking(true);
		if self.len_read < LEN_BYTES {
			match (&self.tcp_stream).read(&mut self.len_arr[self.len_read..]) {
				Ok(amount) => self.len_read += amount,
				Err(_) => return None,
			}
		}
		if self.len_read == LEN_BYTES {
			let length = u64::from_be_bytes(self.len_arr);
			if self.cached.len() != length as usize {
				self.cached = vec![0; length as usize];
			}
			match (&self.tcp_stream).read(&mut self.cached[self.cached_count..]) {
				Ok(amount) => self.cached_count += amount,
				Err(_) => return None,
			}
			if self.cached_count == self.cached.len() {
				let ret = replace(&mut self.cached, Vec::new());
				self.cached_count = 0;
				self.len_read = 0;
				return Some(ret);
			}
		}
		None
	}
}

impl TcpServerConn {
	pub fn from_stream_addr(tcp_stream :TcpStream, addr :SocketAddr) -> Self {
		TcpServerConn {
			stream : TcpMsgStream::from_tcp_stream(tcp_stream),
			addr,
		}
	}
}

impl TcpClientConn {
	pub fn from_stream(tcp_stream :TcpStream) -> Self {
		TcpClientConn {
			stream : TcpMsgStream::from_tcp_stream(tcp_stream),
		}
	}
	pub fn from_socket_addr(addr :impl ToSocketAddrs) -> Self {
		let tcp_stream = TcpStream::connect(addr).expect("couldn't open connection to server");
		TcpClientConn::from_stream(tcp_stream)
	}
}

pub struct TcpServerSocket {
	listener :TcpListener,
}

impl TcpServerSocket {
	pub fn new() -> Self {
		Self::with_socket_addr("127.0.0.1:7700")
	}
	pub fn with_socket_addr(addr :impl ToSocketAddrs) -> Self {
		let listener = TcpListener::bind(addr).expect("can't open tcp listener");
		listener.set_nonblocking(true).expect("can't set nonblocking");
		TcpServerSocket {
			listener,
		}
	}
	pub fn try_open_conn(&mut self) -> Option<TcpServerConn> {
		match self.listener.accept() {
			Ok((stream, addr)) => {
				let conn = TcpServerConn::from_stream_addr(stream, addr);
				Some(conn)
			}
			Err(_) => None,
		}
	}
}
