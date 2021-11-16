use anyhow::{Error, Result};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::time::SystemTime;
use std::net::{SocketAddr, ToSocketAddrs};
use quinn::RecvStream;
use crate::generic_net::{MsgStream, NetErr, MsgStreamClientConn,
	MsgStreamServerConn, NetworkServerSocket};
use std::sync::Arc;

use std::thread;

use futures::StreamExt;
use futures::channel::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};

use rustls::{PrivateKey, Certificate};
use rustls::client::{ServerCertVerified, ServerCertVerifier, ServerName};

use tokio::runtime;
use tokio::io::AsyncWriteExt;

/*macro_rules! eprintln {
	($f:expr, $e:expr) => {{
		print!("{}:{}: ", line!(), column!());
		println!($f, $e);
	}};
}*/

macro_rules! ltry {
	($f:expr; $e:expr) => {{
		match $f {
			Ok(v) => v,
			Err(e) => {
				eprintln!("Net Error: {:?}", e);
				$e
			}
		}
	}};
}

/// A certificate verifier that accepts any certificate
struct NullVerifier;
impl ServerCertVerifier for NullVerifier {
	fn verify_server_cert(
		&self,
		_end_entity :&Certificate,
		_intermediates :&[Certificate],
		_server_name :&ServerName,
		_scts :&mut dyn Iterator<Item = &[u8]>,
		_ocsp_response :&[u8],
		_now :SystemTime,
	) -> Result<ServerCertVerified, rustls::Error> {
		Ok(ServerCertVerified::assertion())
	}
}

fn run_quinn_server(addr :&SocketAddr, conn_send :Sender<QuicServerConn>) -> Result<()> {
	let cert = rcgen::generate_simple_self_signed(vec!["mimas-host".into()])?;

	let key_der = cert.serialize_private_key_der();
	let cert_der = cert.serialize_der()?;
	let key = PrivateKey(key_der);
	let cert = Certificate(cert_der);

	let server_crypto = rustls::ServerConfig::builder()
		.with_safe_defaults()
		.with_no_client_auth()
		.with_single_cert(vec![cert], key)?;

	let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(server_crypto));

	Arc::get_mut(&mut server_config.transport)
		.unwrap()
		// Prevent the connection from timing out
		// when there is no activiy on the connection
		.max_idle_timeout(None);

	let runtime = runtime::Builder::new_current_thread()
		.enable_all()
		.build()?;

	let (_endpoint, mut incoming) = quinn::Endpoint::server(server_config, *addr)?;

	runtime.block_on(async move {
		while let Some(connecting) = incoming.next().await {
			let sender_clone = conn_send.clone();
			tokio::spawn(async move { loop {
				let new_conn = if let Ok(new_conn) = connecting.await {
					new_conn
				} else {
					break;
				};
				let addr = new_conn.connection.remote_address();
				// Only regard the first stream as new connection
				let (stream, _incoming) = new_conn.bi_streams.into_future().await;
				let (mut wtr, rdr) = if let Some(Ok(stream)) = stream {
					stream
				} else {
					break;
				};
				let (msg_stream, mut rcv, snd) = QuicMsgStream::new();

				let conn = QuicServerConn {
					stream : msg_stream,
					addr,
				};
				ltry!(sender_clone.send(conn); break);

				spawn_msg_rcv_task(rdr, snd);

				while let Some(msg) = rcv.next().await {
					let len_buf = (msg.len() as u64).to_be_bytes();
					ltry!(wtr.write_all(&len_buf).await; break);
					ltry!(wtr.write_all(&msg).await; break);
				}
				// Gracefully terminate the stream
				if let Err(e) = wtr.shutdown().await {
					eprintln!("failed to shutdown stream: {}", e);
				}
				break;
			} });
		}
	});
	Ok(())
}

async fn msg_rcv_task(mut rdr :RecvStream, to_receive :Sender<Vec<u8>>) {
	loop {
		let mut len_buf = [0; 8];
		if let Err(e) = rdr.read_exact(&mut len_buf).await {
			if quinn::ReadExactError::FinishedEarly != e {
				eprintln!("Net error: {:?}", e);
			} else {
				// Graceful termination of the stream,
				// don't print an error.
			}
			// The stream terminated.
			break;
		}
		let len = u64::from_be_bytes(len_buf) as usize;
		let mut buf = vec![0; len];
		ltry!(rdr.read_exact(&mut buf).await; break);
		ltry!(to_receive.send(buf); break);
	}
}

fn spawn_msg_rcv_task(rdr :RecvStream, to_receive :Sender<Vec<u8>>) {
	tokio::spawn(msg_rcv_task(rdr, to_receive));
}

