use rusqlite::{Connection, NO_PARAMS, OptionalExtension};
use rusqlite::types::ToSql;
use sqlite_generic::{get_user_version, set_user_version,
	get_app_id, set_app_id, open_or_create_db};
use std::path::Path;
use StrErr;
use map_storage::PlayerIdPair;

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
			pwhash BLOB
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

pub struct PlayerPwHash {
	pub data :Vec<u8>,
}

pub trait AuthBackend {
	fn get_player_id(&mut self, name :&str, src :u8) -> Result<Option<PlayerIdPair>, StrErr>;
	fn get_player_name(&mut self, id :PlayerIdPair) -> Result<Option<String>, StrErr>;
	fn get_player_pwh(&mut self, id :PlayerIdPair) -> Result<Option<PlayerPwHash>, StrErr>;
	fn set_player_pwh(&mut self, id :PlayerIdPair, pwh :PlayerPwHash) -> Result<(), StrErr>;
	fn add_player(&mut self, name :&str, pwh: PlayerPwHash) -> Result<(), StrErr>;
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
		})
	}
	fn get_player_name(&mut self, id :PlayerIdPair) -> Result<Option<String>, StrErr> {
		let mut stmt = self.conn.prepare_cached("SELECT name FROM player_name_id_map WHERE id=?")?;
		let name :Option<String> = stmt.query_row(
			&[&(id.id_src()) as &dyn ToSql],
			|row| row.get(0)
		).optional()?;
		Ok(name)
	}
	fn get_player_pwh(&mut self, id :PlayerIdPair) -> Result<Option<PlayerPwHash>, StrErr> {
		let mut stmt = self.conn.prepare_cached("SELECT pwhash FROM player_pw_hashes WHERE id_src=? AND id=?")?;
		let pwh :Option<Vec<u8>> = stmt.query_row(
			&[&(id.id_src()) as &dyn ToSql, &(id.id_i64())],
			|row| row.get(0)
		).optional()?;
		Ok(pwh.map(|p| PlayerPwHash {
			data : p,
		}))
	}
	fn set_player_pwh(&mut self, id :PlayerIdPair, pwh :PlayerPwHash) -> Result<(), StrErr> {
		let mut stmt = self.conn.prepare_cached("UPDATE player_pw_hashes SET pwhash=? WHERE id_src=? AND id=?")?;
		stmt.execute(&[&pwh.data as &dyn ToSql, &(id.id_src()), &(id.id_i64())])?;
		Ok(())
	}
	fn add_player(&mut self, name :&str, pwh: PlayerPwHash) -> Result<(), StrErr> {
		let name_lower = name.to_lowercase();
		let mut stmt = self.conn.prepare_cached("INSERT INTO player_name_id_map (name, lcname) \
			VALUES (?, ?);")?;
		stmt.execute(&[&name as &dyn ToSql, &name_lower])?;
		let mut stmt = self.conn.prepare_cached("SELECT id FROM player_name_id_map WHERE name=?")?;
		let id :i64 = stmt.query_row(&[&name], |row| row.get(0))?;
		let mut stmt = self.conn.prepare_cached("INSERT INTO player_pw_hashes (id, pwhash) \
			VALUES (?, ?);")?;
		stmt.execute(&[&id as &dyn ToSql, &pwh.data])?;
		Ok(())
	}
}
