use rusqlite::{Connection, NO_PARAMS};
use map::MapChunkData;
use nalgebra::Vector3;

pub struct SqliteStorageBackend {
	conn :Connection,
}

impl SqliteStorageBackend {
	pub fn from_conn(conn :Connection) -> Self {
		Self {
			conn,
		}
	}
	fn create_tables(&mut self) {
		self.conn.execute(
			"CREATE TABLE kvstore (
				key VARCHAR(16) PRIMARY KEY,
				content BLOB,
			)",
			NO_PARAMS,
		).unwrap();
		self.conn.execute(
			"CREATE TABLE chunks (
				x INTEGER,
				y INTEGER,
				z INTEGER,
				content BLOB,
				PRIMARY KEY(x, y, z)
			)",
			NO_PARAMS,
		).unwrap();
	}
}

impl StorageBackend for SqliteStorageBackend {
	fn store_chunk(&mut self, pos :Vector3<isize>,
			data :&MapChunkData) {
		// TODO
	}
	fn load_chunk(&mut self, pos :Vector3<isize>) -> MapChunkData {
		// TODO
		unimplemented!()
	}
}

pub trait StorageBackend {
	fn store_chunk(&mut self, pos :Vector3<isize>,
			data :&MapChunkData);
	fn load_chunk(&mut self, pos :Vector3<isize>) -> MapChunkData;
}
