use rusqlite::{Connection, NO_PARAMS, OptionalExtension};
use rusqlite::types::ToSql;
use crate::sqlite_generic::{get_user_version, set_user_version,
	get_app_id, set_app_id, open_or_create_db};
use argon2::Config;
use std::path::Path;
use rand::Rng;
use crate::StrErr;
use crate::map_storage::PlayerIdPair;

/// Magic used to identify the mehlon application.
///
/// This magic was taken from hexdump -n 32 /dev/urandom output.
const MEHLON_LOCALAUTH_APP_ID :i32 = 0x7bb612f as i32;

const USER_VERSION :u16 = 1;

fn init_db(conn :&mut Connection) -> Result<(), StrErr> {
	set_app_id(conn, MEHLON_LOCALAUTH_APP_ID)?;
	set_user_version(conn, USER_VERSION)?;
	conn.execute(
		"CREATE TABLE IF NOT EXISTS player_name_id_map (
			id INTEGER PRIMARY KEY AUTOINCREMENT,
			name VARCHAR(16),
			lcname VARCHAR(16),
			UNIQUE(lcname)
		)",
		NO_PARAMS,
	)?;
	conn.execute(
		"CREATE TABLE IF NOT EXISTS player_pw_hashes (
			id INTEGER PRIMARY KEY,
			pwhash VARCHAR(16)
		)",
		NO_PARAMS,
	)?;
	Ok(())
}

fn expect_user_ver(conn :&mut Connection) -> Result<(), StrErr> {
	let app_id = get_app_id(conn)?;
	let user_version = get_user_version(conn)?;
	if app_id != MEHLON_LOCALAUTH_APP_ID {
		Err(format!("expected app id {} but was {}",
			MEHLON_LOCALAUTH_APP_ID, app_id))?;
	}
	if user_version > USER_VERSION {
		Err(format!("user_version of database {} newer than maximum supported {}",
			user_version, USER_VERSION))?;
	} else if user_version < USER_VERSION {
		// TODO if format of the db changes,
		// remove the error below and put any migration code here
		Err(format!("user_version {} is too old", user_version))?;
	}
	Ok(())
}

