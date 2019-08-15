use rusqlite::{Connection, NO_PARAMS, OptionalExtension};
use rusqlite::types::{Value, ToSql};
use map::{MapChunkData, CHUNKSIZE};
use StrErr;
use nalgebra::Vector3;
use std::{str, io, path::Path};
use byteorder::{ReadBytesExt, WriteBytesExt, BigEndian};
use flate2::{Compression, GzBuilder, read::GzDecoder};
use config::Config;
use toml::{from_str, to_string};
use sqlite_generic::{get_user_version, set_user_version,
	get_app_id, set_app_id, open_or_create_db};
use local_auth::SqliteLocalAuth;
use std::num::NonZeroU64;
use game_params::{NameIdMap, check_name_format};

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

fn serialize_mapchunk_data(data :&MapChunkData) -> Vec<u8> {
	let mut blocks = Vec::new();
	for b in data.0.iter() {
		blocks.write_u8(b.id()).unwrap();
	}
	let rdr :&[u8] = &blocks;
	let mut gz_enc = GzBuilder::new().read(rdr, Compression::fast());
	let mut r = Vec::<u8>::new();

	// Version
	r.write_u8(0).unwrap();
	io::copy(&mut gz_enc, &mut r).unwrap();
	r
}

fn deserialize_mapchunk_data(data :&[u8], m :&NameIdMap) -> Result<MapChunkData, StrErr> {
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
	let mut r = MapChunkData::uninitialized();
	for v in r.0.iter_mut() {
		let n = rdr.read_u8()?;
		*v = m.mb_from_id(n).ok_or("invalid block number")?;
	}
	Ok(r)
}

fn serialize_name_id_map(m :&NameIdMap) -> Vec<u8> {
	use std::io::Write;
	let names = m.names();
	let mut r = Vec::new();
	// Version
	r.write_u8(0).unwrap();
	assert!(names.len() < u16::max_value() as usize);
	r.write_u16::<BigEndian>(names.len() as u16).unwrap();
	for n in names {
		assert!(n.len() < u8::max_value() as usize);
		r.write_u8(n.len() as u8).unwrap();
		r.write(&n.as_bytes()).unwrap();
	}
	r
}

fn deserialize_name_id_map(data :&[u8]) -> Result<NameIdMap, StrErr> {
	use std::io::Read;
	let mut rdr = data;
	let version = rdr.read_u8()?;
	if version != 0 {
		// The version is too recent
		Err(format!("Unsupported name id map version {}", version))?;
	}
	let count = rdr.read_u16::<BigEndian>()?;
	if count >= u8::max_value() as u16 {
		// We use u8 as storage for now so we don't support
		// any counts above 255. 255 is reserved.
		Err(format!("Too many id's stored in name id map {}", count))?;
	}
	let mut res = Vec::with_capacity(count as usize);
	for _ in 0 .. count {
		let len = rdr.read_u8()? as usize;
		let mut s = vec![0; len];
		rdr.read_exact(&mut s)?;
		let name = String::from_utf8(s)?;
		let _components = check_name_format(&name)?;
		res.push(name);
	}
	Ok(NameIdMap::from_name_list(res))
}

