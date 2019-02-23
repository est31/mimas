use rusqlite::{Connection, NO_PARAMS, OpenFlags};
use StrErr;
use std::path::Path;

/// Open or create a new database connection,
/// returning whether creation was needed
///
/// Returns `(conn, created)` with `conn` being a connection
/// to a possibly new db file and `created` being true if creation
/// was neccessary.
pub fn open_or_create_db(path :impl AsRef<Path> + Clone) -> Result<(Connection, bool), StrErr> {
	// SQLite doesn't tell us whether a newly opened sqlite file has been
	// existing on disk previously, or just been created.
	// Thus, we need to do two calls: first one which doesn't auto-create,
	// then one which does.

	let conn = Connection::open_with_flags(path.clone(), OpenFlags::SQLITE_OPEN_READ_WRITE);
	match conn {
		Ok(conn) => Ok((conn, false)),
		Err(rusqlite::Error::SqliteFailure(e, _))
				if e.code == libsqlite3_sys::ErrorCode::CannotOpen => {
			let conn = Connection::open(path)?;
			Ok((conn, true))
		},
		Err(v) => Err(v)?,
	}
}

pub fn get_user_version(conn :&mut Connection) -> Result<u16, StrErr> {
	let r = conn.query_row("PRAGMA user_version;", NO_PARAMS, |v| v.get(0))?;
	Ok(r)
}
pub fn set_user_version(conn :&mut Connection, version :u16) -> Result<(), StrErr> {
	// Apparently sqlite wants you to be exposed to bobby tables shit
	// because they don't allow you to use ? or other methods to avoid
	// string formatting :/.
	conn.execute(&format!("PRAGMA user_version = {};", version), NO_PARAMS)?;
	Ok(())
}
pub fn get_app_id(conn :&mut Connection) -> Result<i32, StrErr> {
	let r = conn.query_row("PRAGMA application_id;", NO_PARAMS, |v| v.get(0))?;
	Ok(r)
}
pub fn set_app_id(conn :&mut Connection, id :i32) -> Result<(), StrErr> {
	// Apparently sqlite wants you to be exposed to bobby tables shit
	// because they don't allow you to use ? or other methods to avoid
	// string formatting :/.
	conn.execute(&format!("PRAGMA application_id = {};", id), NO_PARAMS)?;
	Ok(())
}
