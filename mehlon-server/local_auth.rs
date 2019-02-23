use rusqlite::{Connection, NO_PARAMS};
use sqlite_generic::{get_user_version, set_user_version,
	get_app_id, set_app_id, open_or_create_db};
use std::path::Path;
use StrErr;

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
			id_src INTEGER,
			id INTEGER,
			name VARCHAR(16),
			lcname VARCHAR(16),
			PRIMARY KEY(id_src, id),
			UNIQUE(lcname)
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
