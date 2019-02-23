use rusqlite::{Connection, NO_PARAMS};
use StrErr;

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