pub struct SqliteLocalAuth {
	conn :Connection,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct PlayerPwHash {
	params :HashParams,
	hash :Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct HashParams {
	salt :Vec<u8>,
}

const PARAMS :&str = "$argon2id$v=19$m=4096,t=4,p=1$";

impl PlayerPwHash {
	pub fn deserialize(data :String) -> Result<Self, StrErr> {
		if !data.starts_with(PARAMS) {
			Err("Deserialization of player pw hash params not implemented yet!")?;
		}
		let salt_hash = &data[PARAMS.len()..];
		match salt_hash.split("$").collect::<Vec<_>>()[..] {
			[salt_enc, hash_enc] => Ok(PlayerPwHash {
				params : HashParams {
					salt : base64::decode(salt_enc)?,
				},
				hash : base64::decode(hash_enc)?,
			}),
			_ => Err("player pw hash lacks salt or hash")?,
		}
	}
	pub fn serialize(&self) -> String {
		let mut s = "".to_string();
		self.params.serialize(&mut s);
		s += "$";
		s += &base64::encode(&self.hash);
		return s;
	}
	pub fn hash(&self) -> &[u8] {
		&self.hash
	}
	pub fn params(&self) -> &HashParams {
		&self.params
	}
	pub fn hash_password(pw :&str, params :HashParams) -> Result<Self, StrErr> {
		let hash = {
			let config = params.get_argon2_config();

			//let i = std::time::Instant::now();
			let hash = argon2::hash_raw(pw.as_bytes(), &params.salt, &config)?;
			//println!("hashing took {:?}", (std::time::Instant::now() - i));
			hash
		};

		Ok(Self {
			params,
			hash,
		})
	}
}

impl HashParams {
	pub fn random() -> Self {
		let mut rng = rand::thread_rng();
		let mut salt = vec![0; 8];
		rng.fill(&mut salt[..]);
		HashParams {
			salt,
		}
	}
	fn serialize(&self, s :&mut String) {
		*s += PARAMS;
		*s += &base64::encode(&self.salt);
	}
	fn get_argon2_config(&self) -> Config {
		Config {
			ad : &[],
			hash_length : 32,
			lanes : 1,
			mem_cost : 4096,
			secret : &[],
			thread_mode : argon2::ThreadMode::Sequential,
			time_cost : 4,
			variant : argon2::Variant::Argon2id,
			version : argon2::Version::Version13,
		}
	}
}

pub trait AuthBackend {
	fn get_player_id(&mut self, name :&str, src :u8) -> Result<Option<PlayerIdPair>, StrErr>;
	fn get_player_name(&mut self, id :PlayerIdPair) -> Result<Option<String>, StrErr>;
	fn get_player_pwh(&mut self, id :PlayerIdPair) -> Result<Option<PlayerPwHash>, StrErr>;
	fn set_player_pwh(&mut self, id :PlayerIdPair, pwh :PlayerPwHash) -> Result<(), StrErr>;
	fn add_player(&mut self, name :&str, pwh: PlayerPwHash, id_src :u8)
		-> Result<PlayerIdPair, StrErr>;
}

impl SqliteLocalAuth {
	pub fn from_conn(mut conn :Connection, freshly_created :bool) -> Result<Self, StrErr> {
		if freshly_created {
			init_db(&mut conn)?;
		} else {
			expect_user_ver(&mut conn)?;
		}

		Ok(Self {
			conn,
		})
	}
	pub fn open_or_create(path :impl AsRef<Path> + Clone) -> Result<Self, StrErr> {
		let (conn, freshly_created) = open_or_create_db(path)?;
		Ok(Self::from_conn(conn, freshly_created)?)
	}
}

impl AuthBackend for SqliteLocalAuth {
	fn get_player_id(&mut self, name :&str, src :u8)
			-> Result<Option<PlayerIdPair>, StrErr> {
		let name_lower = name.to_lowercase();
		let mut stmt = self.conn.prepare_cached("SELECT id FROM player_name_id_map WHERE name=?")?;
		let id :Option<i64> = stmt.query_row(
			&[&name_lower], |row| row.get(0)
		).optional()?;
		Ok(id.map(|id| {
			PlayerIdPair::from_components(src, id as u64)
		}))
	}
	fn get_player_name(&mut self, id :PlayerIdPair) -> Result<Option<String>, StrErr> {
		let mut stmt = self.conn.prepare_cached("SELECT name FROM player_name_id_map WHERE id=?")?;
		let name :Option<String> = stmt.query_row(
			&[&(id.id_i64())],
			|row| row.get(0)
		).optional()?;
		Ok(name)
	}
	fn get_player_pwh(&mut self, id :PlayerIdPair) -> Result<Option<PlayerPwHash>, StrErr> {
		let mut stmt = self.conn.prepare_cached("SELECT pwhash FROM player_pw_hashes WHERE id=?")?;
		let pwh :Option<String> = stmt.query_row(
			&[&(id.id_i64())],
			|row| row.get(0)
		).optional()?;
		Ok(if let Some(p) = pwh {
			Some(PlayerPwHash::deserialize(p)?)
		} else {
			None
		})
	}
	fn set_player_pwh(&mut self, id :PlayerIdPair, pwh :PlayerPwHash) -> Result<(), StrErr> {
		let mut stmt = self.conn.prepare_cached("UPDATE player_pw_hashes SET pwhash=? WHERE id=?")?;
		stmt.execute(&[&pwh.serialize() as &dyn ToSql, &(id.id_i64())])?;
		Ok(())
	}
	fn add_player(&mut self, name :&str, pwh: PlayerPwHash, id_src :u8)
			-> Result<PlayerIdPair, StrErr> {
		let name_lower = name.to_lowercase();
		let mut stmt = self.conn.prepare_cached("INSERT INTO player_name_id_map (name, lcname) \
			VALUES (?, ?);")?;
		stmt.execute(&[&name as &dyn ToSql, &name_lower])?;
		let mut stmt = self.conn.prepare_cached("SELECT id FROM player_name_id_map WHERE name=?")?;
		let id :i64 = stmt.query_row(&[&name], |row| row.get(0))?;
		let mut stmt = self.conn.prepare_cached("INSERT INTO player_pw_hashes (id, pwhash) \
			VALUES (?, ?);")?;
		stmt.execute(&[&id as &dyn ToSql, &pwh.serialize()])?;
		let id_pair = PlayerIdPair::from_components(id_src, id as u64);
		Ok(id_pair)
	}
}