#[test]
#[cfg(test)]
fn ensure_name_id_roundtrip() {
	let nm = NameIdMap::builtin_name_list();
	let serialized = serialize_name_id_map(&nm);
	let round_trip = deserialize_name_id_map(&serialized).unwrap();
	assert_eq!(nm.names(), round_trip.names());
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
	fn load_chunk(&mut self, pos :Vector3<isize>, m :&NameIdMap) -> Result<Option<MapChunkData>, StrErr> {
		let pos = pos / CHUNKSIZE;
		let mut stmt = self.conn.prepare_cached("SELECT content FROM chunks WHERE x=? AND y=? AND z=?")?;
		let data :Option<Vec<u8>> = stmt.query_row(
			&[&pos.x, &pos.y, &pos.z],
			|row| row.get(0)
		).optional()?;
		if let Some(data) = data {
			let chunk = deserialize_mapchunk_data(&data, m)?;
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
	fn load_chunk(&mut self, _pos :Vector3<isize>, _m :&NameIdMap) -> Result<Option<MapChunkData>, StrErr> {
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

#[derive(PartialEq, Eq, Hash, Copy, Clone, Serialize, Deserialize)]
pub struct PlayerIdPair(NonZeroU64);

impl PlayerIdPair {
	pub fn singleplayer() -> Self {
		Self::from_components(0, 1)
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
		Self(NonZeroU64::new(v).unwrap())
	}
	pub fn id_src(&self) -> u8 {
		self.0.get().to_be_bytes()[0]
	}
	pub fn id_u64(&self) -> u64 {
		self.0.get() & ((1 << (64 - 8)) - 1)
	}
	pub fn id_i64(&self) -> i64 {
		self.id_u64() as i64
	}
}

#[cfg(test)]
#[test]
fn test_player_id_pair() {
	for i in 0 .. 32 {
		for j in 0 .. 32 {
			if (i, j) == (0, 0) {
				continue;
			}
			let id = PlayerIdPair::from_components(i, j);
			assert_eq!((id.id_src(), id.id_u64()), (i, j));
		}
	}
}

// This function is not generic on the backend because of a limitation of the language:
// Box<dyn Trait> does not impl Trait.
pub(crate) fn load_name_id_map(backend :&mut DynStorageBackend) -> Result<NameIdMap, StrErr> {
	let buf = if let Some(v) = backend.get_global_kv("name_id_map")? {
		v
	} else {
		return Ok(NameIdMap::builtin_name_list());
	};
	let nm = deserialize_name_id_map(&buf)?;
	Ok(nm)
}

// This function is not generic on the backend because of a limitation of the language:
// Box<dyn Trait> does not impl Trait.
pub(crate) fn save_name_id_map(backend :&mut DynStorageBackend, nm :&NameIdMap) -> Result<(), StrErr> {
	let buf = serialize_name_id_map(nm);
	backend.set_global_kv("name_id_map", &buf)?;
	Ok(())
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

#[derive(Clone, Copy, Serialize, Deserialize)]
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
		Self::from_pos_pitch_yaw(pos, 45.0, 0.0)
	}
	pub fn from_pos_pitch_yaw(pos :Vector3<f32>, pitch :f32, yaw :f32) -> Self {
		Self {
			x : pos.x,
			y : pos.y,
			z : pos.z,
			pitch,
			yaw,
		}
	}
	pub fn pos(&self) -> Vector3<f32> {
		Vector3::new(self.x, self.y, self.z)
	}
	pub fn pitch(&self) -> f32 {
		self.pitch
	}
	pub fn yaw(&self) -> f32 {
		self.yaw
	}
	pub fn deserialize(buf :&[u8]) -> Result<Self, StrErr> {
		let serialized_str = str::from_utf8(buf)?;
		let deserialized = from_str(serialized_str)?;
		Ok(deserialized)
	}
}

pub type DynStorageBackend = Box<dyn StorageBackend + Send>;

fn sqlite_backend_from_config(config :&mut Config, auth_needed :bool)
		-> Option<(DynStorageBackend, Option<SqliteLocalAuth>)> {
	let p = config.map_storage_path.as_ref()?;

	let p_config = Path::new(&p);
	let p_auth = p_config.with_file_name(p_config.file_stem()
			.and_then(|v| v.to_str()).unwrap_or("").to_owned()
		+ "-auth.sqlite");

	let sqlite_backend = match SqliteStorageBackend::open_or_create(&p) {
		Ok(mut b) => {
			manage_mapgen_meta_toml(&mut b, config).unwrap();
			b
		},
		Err(e) => {
			println!("Error while opening database: {:?}", e);
			return None;
		},
	};
	let storage_backend = Box::new(sqlite_backend);
	let local_auth = if auth_needed {
		Some(SqliteLocalAuth::open_or_create(p_auth).unwrap())
	} else {
		None
	};

	Some((storage_backend, local_auth))
}

pub fn backends_from_config(config :&mut Config, auth_needed :bool)
		-> (DynStorageBackend, Option<SqliteLocalAuth>) {
	sqlite_backend_from_config(config, auth_needed).unwrap_or_else(|| {
		let storage_backend = Box::new(NullStorageBackend);
		let local_auth = if auth_needed {
			let local_auth_conn = Connection::open_in_memory().unwrap();
			Some(SqliteLocalAuth::from_conn(local_auth_conn, true).unwrap())
		} else {
			None
		};
		(storage_backend, local_auth)
	})
}

pub trait StorageBackend {
	fn store_chunk(&mut self, pos :Vector3<isize>,
			data :&MapChunkData) -> Result<(), StrErr>;
	fn tick(&mut self) -> Result<(), StrErr>;
	fn load_chunk(&mut self, pos :Vector3<isize>, m :&NameIdMap) -> Result<Option<MapChunkData>, StrErr>;
	fn get_global_kv(&mut self, key :&str) -> Result<Option<Vec<u8>>, StrErr>;
	fn set_global_kv(&mut self, key :&str, content :&[u8]) -> Result<(), StrErr>;
	fn get_player_kv(&mut self, id_pair :PlayerIdPair, key :&str) -> Result<Option<Vec<u8>>, StrErr>;
	fn set_player_kv(&mut self, id_pair :PlayerIdPair, key :&str, content :&[u8]) -> Result<(), StrErr>;
}
