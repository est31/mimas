
use std::sync::mpsc::{channel, Receiver, Sender};
use std::net::{TcpStream, TcpListener, SocketAddr, ToSocketAddrs};
use std::io::{Read, Write, Error as IoError, ErrorKind};
use std::mem::replace;
use {ClientToServerMsg, ServerToClientMsg};
use bincode::{serialize, deserialize};
use StrErr;

pub trait NetworkServerSocket {
	type Conn :NetworkServerConn + 'static;
	fn try_open_conn(&mut self) -> Option<Self::Conn>;
}

pub trait NetworkServerConn {
	fn try_recv(&mut self) -> Result<Option<ClientToServerMsg>, NetErr>;
	fn send(&self, msg :ServerToClientMsg) -> Result<(), NetErr>;
}

pub trait NetworkClientConn {
	fn try_recv(&mut self) -> Result<Option<ServerToClientMsg>, NetErr>;
	fn send(&self, msg :ClientToServerMsg) -> Result<(), NetErr>;
}

impl NetworkClientConn for Box<dyn NetworkClientConn> {
	fn try_recv(&mut self) -> Result<Option<ServerToClientMsg>, NetErr> {
		(**self).try_recv()
	}
	fn send(&self, msg :ClientToServerMsg) -> Result<(), NetErr> {
		(**self).send(msg)
	}
}

pub struct MpscServerSocket {
	srv_conn :Option<MpscServerConn>,
}

pub struct MpscServerConn {
	stc_s :Sender<ServerToClientMsg>,
	cts_r :Receiver<ClientToServerMsg>,
}

pub struct MpscClientConn {
	stc_r :Receiver<ServerToClientMsg>,
	cts_s :Sender<ClientToServerMsg>,
}

impl NetworkServerSocket for MpscServerSocket {
	type Conn = MpscServerConn;
	fn try_open_conn(&mut self) -> Option<Self::Conn> {
		self.srv_conn.take()
	}
}

impl NetworkServerConn for MpscServerConn {
	fn try_recv(&mut self) -> Result<Option<ClientToServerMsg>, NetErr> {
		Ok(self.cts_r.try_recv().ok())
	}
	fn send(&self, msg :ServerToClientMsg) -> Result<(), NetErr> {
		let _ = self.stc_s.send(msg);
		Ok(())
	}
}

impl NetworkClientConn for MpscClientConn {
	fn try_recv(&mut self) -> Result<Option<ServerToClientMsg>, NetErr> {
		Ok(self.stc_r.try_recv().ok())
	}
	fn send(&self, msg :ClientToServerMsg) -> Result<(), NetErr> {
		let _ = self.cts_s.send(msg);
		Ok(())
	}
}

