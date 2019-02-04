use rusqlite::{Connection, NO_PARAMS, OptionalExtension};
use map::{MapChunkData, CHUNKSIZE};
use StrErr;
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
			"CREATE TABLE IF NOT EXISTS kvstore (
				key VARCHAR(16) PRIMARY KEY,
				content BLOB,
			)",
			NO_PARAMS,
		).unwrap();
		self.conn.execute(
			"CREATE TABLE IF NOT EXISTS chunks (
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

fn serialize_mapchunk_data(data :&MapChunkData) -> Vec<u8> {
	unimplemented!()
}

fn deserialize_mapchunk_data(data :&[u8]) -> Result<MapChunkData, StrErr> {
	unimplemented!()
}

impl StorageBackend for SqliteStorageBackend {
	fn store_chunk(&mut self, pos :Vector3<isize>,
			data :&MapChunkData) -> Result<(), StrErr> {
		let pos = pos / CHUNKSIZE;
		let data = serialize_mapchunk_data(&data);
		// TODO prepare this statement
		self.conn.execute_named("UPDATE OR INSERT INTO chunks (x, y, z, content) \
			VALUES (:x, :y, :z, :content)",
			&[(":x", &pos.x), (":y", &pos.y), (":z", &pos.z), (":content", &data)])?;
		Ok(())
	}
	fn load_chunk(&mut self, pos :Vector3<isize>) -> Result<Option<MapChunkData>, StrErr> {
		let pos = pos / CHUNKSIZE;
		// TODO prepare this statement
		let data :Option<Vec<u8>> = self.conn.query_row("SELECT content FROM chunks WHERE x=?,y=?,z=?",
			&[&pos.x, &pos.y, &pos.z],
			|row| row.get(0)
		).optional()?;
		if let Some(data) = data {
			let chunk = deserialize_mapchunk_data(&data)?;
			Ok(Some(chunk))
		} else {
			Ok(None)
		}
	}
}

pub trait StorageBackend {
	fn store_chunk(&mut self, pos :Vector3<isize>,
			data :&MapChunkData) -> Result<(), StrErr>;
	fn load_chunk(&mut self, pos :Vector3<isize>) -> Result<Option<MapChunkData>, StrErr>;
}
