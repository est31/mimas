use anyhow::{anyhow, bail, Result};
use rusqlite::{Connection, OptionalExtension};
use rusqlite::types::{Value, ToSql};
use mimas_common::map::{MapChunkData, MetadataEntry, CHUNKSIZE};
use nalgebra::Vector3;
use std::{str, io, path::Path};
use byteorder::{ReadBytesExt, WriteBytesExt, BigEndian};
use flate2::{Compression, GzBuilder, read::GzDecoder};
use mimas_common::config::Config;
use toml::{from_str, to_string};
use mimas_common::sqlite_generic::{get_user_version, set_user_version,
	get_app_id, set_app_id, open_or_create_db};
use mimas_common::local_auth::SqliteLocalAuth;
use mimas_common::game_params::{NameIdMap, parse_block_name, Id};
use mimas_common::inventory::SelectableInventory;

use mimas_common::map_storage::{PlayerIdPair, DynStorageBackend, StorageBackend, NullStorageBackend};

pub struct SqliteStorageBackend {
	conn :Connection,
	ctr :u32,
}

/// Magic used to identify the mimas application.
///
/// This magic was taken from hexdump -n 32 /dev/urandom output.
const MIMAS_SQLITE_APP_ID :i32 = 0x84eeae3cu32 as i32;

const USER_VERSION :u16 = 2;

/// We group multiple writes into transactions
/// as each transaction incurs a time penalty,
/// which added up, makes having one transaction
/// per write really slow.
const WRITES_PER_TRANSACTION :u32 = 50;

fn init_db(conn :&mut Connection) -> Result<()> {
	set_app_id(conn, MIMAS_SQLITE_APP_ID)?;
	set_user_version(conn, USER_VERSION)?;
	conn.execute(
		"CREATE TABLE IF NOT EXISTS kvstore (
			kkey VARCHAR(16) PRIMARY KEY,
			content BLOB
		);",
		[],
	)?;
	conn.execute(
		"CREATE TABLE IF NOT EXISTS chunks (
			x INTEGER,
			y INTEGER,
			z INTEGER,
			content BLOB,
			PRIMARY KEY(x, y, z)
		)",
		[],
	)?;
	migrate_v2(conn)?;
	Ok(())
}

fn migrate_v2(conn :&mut Connection) -> Result<()> {
	conn.execute(
		"CREATE TABLE IF NOT EXISTS player_kvstore (
			id_src INTEGER,
			id INTEGER,
			kkey VARCHAR(16),
			content BLOB,
			PRIMARY KEY(id_src, id, kkey)
		)",
		[],
	)?;
	Ok(())
}

fn expect_user_ver(conn :&mut Connection) -> Result<()> {
	let app_id = get_app_id(conn)?;
	let user_version = get_user_version(conn)?;
	if app_id != MIMAS_SQLITE_APP_ID {
		bail!("expected app id {} but was {}",
			MIMAS_SQLITE_APP_ID, app_id);
	}
	if user_version > USER_VERSION {
		bail!("user_version of database {} newer than maximum supported {}",
			user_version, USER_VERSION);
	} else if user_version < USER_VERSION {
		migrate_v2(conn)?;
		set_user_version(conn, USER_VERSION)?;
	}
	Ok(())
}