fn run_quinn_client(url :impl ToSocketAddrs,
		mut to_send :UnboundedReceiver<Vec<u8>>, to_receive :Sender<Vec<u8>>) -> Result<()> {
	let url = url.to_socket_addrs()?.next().expect("socket addr expected");

	let config = rustls::client::ClientConfig::builder()
		.with_safe_defaults()
		// Trust all certificates
		.with_custom_certificate_verifier(Arc::new(NullVerifier))
		.with_no_client_auth();

	let listen_addr = "[::]:0".parse().unwrap();

	let mut endpoint = quinn::Endpoint::client(listen_addr)?;
	let mut client_config = quinn::ClientConfig::new(Arc::new(config));

	Arc::get_mut(&mut client_config.transport)
		.unwrap()
		// Prevent the connection from timing out
		// when there is no activiy on the connection
		.max_idle_timeout(None);


	endpoint.set_default_client_config(client_config);

	let runtime = runtime::Builder::new_current_thread()
		.enable_all()
		.build()?;

	runtime.block_on(async { loop {
		let endpoint_future = endpoint.connect(url, "mimas-host")?;
		let new_conn = match endpoint_future.await {
			Ok(new_conn) => new_conn,
			Err(e) => {
				eprintln!("Net Error: {:?}", e);
				break Ok(());
			},
		};
		println!("connected to server.");
		let stream = new_conn.connection.open_bi();
		let (mut wtr, rdr) = match stream.await {
			Ok(stream) => stream,
			Err(e) => {
				eprintln!("Net Error: {:?}", e);
				break Ok(());
			},
		};
		spawn_msg_rcv_task(rdr, to_receive);
		while let Some(msg) = to_send.next().await {
			let len_buf = (msg.len() as u64).to_be_bytes();
			ltry!(wtr.write_all(&len_buf).await; break);
			ltry!(wtr.write_all(&msg).await; break);
		}
		// Gracefully terminate the stream
		if let Err(e) = wtr.shutdown().await {
			println!("failed to shutdown stream: {}", e)
		}
		break Ok(());
	}}).map_err(|e :Error| e)?;
	Ok(())
}

pub struct QuicMsgStream {
	sender :UnboundedSender<Vec<u8>>,
	receiver :Receiver<Vec<u8>>,
}

impl QuicMsgStream {
	pub fn new() -> (Self, UnboundedReceiver<Vec<u8>>, Sender<Vec<u8>>) {
		let (u_s, u_rx) = unbounded();
		let (c_s, c_rx) = channel();
		let slf = Self {
			sender : u_s,
			receiver : c_rx,
		};
		(slf, u_rx, c_s)
	}
}

impl MsgStream for QuicMsgStream {
	fn send_msg(&self, buf :&[u8]) -> Result<(), NetErr> {
		self.sender.unbounded_send(buf.into())
			.map_err(|_| NetErr::ConnectionClosed)
	}
	fn try_recv_msg(&mut self) -> Result<Option<Vec<u8>>, NetErr> {
		match self.receiver.try_recv() {
			Ok(v) => Ok(Some(v)),
			Err(TryRecvError::Empty) => Ok(None),
			Err(TryRecvError::Disconnected) => Err(NetErr::ConnectionClosed),
		}
	}
}

pub type QuicClientConn = MsgStreamClientConn<QuicMsgStream>;
pub type QuicServerConn = MsgStreamServerConn<QuicMsgStream>;

impl QuicClientConn {
	pub fn from_socket_addr(addr :&SocketAddr) -> Result<Self> {
		let (stream, rcv, snd) = QuicMsgStream::new();
		let addr = addr.clone();
		thread::spawn(move || {
			run_quinn_client(&addr, rcv, snd).expect("errors in quic client");
		});
		Ok(Self {
			stream,
		})
	}
}

pub struct QuicServerSocket {
	listen_addr :SocketAddr,
	conn_recv :Receiver<QuicServerConn>,
}

impl QuicServerSocket {
	pub fn new() -> Result<Self> {
		let addr = "127.0.0.1:7700".parse().unwrap();
		Self::with_socket_addr(&addr)
	}
	pub fn with_socket_addr(addr :&SocketAddr) -> Result<Self> {
		let addr = addr.clone();
		let (conn_send, conn_recv) = channel();

		thread::spawn(move || {
			run_quinn_server(&addr, conn_send).expect("errors in quic server");
		});
		Ok(Self {
			listen_addr : addr,
			conn_recv
		})
	}
	pub fn listen_addr(&self) -> &SocketAddr {
		&self.listen_addr
	}
}

impl NetworkServerSocket for QuicServerSocket {
	type Conn = QuicServerConn;
	fn try_open_conn(&mut self) -> Option<Self::Conn> {
		self.conn_recv.try_recv().ok()
	}
}