impl MpscServerSocket {
	pub fn new() -> (Self, MpscClientConn) {
		let (srv_conn, client_conn) = MpscServerConn::new();
		let res = MpscServerSocket {
			srv_conn : Some(srv_conn),
		};
		(res, client_conn)
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

pub trait MsgStream {
	fn send_msg(&self, buf :&[u8]) -> Result<(), NetErr>;
	fn try_recv_msg(&mut self) -> Result<Option<Vec<u8>>, NetErr>;
}

pub struct MsgStreamServerConn<M :MsgStream> {
	stream :M,
	addr :SocketAddr,
}
pub struct MsgStreamClientConn<M :MsgStream> {
	stream :M,
}

pub type TcpClientConn = MsgStreamClientConn<TcpMsgStream>;
pub type TcpServerConn = MsgStreamServerConn<TcpMsgStream>;

const LEN_BYTES :usize = 8;
pub struct TcpMsgStream {
	len_arr :[u8; LEN_BYTES],
	len_read :usize,
	cached :Vec<u8>,
	cached_count :usize,
	tcp_stream :TcpStream,
}

impl<M :MsgStream> NetworkServerConn for MsgStreamServerConn<M> {
	fn try_recv(&mut self) -> Result<Option<ClientToServerMsg>, NetErr> {
		let msg = self.stream.try_recv_msg()?;
		if let Some(msg) = msg {
			//println!("server recv: {} {:?}", msg.len(), &msg[..4]);
			Ok(Some(deserialize(&msg).unwrap()))
		} else {
			Ok(None)
		}
	}
	fn send(&self, msg :ServerToClientMsg) -> Result<(), NetErr> {
		//self.stream.send_msg(&serialize(&msg).unwrap());
		let buf = &serialize(&msg).unwrap();
		println!("server send: {} {:?}", buf.len(), &buf[..4]);
		let _ :ServerToClientMsg = deserialize(&buf).unwrap();
		self.stream.send_msg(buf)
	}
}

impl<M :MsgStream> NetworkClientConn for MsgStreamClientConn<M> {
	fn try_recv(&mut self) -> Result<Option<ServerToClientMsg>, NetErr> {
		let msg = self.stream.try_recv_msg()?;
		if let Some(msg) = msg {
			println!("client recv: {} {:?}", msg.len(), &msg[..4]);
			Ok(Some(deserialize(&msg).unwrap()))
		} else {
			Ok(None)
		}
	}
	fn send(&self, msg :ClientToServerMsg) -> Result<(), NetErr> {
		//self.stream.send_msg(&serialize(&msg).unwrap());
		let buf = &serialize(&msg).unwrap();
		//println!("client send: {} {:?}", buf.len(), &buf[..4]);
		let _ :ClientToServerMsg = deserialize(&buf).unwrap();
		self.stream.send_msg(buf)
	}
}

#[derive(Clone, Debug)]
pub enum NetErr {
	ConnectionClosed,
	Other,
}

impl From<IoError> for NetErr {
	fn from(io_err :IoError) -> Self {
		match io_err.kind() {
			ErrorKind::ConnectionReset => NetErr::ConnectionClosed,
			_ => NetErr::Other,
		}
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
}
impl MsgStream for TcpMsgStream {
	fn send_msg(&self, buf :&[u8]) -> Result<(), NetErr> {
		// Set it to blocking mode for the duration of the write
		// We don't support partial writing yet.
		self.tcp_stream.set_nonblocking(false)?;
		(&self.tcp_stream).write_all(&(buf.len() as u64).to_be_bytes())?;
		(&self.tcp_stream).write_all(buf)?;
		(&self.tcp_stream).flush()?;
		Ok(())
	}
	fn try_recv_msg(&mut self) -> Result<Option<Vec<u8>>, NetErr> {
		// Set it to nonblocking mode because we do support partial receiving of data.
		self.tcp_stream.set_nonblocking(true)?;
		if self.len_read < LEN_BYTES {
			match (&self.tcp_stream).read(&mut self.len_arr[self.len_read..]) {
				Ok(amount) => self.len_read += amount,
				Err(ref e) if e.kind() == ErrorKind::WouldBlock => return Ok(None),
				Err(e) => return Err(NetErr::from(e)),
			}
		}
		if self.len_read == LEN_BYTES {
			let length = u64::from_be_bytes(self.len_arr);
			if self.cached.len() != length as usize {
				self.cached = vec![0; length as usize];
			}
			match (&self.tcp_stream).read(&mut self.cached[self.cached_count..]) {
				Ok(amount) => self.cached_count += amount,
				Err(ref e) if e.kind() == ErrorKind::WouldBlock => return Ok(None),
				Err(e) => return Err(NetErr::from(e)),
			}
			if self.cached_count == self.cached.len() {
				let ret = replace(&mut self.cached, Vec::new());
				self.cached_count = 0;
				self.len_read = 0;
				return Ok(Some(ret));
			}
		}
		Ok(None)
	}
}

impl TcpServerConn {
	pub fn from_stream_addr(tcp_stream :TcpStream, addr :SocketAddr) -> Self {
		TcpServerConn {
			stream : TcpMsgStream::from_tcp_stream(tcp_stream),
			addr,
		}
	}
	pub fn get_addr(&self) -> SocketAddr {
		self.addr
	}
}

impl TcpClientConn {
	pub fn from_stream(tcp_stream :TcpStream) -> Self {
		TcpClientConn {
			stream : TcpMsgStream::from_tcp_stream(tcp_stream),
		}
	}
	pub fn from_socket_addr(addr :impl ToSocketAddrs) -> Result<Self, StrErr> {
		let tcp_stream = TcpStream::connect(addr)?;
		Ok(TcpClientConn::from_stream(tcp_stream))
	}
}

pub struct TcpServerSocket {
	listener :TcpListener,
}

impl NetworkServerSocket for TcpServerSocket {
	type Conn = TcpServerConn;
	fn try_open_conn(&mut self) -> Option<Self::Conn> {
		match self.listener.accept() {
			Ok((stream, addr)) => {
				let conn = TcpServerConn::from_stream_addr(stream, addr);
				Some(conn)
			}
			Err(_) => None,
		}
	}
}

impl TcpServerSocket {
	pub fn new() -> Result<Self, StrErr> {
		Self::with_socket_addr("127.0.0.1:7700")
	}
	pub fn with_socket_addr(addr :impl ToSocketAddrs) -> Result<Self, StrErr> {
		let listener = TcpListener::bind(addr)?;
		listener.set_nonblocking(true)?;
		Ok(TcpServerSocket {
			listener,
		})
	}
}
