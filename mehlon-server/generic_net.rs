
use std::sync::mpsc::{channel, Receiver, Sender};
use {ClientToServerMsg, ServerToClientMsg};

pub trait NetworkServerConn {
	fn try_recv(&self) -> Option<ClientToServerMsg>;
	fn send(&self, msg :ServerToClientMsg);
}

pub trait NetworkClientConn {
	fn try_recv(&self) -> Option<ServerToClientMsg>;
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
	fn try_recv(&self) -> Option<ClientToServerMsg> {
		self.cts_r.try_recv().ok()
	}
	fn send(&self, msg :ServerToClientMsg) {
		let _ = self.stc_s.send(msg);
	}
}

impl NetworkClientConn for MpscClientConn {
	fn try_recv(&self) -> Option<ServerToClientMsg> {
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
