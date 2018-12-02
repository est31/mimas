
use std::sync::mpsc::{channel, Receiver, Sender};
use {ClientToServerMsg, ServerToClientMsg};

pub trait NetworkServerSocket {
	fn try_recv(&self) -> Option<ClientToServerMsg>;
	fn send(&self, msg :ServerToClientMsg);
}

pub trait NetworkClientSocket {
	fn try_recv(&self) -> Option<ServerToClientMsg>;
	fn send(&self, msg :ClientToServerMsg);
}

pub struct MpscServerSocket {
	pub(crate) stc_s :Sender<ServerToClientMsg>,
	pub(crate) cts_r :Receiver<ClientToServerMsg>,
}

pub struct ServerConnection {
	pub(crate) stc_r :Receiver<ServerToClientMsg>,
	pub(crate) cts_s :Sender<ClientToServerMsg>,
}

impl NetworkServerSocket for MpscServerSocket {
	fn try_recv(&self) -> Option<ClientToServerMsg> {
		self.cts_r.try_recv().ok()
	}
	fn send(&self, msg :ServerToClientMsg) {
		let _ = self.stc_s.send(msg);
	}
}

impl NetworkClientSocket for ServerConnection {
	fn try_recv(&self) -> Option<ServerToClientMsg> {
		self.stc_r.try_recv().ok()
	}
	fn send(&self, msg :ClientToServerMsg) {
		let _ = self.cts_s.send(msg);
	}
}

impl MpscServerSocket {
	pub fn new() -> (Self, ServerConnection) {
		let (stc_s, stc_r) = channel();
		let (cts_s, cts_r) = channel();
		let mpsc_socket = MpscServerSocket {
			stc_s,
			cts_r,
		};
		let srv_conn = ServerConnection {
			stc_r,
			cts_s,
		};
		(mpsc_socket, srv_conn)
	}
}
