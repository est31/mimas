use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::net::ToSocketAddrs;
use StrErr;
use quinn::{BiStream, NewStream};
use slog::{o, Drain, Logger};
use generic_net::{MsgStream, NetErr, MsgStreamClientConn,
	MsgStreamServerConn, NetworkServerSocket};
use std::sync::Arc;

use std::thread;

use futures::{Future, Stream};
use futures::sync::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};
use tokio::runtime::current_thread::Runtime;
use tokio::io::AsyncRead;
use tokio::io::ReadHalf;

/// A certificate verifier that accepts any certificate
struct NullVerifier;
impl rustls::ServerCertVerifier for NullVerifier {
	fn verify_server_cert(
		&self,
		_roots :&rustls::RootCertStore,
		_presented_certs :&[rustls::Certificate],
		_dns_name :webpki::DNSNameRef,
		_ocsp_response :&[u8],
	) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
		Ok(rustls::ServerCertVerified::assertion())
	}
}

fn run_quinn_server(addr :impl ToSocketAddrs, conn_send :Sender<QuicServerConn>) -> Result<(), StrErr> {
	let log = get_logger();

	let mut server_config = quinn::ServerConfigBuilder::default();
	let cert = rcgen::generate_simple_self_signed(vec!["mehlon-host".into()]);

	let key_der = cert.serialize_private_key_der();
	let cert_der = cert.serialize_der();
	let key = quinn::PrivateKey::from_der(&key_der)?;
	let cert = quinn::Certificate::from_der(&cert_der)?;
	server_config.set_certificate(quinn::CertificateChain::from_certs(vec![cert]), key)?;

	let mut endpoint = quinn::Endpoint::new();
	endpoint.logger(log.clone());
	endpoint.listen(server_config.build());

	let mut runtime = Runtime::new()?;


	let (_, driver, incoming) = endpoint.bind(addr)?;
    runtime.spawn(incoming.fold(conn_send, move |conn_send, conn| {
		let quinn::NewConnection {
			incoming,
			connection,
		} = conn;
		let addr = connection.remote_address();
		let sender_clone = conn_send.clone();
		tokio_current_thread::spawn(
			incoming
				.map_err(move |e| eprintln!("Connection terminated: {}", e))
				.fold(sender_clone, move |conn_send, stream| {
					// TODO only regard the first stream as new connection
					let stream = match stream {
						NewStream::Uni(_) => panic!("oh no for now I dont want to deal with this"),
						NewStream::Bi(stream) => stream,
					};
					let (rdr, wtr) = stream.split();
					let (msg_stream, rcv, snd) = QuicMsgStream::new();

					let conn = QuicServerConn {
						stream : msg_stream,
						addr,
					};
					conn_send.send(conn);

					spawn_msg_rcv_task(rdr, snd);

					rcv.fold(wtr, |wtr, msg| {
						let len_buf = (msg.len() as u64).to_be_bytes();
						tokio::io::write_all(wtr, len_buf).map_err(|e| {eprintln!("Net Error: {:?}", e); })
							.map(|(wtr, _msg)| wtr)
							.and_then(|wtr| {
								tokio::io::write_all(wtr, msg).map_err(|e| {eprintln!("Net Error: {:?}", e); })
									.map(|(wtr, _msg)| wtr)
							})
					}).map(|_| conn_send)

				})
				.map(|_| {}),
		);
		Ok(conn_send)
	}).map(|_| {}));
    runtime.block_on(driver)?;
	Ok(())
}

fn spawn_msg_rcv_task(rdr :ReadHalf<BiStream>, to_receive :Sender<Vec<u8>>) {
	tokio_current_thread::spawn(tokio::io::read_exact(rdr, [0; 8])
		.and_then(move |(rdr, v)| {
			let len = u64::from_be_bytes(v) as usize;
			tokio::io::read_exact(rdr, vec![0; len])
				.and_then(move |(rdr, v)| {
					to_receive.send(v);
					spawn_msg_rcv_task(rdr, to_receive);
					Ok(())
				})
		}).map_err(|e| {eprintln!("Net Error: {:?}", e); })
					.map(|_| ())
	);
}

