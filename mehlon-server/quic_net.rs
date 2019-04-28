use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::net::ToSocketAddrs;
use StrErr;
use quinn::{NewStream, RecvStream, EndpointBuilder};
use slog::{o, Drain, Logger};
use generic_net::{MsgStream, NetErr, MsgStreamClientConn,
	MsgStreamServerConn, NetworkServerSocket};
use std::sync::Arc;

use std::thread;

use futures::{Future, Stream};
use futures::sync::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};
use tokio::runtime::current_thread::{self, Runtime};

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

// This value prevents the connection from timing
// out when there is no activiy on the connection
//
// We are not setting to 0 as we are supposed to,
// because there is a bug in quinn right now.
// Hopefully the next version will resolve it.
// 1 << 27 - 1 is the biggest value we can set
// idle_timeout to without encountering bugs.
// See also:
// https://github.com/djc/quinn/issues/210
const TRANSPORT_IDLE_TIMEOUT :u64 = 0;

fn run_quinn_server(addr :impl ToSocketAddrs, conn_send :Sender<QuicServerConn>) -> Result<(), StrErr> {
	let log = get_logger();

	let mut server_config = quinn::ServerConfigBuilder::default();
	let cert = rcgen::generate_simple_self_signed(vec!["mehlon-host".into()]);

	let key_der = cert.serialize_private_key_der();
	let cert_der = cert.serialize_der();
	let key = quinn::PrivateKey::from_der(&key_der)?;
	let cert = quinn::Certificate::from_der(&cert_der)?;

	server_config.certificate(quinn::CertificateChain::from_certs(vec![cert]), key)?;

	let mut server_config = server_config.build();

	Arc::get_mut(&mut server_config.transport)
		.unwrap()
		.idle_timeout = TRANSPORT_IDLE_TIMEOUT;

	let mut endpoint = EndpointBuilder::default();
	endpoint.logger(log.clone());
	endpoint.listen(server_config);

	let mut runtime = Runtime::new()?;


	let (driver, _, incoming) = endpoint.bind(addr)?;
	runtime.spawn(incoming.fold(conn_send, move |conn_send, conn_tup| {
		let (connection_drv, connection, incoming) = conn_tup;
		current_thread::spawn(connection_drv
			.map_err(|e| eprintln!("Connection driver error: {}", e)));
		let addr = connection.remote_address();
		let sender_clone = conn_send.clone();
		current_thread::spawn(
			incoming
				.map_err(move |e| eprintln!("Connection terminated: {}", e))
				.into_future()
				// Only regard the first stream as new connection
				.and_then(move |(stream, _incoming)| {
					let (wtr, rdr) = match stream {
						Some(NewStream::Bi(send_s, recv_s)) => (send_s, recv_s),
						None | Some(NewStream::Uni(_)) => return Ok(()),
					};
					let (msg_stream, rcv, snd) = QuicMsgStream::new();

					let conn = QuicServerConn {
						stream : msg_stream,
						addr,
					};
					sender_clone.send(conn);

					spawn_msg_rcv_task(rdr, snd);

					current_thread::spawn(
						rcv.fold(wtr, |wtr, msg| {
							let len_buf = (msg.len() as u64).to_be_bytes();
							tokio::io::write_all(wtr, len_buf).map_err(|e| {eprintln!("Net Error: {:?}", e); })
								.map(|(wtr, _msg)| wtr)
								.and_then(|wtr| {
									tokio::io::write_all(wtr, msg).map_err(|e| {eprintln!("Net Error: {:?}", e); })
										.map(|(wtr, _msg)| wtr)
								})
						})
						// Gracefully terminate the stream
						.and_then(|wtr| {
							tokio::io::shutdown(wtr)
								.map_err(|e| eprintln!("failed to shutdown stream: {}", e))
						})
						.map(|_| ())
					);

					Ok(())
				})
				.map_err(move |_e| eprintln!("error"))
				.map(|_| {}),
		);
		Ok(conn_send)
	}).map(|_| {}));
    runtime.block_on(driver)?;
	Ok(())
}

fn spawn_msg_rcv_task(rdr :RecvStream, to_receive :Sender<Vec<u8>>) {
	current_thread::spawn(tokio::io::read_exact(rdr, [0; 8])
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

	let mut endpoint = EndpointBuilder::default();
	let mut client_config = quinn::ClientConfigBuilder::default();

	client_config.protocols(&[b"mehlon"]);

	let mut client_config = client_config.build();

	Arc::get_mut(&mut client_config.transport)
		.unwrap()
		.idle_timeout = TRANSPORT_IDLE_TIMEOUT;

	// Trust all certificates
	Arc::get_mut(&mut client_config.crypto).unwrap().dangerous()
		.set_certificate_verifier(Arc::new(NullVerifier));

	endpoint.default_client_config(client_config);

	let (driver, endpoint, _) = endpoint.bind("[::]:0")?;

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
			let (driver, conn, _incoming) = conn;
			current_thread::spawn(driver.map_err(|e| eprintln!("connection driver error: {}", e)));
			let stream = conn.open_bi();
			stream.map_err(|e| {eprintln!("Net Error: {:?}", e); })
			.and_then(move |stream| {
				let (wtr, rdr) = stream;
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
	).map_err(|_| "network error")?;
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
