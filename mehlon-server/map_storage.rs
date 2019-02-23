use rusqlite::{Connection, NO_PARAMS, OptionalExtension};
use rusqlite::types::{Value, ToSql};
use map::{MapChunkData, MapBlock, CHUNKSIZE};
use StrErr;
use nalgebra::Vector3;
use std::{str, io, path::Path};
use byteorder::{ReadBytesExt, WriteBytesExt};
use flate2::{Compression, GzBuilder, read::GzDecoder};
use config::Config;
use toml::{from_str, to_string};
use sqlite_generic::{get_user_version, set_user_version,
	get_app_id, set_app_id, open_or_create_db};

pub struct SqliteStorageBackend {
	conn :Connection,
	ctr :u32,
}

/// Magic used to identify the mehlon application.
///
/// This magic was taken from hexdump -n 32 /dev/urandom output.
const MEHLON_SQLITE_APP_ID :i32 = 0x84eeae3cu32 as i32;

const USER_VERSION :u16 = 2;

/// We group multiple writes into transactions
/// as each transaction incurs a time penalty,
/// which added up, makes having one transaction
/// per write really slow.
const WRITES_PER_TRANSACTION :u32 = 50;

fn init_db(conn :&mut Connection) -> Result<(), StrErr> {
	set_app_id(conn, MEHLON_SQLITE_APP_ID)?;
	set_user_version(conn, USER_VERSION)?;
	conn.execute(
		"CREATE TABLE IF NOT EXISTS kvstore (
			kkey VARCHAR(16) PRIMARY KEY,
			content BLOB
		);",
		NO_PARAMS,
	)?;
	conn.execute(
		"CREATE TABLE IF NOT EXISTS chunks (
			x INTEGER,
			y INTEGER,
			z INTEGER,
			content BLOB,
			PRIMARY KEY(x, y, z)
		)",
		NO_PARAMS,
	)?;
	migrate_v2(conn)?;
	Ok(())
}

fn migrate_v2(conn :&mut Connection) -> Result<(), StrErr> {
	conn.execute(
		"CREATE TABLE IF NOT EXISTS player_kvstore (
			id_src INTEGER,
			id INTEGER,
			kkey VARCHAR(16),
			content BLOB,
			PRIMARY KEY(id_src, id, kkey)
		)",
		NO_PARAMS,
	)?;
	Ok(())
}

fn expect_user_ver(conn :&mut Connection) -> Result<(), StrErr> {
	let app_id = get_app_id(conn)?;
	let user_version = get_user_version(conn)?;
	if app_id != MEHLON_SQLITE_APP_ID {
		Err(format!("expected app id {} but was {}",
			MEHLON_SQLITE_APP_ID, app_id))?;
	}
	if user_version > USER_VERSION {
		Err(format!("user_version of database {} newer than maximum supported {}",
			user_version, USER_VERSION))?;
	} else if user_version < USER_VERSION {
		migrate_v2(conn)?;
		set_user_version(conn, USER_VERSION)?;
	}
	Ok(())
}

impl SqliteStorageBackend {
	pub fn from_conn(mut conn :Connection, freshly_created :bool) -> Result<Self, StrErr> {
		if freshly_created {
			init_db(&mut conn)?;
		} else {
			expect_user_ver(&mut conn)?;
		}

		Ok(Self {
			conn,
			ctr : 0,
		})
	}
	pub fn open_or_create(path :impl AsRef<Path> + Clone) -> Result<Self, StrErr> {
		let (conn, freshly_created) = open_or_create_db(path)?;
		Ok(Self::from_conn(conn, freshly_created)?)
	}
	fn maybe_begin_commit(&mut self) -> Result<(), StrErr> {
		if self.ctr == 0 {
			self.ctr = WRITES_PER_TRANSACTION;
			if !self.conn.is_autocommit() {
				let mut stmt = self.conn.prepare_cached("COMMIT;")?;
				stmt.execute(NO_PARAMS)?;
			}
		} else {
			self.ctr -= 1;
		}
		if self.conn.is_autocommit() {
			let mut stmt = self.conn.prepare_cached("BEGIN;")?;
			stmt.execute(NO_PARAMS)?;
		}
		Ok(())
	}
}