impl SqliteStorageBackend {
	pub fn from_conn(mut conn :Connection, freshly_created :bool) -> Result<Self> {
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
	pub fn open_or_create(path :impl AsRef<Path> + Clone) -> Result<Self> {
		let (conn, freshly_created) = open_or_create_db(path)?;
		Ok(Self::from_conn(conn, freshly_created)?)
	}
	fn maybe_begin_commit(&mut self) -> Result<()> {
		if self.ctr == 0 {
			self.ctr = WRITES_PER_TRANSACTION;
			if !self.conn.is_autocommit() {
				let mut stmt = self.conn.prepare_cached("COMMIT;")?;
				stmt.execute([])?;
			}
		} else {
			self.ctr -= 1;
		}
		if self.conn.is_autocommit() {
			let mut stmt = self.conn.prepare_cached("BEGIN;")?;
			stmt.execute([])?;
		}
		Ok(())
	}
}

fn serialize_mapchunk_data(data :&MapChunkData) -> Vec<u8> {
	let mut blocks = Vec::new();
	for b in data.0.iter() {
		blocks.write_u8(b.id()).unwrap();
	}
	// TODO maybe create an error if the number doesn't fit
	blocks.write_u16::<BigEndian>(data.1.metadata.len() as u16).unwrap();
	for (pos, entry) in data.1.metadata.iter() {
		blocks.write_u8(pos[0]).unwrap();
		blocks.write_u8(pos[1]).unwrap();
		blocks.write_u8(pos[2]).unwrap();

		// TODO maybe create an error if the entries number doesn't fit
		//blocks.write_u8(entries.len() as u8).unwrap();
		blocks.write_u8(1).unwrap();
		match entry {
			MetadataEntry::Inventory(inv) => {
				// Kind 0 stands for inventories
				blocks.write_u8(0).unwrap();
				inv.serialize_to(&mut blocks);
			},
		}
	}
	let rdr :&[u8] = &blocks;
	let mut gz_enc = GzBuilder::new().read(rdr, Compression::fast());
	let mut r = Vec::<u8>::new();

	// Version
	r.write_u8(1).unwrap();
	io::copy(&mut gz_enc, &mut r).unwrap();
	r
}

fn deserialize_mapchunk_data(data :&[u8], m :&NameIdMap) -> Result<MapChunkData> {
	let mut rdr = data;
	let version = rdr.read_u8()?;
	if version > 1 {
		// The version is too recent

		bail!("Unsupported map chunk version {}", version);
	}
	let mut gz_dec = GzDecoder::new(rdr);
	let mut buffer = Vec::<u8>::new();
	io::copy(&mut gz_dec, &mut buffer)?;
	let mut rdr :&[u8] = &buffer;
	let mut r = MapChunkData::uninitialized();
	for v in r.0.iter_mut() {
		let n = rdr.read_u8()?;
		*v = m.mb_from_id(n).ok_or(anyhow!("invalid block number"))?;
	}
	if version > 0 {
		let count = rdr.read_u16::<BigEndian>()?;

		for _ in 0 .. count {
			let pos = Vector3::new(rdr.read_u8()?, rdr.read_u8()?, rdr.read_u8()?);
			// TODO bounds checking of the position vector
			let entries_count = rdr.read_u8()?;
			if entries_count > 1 {
				// For now, we only support 1 entry at most
				bail!("Too many metadata entries: {}", entries_count);
			} else if entries_count == 1 {
				let kind = rdr.read_u8()?;
				let entry = match kind {
					// 0 is for inventories
					0 => {
						let inv = SelectableInventory::deserialize_rdr(&mut rdr, m)?;
						MetadataEntry::Inventory(inv)
					},
					_ => bail!("Unsupported entry kind"),
				};
				r.1.metadata.insert(pos, entry);
			}
		}
	}
	Ok(r)
}

fn serialize_name_id_map<T :Id>(m :&NameIdMap<T>) -> Vec<u8> {
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

fn deserialize_name_id_map<T :Id>(data :&[u8]) -> Result<NameIdMap<T>> {
	use std::io::Read;
	let mut rdr = data;
	let version = rdr.read_u8()?;
	if version != 0 {
		// The version is too recent
		bail!("Unsupported name id map version {}", version);
	}
	let count = rdr.read_u16::<BigEndian>()?;
	if count >= u8::max_value() as u16 {
		// We use u8 as storage for now so we don't support
		// any counts above 255. 255 is reserved.
		bail!("Too many ids stored in name id map {}", count);
	}
	let mut res = Vec::with_capacity(count as usize);
	for _ in 0 .. count {
		let len = rdr.read_u8()? as usize;
		let mut s = vec![0; len];
		rdr.read_exact(&mut s)?;
		let name = String::from_utf8(s)?;
		// For backwards compatibility with a few
		// (unreleased) git versions that used ::
		// instead of :
		let name = name.replace("::", ":");
		// To ensure the block is correctly named
		let _components = parse_block_name(&name)?;
		res.push(name);
	}
	Ok(NameIdMap::from_name_list(res))
}

#[test]
#[cfg(test)]
fn ensure_name_id_roundtrip() {
	let nm = NameIdMap::builtin_name_list();
	let serialized = serialize_name_id_map(&nm);
	let round_trip :NameIdMap = deserialize_name_id_map(&serialized).unwrap();
	assert_eq!(nm.names(), round_trip.names());
}

impl StorageBackend for SqliteStorageBackend {
	fn store_chunk(&mut self, pos :Vector3<isize>,
			data :&MapChunkData) -> Result<()> {
		let pos = pos / CHUNKSIZE;
		let data = serialize_mapchunk_data(&data);
		self.maybe_begin_commit()?;
		let mut stmt = self.conn.prepare_cached("INSERT OR REPLACE INTO chunks (x, y, z, content) \
			VALUES (?, ?, ?, ?);")?;
		stmt.execute(&[&pos.x as &dyn ToSql, &pos.y, &pos.z, &data])?;
		Ok(())
	}
	fn tick(&mut self) -> Result<()> {
		if !self.conn.is_autocommit() {
			self.ctr = WRITES_PER_TRANSACTION;
			let mut stmt = self.conn.prepare_cached("COMMIT;")?;
			stmt.execute([])?;
		}
		Ok(())
	}
	fn load_chunk(&mut self, pos :Vector3<isize>, m :&NameIdMap) -> Result<Option<MapChunkData>> {
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
	fn get_global_kv(&mut self, key :&str) -> Result<Option<Vec<u8>>> {
		let mut stmt = self.conn.prepare_cached("SELECT content FROM kvstore WHERE kkey=?")?;
		let data :Option<Value> = stmt.query_row(
			&[&key],
			|row| row.get(0)
		).optional()?;
		Ok(value_to_vec(data)?)
	}
	fn set_global_kv(&mut self, key :&str, content :&[u8]) -> Result<()> {
		self.maybe_begin_commit()?;
		let mut stmt = self.conn.prepare_cached("INSERT OR REPLACE INTO kvstore (kkey, content) \
			VALUES (?, ?);")?;
		stmt.execute(&[&key as &dyn ToSql, &content])?;
		Ok(())
	}
	fn get_player_kv(&mut self, id_pair :PlayerIdPair, key :&str) -> Result<Option<Vec<u8>>> {
		let mut stmt = self.conn.prepare_cached("SELECT content FROM player_kvstore WHERE id_src=? AND id=? AND kkey=?")?;
		let data :Option<Value> = stmt.query_row(
			&[&(id_pair.id_src()) as &dyn ToSql, &(id_pair.id_i64()), &key],
			|row| row.get(0)
		).optional()?;
		Ok(value_to_vec(data)?)
	}
	fn set_player_kv(&mut self, id_pair :PlayerIdPair, key :&str, content :&[u8]) -> Result<()> {
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
fn value_to_vec(v :Option<Value>) -> Result<Option<Vec<u8>>> {
	Ok(match v {
		Some(Value::Text(s)) => Some(s.into_bytes()),
		Some(Value::Blob(b)) => Some(b),
		Some(_) => bail!("SQL column entry type mismatch: Blob or String required."),
		None => None,
	})
}

// This function is not generic on the backend because of a limitation of the language:
// Box<dyn Trait> does not impl Trait.
pub fn load_name_id_map(backend :&mut DynStorageBackend) -> Result<NameIdMap> {
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
pub fn save_name_id_map(backend :&mut DynStorageBackend, nm :&NameIdMap) -> Result<()> {
	let buf = serialize_name_id_map(nm);
	backend.set_global_kv("name_id_map", &buf)?;
	Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct MapgenMetaToml {
	seed :u64,
	mapgen_name :String,
}

fn load_mapgen_meta_toml<B :StorageBackend>(backend :&mut B) -> Result<Option<MapgenMetaToml>> {
	let mapgen_meta_arr = if let Some(v) = backend.get_global_kv("mapgen_meta")? {
		v
	} else {
		return Ok(None);
	};
	let mapgen_meta_str = str::from_utf8(&mapgen_meta_arr)?;
	let mapgen_meta = from_str(mapgen_meta_str)?;
	Ok(Some(mapgen_meta))
}

fn save_mapgen_meta_toml<B :StorageBackend>(backend :&mut B, m :&MapgenMetaToml) -> Result<()> {
	let mapgen_meta_str = to_string(m)?;
	backend.set_global_kv("mapgen_meta", mapgen_meta_str.as_bytes())?;
	Ok(())
}

fn manage_mapgen_meta_toml<B :StorageBackend>(backend :&mut B, config :&mut Config) -> Result<()> {
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