fn run_quinn_client(url :impl ToSocketAddrs,
		to_send :UnboundedReceiver<Vec<u8>>, to_receive :Sender<Vec<u8>>) -> Result<(), StrErr> {
	let url = url.to_socket_addrs()?.next().expect("socket addr expected");

	let mut endpoint = quinn::Endpoint::new();
	let mut client_config = quinn::ClientConfigBuilder::new();

	client_config.set_protocols(&[b"mehlon"]);

	let mut client_config = client_config.build();

	// Trust all certificates
	Arc::get_mut(&mut client_config.tls_config).unwrap().dangerous()
		.set_certificate_verifier(Arc::new(NullVerifier));

	endpoint.default_client_config(client_config);

	let (endpoint, driver, _) = endpoint.bind("[::]:0")?;

	let mut runtime = Runtime::new()?;
	let endpoint_future = endpoint.connect(
		&url,
		"mehlon-host"
	)?;
	runtime.spawn(driver.map_err(|e| eprintln!("IO error: {}", e)));
	runtime.block_on(
		endpoint_future
		.map_err(|e| {eprintln!("Net Error: {:?}", e); })
		.and_then(move |conn| {
			println!("connected to server.");
			let conn = conn.connection;
			let stream = conn.open_bi();
			stream.map_err(|e| {eprintln!("Net Error: {:?}", e); })
			.and_then(move |stream| {
				let (rdr, wtr) = stream.split();
				spawn_msg_rcv_task(rdr, to_receive);
				to_send.fold(wtr, |wtr, msg| {
					let len_buf = (msg.len() as u64).to_be_bytes();
					tokio::io::write_all(wtr, len_buf).map_err(|e| {eprintln!("Net Error: {:?}", e); })
						.map(|(wtr, _msg)| wtr)
						.and_then(|wtr| {
							tokio::io::write_all(wtr, msg).map_err(|e| {eprintln!("Net Error: {:?}", e); })
								.map(|(wtr, _msg)| wtr)
						})
				})
				// Gracefully terminate the stream
				.and_then(|stream| {
					tokio::io::shutdown(stream)
						.map_err(|e| eprintln!("failed to shutdown stream: {}", e))
				})
			})//.map_err(|e| {eprintln!("Net Error: {:?}", e);})
		}).map_err(|e| {eprintln!("Net Error: {:?}", e); })
	).map_err(|_| "error")?;
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
	pub fn from_socket_addr(addr :impl ToSocketAddrs) -> Result<Self, StrErr> {
		let (stream, rcv, snd) = QuicMsgStream::new();
		let addrs = addr.to_socket_addrs()?.collect::<Vec<_>>();
		thread::spawn(move || {
			let addrs = addrs;
			let addrs = &addrs as &[_];
			run_quinn_client(addrs, rcv, snd).expect("errors in quic client");
		});
		Ok(Self {
			stream,
		})
	}
}

pub struct QuicServerSocket {
	conn_recv :Receiver<QuicServerConn>,
}

fn get_logger() -> Logger {
	let decorator = slog_term::PlainSyncDecorator::new(std::io::stderr());
	let drain = slog_term::FullFormat::new(decorator)
		.use_original_order()
		.build()
		.fuse();
	Logger::root(drain, o!())
}

impl QuicServerSocket {
	pub fn new() -> Result<Self, StrErr> {
		Self::with_socket_addr("127.0.0.1:7700")
	}
	pub fn with_socket_addr(addr :impl ToSocketAddrs) -> Result<Self, StrErr> {
		let addrs = addr.to_socket_addrs()?.collect::<Vec<_>>();
		let (conn_send, conn_recv) = channel();

		thread::spawn(move || {
			let addrs = addrs;
			let addrs = &addrs as &[_];
			run_quinn_server(addrs, conn_send).expect("errors in quic server");
		});
		Ok(Self {
			conn_recv
		})
	}
}

impl NetworkServerSocket for QuicServerSocket {
	type Conn = QuicServerConn;
	fn try_open_conn(&mut self) -> Option<Self::Conn> {
		self.conn_recv.try_recv().ok()
	}
}