fn mapblock_to_number(b :MapBlock) -> u8 {
	use MapBlock::*;
	match b {
		Air => 0,
		Water => 1,
		Sand => 2,
		Ground => 3,
		Wood => 4,
		Stone => 5,
		Leaves => 6,
		Tree => 7,
		Cactus => 8,
		Coal => 9,
		IronOre => 10,
	}
}

fn number_to_mapblock(b :u8) -> Option<MapBlock> {
	use MapBlock::*;
	Some(match b {
		0 => Air,
		1 => Water,
		2 => Sand,
		3 => Ground,
		4 => Wood,
		5 => Stone,
		6 => Leaves,
		7 => Tree,
		8 => Cactus,
		9 => Coal,
		10 => IronOre,
		_ => return None,
	})
}

fn serialize_mapchunk_data(data :&MapChunkData) -> Vec<u8> {
	let mut blocks = Vec::new();
	for b in data.0.iter() {
		blocks.write_u8(mapblock_to_number(*b)).unwrap();
	}
	let rdr :&[u8] = &blocks;
	let mut gz_enc = GzBuilder::new().read(rdr, Compression::fast());
	let mut r = Vec::<u8>::new();

	// Version
	r.write_u8(0).unwrap();
	io::copy(&mut gz_enc, &mut r).unwrap();
	r
}

fn deserialize_mapchunk_data(data :&[u8]) -> Result<MapChunkData, StrErr> {
	let mut rdr = data;
	let version = rdr.read_u8()?;
	if version != 0 {
		// The version is too recent
		Err(format!("Unsupported map chunk version {}", version))?;
	}
	let mut gz_dec = GzDecoder::new(rdr);
	let mut buffer = Vec::<u8>::new();
	io::copy(&mut gz_dec, &mut buffer)?;
	let mut rdr :&[u8] = &buffer;
	let mut r = MapChunkData::fully_air();
	for v in r.0.iter_mut() {
		let n = rdr.read_u8()?;
		*v = number_to_mapblock(n).ok_or("invalid block number")?;
	}
	Ok(r)
}

impl StorageBackend for SqliteStorageBackend {
	fn store_chunk(&mut self, pos :Vector3<isize>,
			data :&MapChunkData) -> Result<(), StrErr> {
		let pos = pos / CHUNKSIZE;
		let data = serialize_mapchunk_data(&data);
		self.maybe_begin_commit()?;
		let mut stmt = self.conn.prepare_cached("INSERT OR REPLACE INTO chunks (x, y, z, content) \
			VALUES (?, ?, ?, ?);")?;
		stmt.execute(&[&pos.x as &dyn ToSql, &pos.y, &pos.z, &data])?;
		Ok(())
	}
	fn tick(&mut self) -> Result<(), StrErr> {
		if !self.conn.is_autocommit() {
			self.ctr = WRITES_PER_TRANSACTION;
			let mut stmt = self.conn.prepare_cached("COMMIT;")?;
			stmt.execute(NO_PARAMS)?;
		}
		Ok(())
	}
	fn load_chunk(&mut self, pos :Vector3<isize>) -> Result<Option<MapChunkData>, StrErr> {
		let pos = pos / CHUNKSIZE;
		let mut stmt = self.conn.prepare_cached("SELECT content FROM chunks WHERE x=? AND y=? AND z=?")?;
		let data :Option<Vec<u8>> = stmt.query_row(
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
	fn get_global_kv(&mut self, key :&str) -> Result<Option<Vec<u8>>, StrErr> {
		let mut stmt = self.conn.prepare_cached("SELECT content FROM kvstore WHERE kkey=?")?;
		let data :Option<Value> = stmt.query_row(
			&[&key],
			|row| row.get(0)
		).optional()?;
		Ok(value_to_vec(data)?)
	}
	fn set_global_kv(&mut self, key :&str, content :&[u8]) -> Result<(), StrErr> {
		self.maybe_begin_commit()?;
		let mut stmt = self.conn.prepare_cached("INSERT OR REPLACE INTO kvstore (kkey, content) \
			VALUES (?, ?);")?;
		stmt.execute(&[&key as &dyn ToSql, &content])?;
		Ok(())
	}
	fn get_player_kv(&mut self, id_pair :PlayerIdPair, key :&str) -> Result<Option<Vec<u8>>, StrErr> {
		let mut stmt = self.conn.prepare_cached("SELECT content FROM player_kvstore WHERE id_src=? AND id=? AND kkey=?")?;
		let data :Option<Value> = stmt.query_row(
			&[&(id_pair.id_src()) as &dyn ToSql, &(id_pair.id_i64()), &key],
			|row| row.get(0)
		).optional()?;
		Ok(value_to_vec(data)?)
	}
	fn set_player_kv(&mut self, id_pair :PlayerIdPair, key :&str, content :&[u8]) -> Result<(), StrErr> {
		self.maybe_begin_commit()?;
		let mut stmt = self.conn.prepare_cached("INSERT OR REPLACE INTO player_kvstore (id_src, id, kkey, content) \
			VALUES (?, ?, ?, ?);")?;
		stmt.execute(&[&(id_pair.id_src()) as &dyn ToSql,
			&(id_pair.id_i64()), &key, &content])?;
		Ok(())
	}
}

// Sqlite has a thing called "affinity" for its types, see also [1].
// Due to this affinity, if stored with a third party program
// like sqlitebrowser, some entries might end up to be of type Text
// even though they are in a column of type Blob.
// As we explicitly want to support the use case where you
// edit entries in sqlitebrowser or other programs,
// we'll have to support reading in the Text format,
// while also supporting the Blob format for possibly binary data.
// [1]: https://www.sqlite.org/datatype3.html
fn value_to_vec(v :Option<Value>) -> Result<Option<Vec<u8>>, StrErr> {
	Ok(match v {
		Some(Value::Text(s)) => Some(s.into_bytes()),
		Some(Value::Blob(b)) => Some(b),
		Some(_) => return Err("SQL column entry type mismatch: Blob or String required.".into()),
		None => None,
	})
}

pub struct NullStorageBackend;

impl StorageBackend for NullStorageBackend {
	fn store_chunk(&mut self, _pos :Vector3<isize>,
			_data :&MapChunkData) -> Result<(), StrErr> {
		Ok(())
	}
	fn tick(&mut self) -> Result<(), StrErr> {
		Ok(())
	}
	fn load_chunk(&mut self, _pos :Vector3<isize>) -> Result<Option<MapChunkData>, StrErr> {
		Ok(None)
	}
	fn get_global_kv(&mut self, _key :&str) -> Result<Option<Vec<u8>>, StrErr> {
		Ok(None)
	}
	fn set_global_kv(&mut self, _key :&str, _content :&[u8]) -> Result<(), StrErr> {
		Ok(())
	}
	fn get_player_kv(&mut self, _id_pair :PlayerIdPair, _key :&str) -> Result<Option<Vec<u8>>, StrErr> {
		Ok(None)
	}
	fn set_player_kv(&mut self, _id_pair :PlayerIdPair, _key :&str, _content :&[u8]) -> Result<(), StrErr> {
		Ok(())
	}
}

#[derive(PartialEq, Eq, Hash, Copy, Clone)]
pub struct PlayerIdPair(u64);

impl PlayerIdPair {
	pub fn singleplayer() -> Self {
		Self::from_components(0, 0)
	}
	pub fn from_components(id_src :u8, id :u64) -> Self {
		// Impose a limit on the id
		// as too large ids interfere
		// with the src component
		// in our local storage.
		// There is simply no need for
		// such high ids anyway so we
		// limit it to make things easier
		// for us.
		assert!(id < 1 << (64 - 17),
			"id of {} is too big", id);
		let v = ((id_src as u64) << (64 - 8)) | id;
		Self(v)
	}
	pub fn id_src(&self) -> u8 {
		self.0.to_be_bytes()[0]
	}
	pub fn id_u64(&self) -> u64 {
		self.0 & (1 << (64 - 8) - 1)
	}
	pub fn id_i64(&self) -> i64 {
		self.id_u64() as i64
	}
}

#[derive(Serialize, Deserialize)]
pub struct MapgenMetaToml {
	seed :u64,
	mapgen_name :String,
}

fn load_mapgen_meta_toml<B :StorageBackend>(backend :&mut B) -> Result<Option<MapgenMetaToml>, StrErr> {
	let mapgen_meta_arr = if let Some(v) = backend.get_global_kv("mapgen_meta")? {
		v
	} else {
		return Ok(None);
	};
	let mapgen_meta_str = str::from_utf8(&mapgen_meta_arr)?;
	let mapgen_meta = from_str(mapgen_meta_str)?;
	Ok(Some(mapgen_meta))
}

fn save_mapgen_meta_toml<B :StorageBackend>(backend :&mut B, m :&MapgenMetaToml) -> Result<(), StrErr> {
	let mapgen_meta_str = to_string(m)?;
	backend.set_global_kv("mapgen_meta", mapgen_meta_str.as_bytes())?;
	Ok(())
}

fn manage_mapgen_meta_toml<B :StorageBackend>(backend :&mut B, config :&mut Config) -> Result<(), StrErr> {
	if let Some(mapgen_meta) = load_mapgen_meta_toml(backend)? {
		// If a seed already exists, use it
		config.mapgen_seed = mapgen_meta.seed;
	} else {
		// Otherwise write our own seed
		let mapgen_meta = MapgenMetaToml {
			seed : config.mapgen_seed,
			mapgen_name : "mgv1".to_string(),
		};
		save_mapgen_meta_toml(backend, &mapgen_meta)?;
	}
	Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct PlayerPosition {
	x :f32,
	y :f32,
	z :f32,
	pitch :f32,
	yaw :f32,
}

impl Default for PlayerPosition {
	fn default() -> Self {
		Self {
			x : 60.0,
			y : 40.0,
			z : 20.0,
			pitch : 45.0,
			yaw : 0.0,
		}
	}
}

impl PlayerPosition {
	pub fn from_pos(pos :Vector3<f32>) -> Self {
		Self {
			x : pos.x,
			y : pos.y,
			z : pos.z,
			pitch : 45.0,
			yaw : 0.0,
		}
	}
	pub fn pos(&self) -> Vector3<f32> {
		Vector3::new(self.x, self.y, self.z)
	}
}

pub fn load_player_position(backend :&mut (impl StorageBackend + ?Sized), id_pair :PlayerIdPair) -> Result<Option<PlayerPosition>, StrErr> {
	let buf = if let Some(v) = backend.get_player_kv(id_pair, "position")? {
		v
	} else {
		return Ok(None);
	};
	let serialized_str = str::from_utf8(&buf)?;
	let deserialized = from_str(serialized_str)?;
	Ok(Some(deserialized))
}

pub type DynStorageBackend = Box<dyn StorageBackend + Send>;

fn sqlite_backend_from_config(config :&mut Config) -> Option<DynStorageBackend> {
	// TODO: once we have NLL, remove the "cloned" below
	// See: https://github.com/rust-lang/rust/issues/57804
	let p = config.map_storage_path.as_ref().cloned()?;
	let sqlite_backend = match SqliteStorageBackend::open_or_create(p) {
		Ok(mut b) => {
			manage_mapgen_meta_toml(&mut b, config).unwrap();
			b
		},
		Err(e) => {
			println!("Error while opening database: {:?}", e);
			return None;
		},
	};
	Some(Box::new(sqlite_backend))
}

pub fn storage_backend_from_config(config :&mut Config) -> DynStorageBackend {
	sqlite_backend_from_config(config).unwrap_or_else(|| {
		Box::new(NullStorageBackend)
	})
}

pub trait StorageBackend {
	fn store_chunk(&mut self, pos :Vector3<isize>,
			data :&MapChunkData) -> Result<(), StrErr>;
	fn tick(&mut self) -> Result<(), StrErr>;
	fn load_chunk(&mut self, pos :Vector3<isize>) -> Result<Option<MapChunkData>, StrErr>;
	fn get_global_kv(&mut self, key :&str) -> Result<Option<Vec<u8>>, StrErr>;
	fn set_global_kv(&mut self, key :&str, content :&[u8]) -> Result<(), StrErr>;
	fn get_player_kv(&mut self, id_pair :PlayerIdPair, key :&str) -> Result<Option<Vec<u8>>, StrErr>;
	fn set_player_kv(&mut self, id_pair :PlayerIdPair, key :&str, content :&[u8]) -> Result<(), StrErr>;
}
