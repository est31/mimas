use toml::value::{Value, Datetime, Array, Table};
use StrErr;

pub trait TomlReadExt {
	fn read<T :?Sized + TomlValue>(&self, key :&str) -> Result<&T, StrErr>;
	fn convert<T :?Sized + TomlValue>(&self) -> Result<&T, StrErr>;
}

impl TomlReadExt for Value {
	fn read<T :?Sized + TomlValue>(&self, key :&str) -> Result<&T, StrErr> {
		let val = self.get(key)
			.ok_or_else(|| {
				format!("key {} not found", key)
			})?;
		let res = <T as TomlValue>::try_conversion(&val)
			.ok_or_else(|| {
				format!("expected type {}", <T as TomlValue>::TYPE_NAME)
			})?;
		Ok(res)
	}
	fn convert<T :?Sized + TomlValue>(&self) -> Result<&T, StrErr> {
		let res = <T as TomlValue>::try_conversion(self)
			.ok_or_else(|| {
				format!("expected type {}", <T as TomlValue>::TYPE_NAME)
			})?;
		Ok(res)
	}
}

pub trait TomlValue {
	const TYPE_NAME :&'static str;
	fn try_conversion(v :&Value) -> Option<&Self>;
}

impl TomlValue for str {
	const TYPE_NAME :&'static str = "string";
	fn try_conversion(v :&Value) -> Option<&Self> {
		v.as_str()
	}
}

impl TomlValue for i64 {
	const TYPE_NAME :&'static str = "integer";
	fn try_conversion(v :&Value) -> Option<&Self> {
		if let Value::Integer(v) = v {
			Some(v)
		} else {
			None
		}
	}
}

impl TomlValue for f64 {
	const TYPE_NAME :&'static str = "float";
	fn try_conversion(v :&Value) -> Option<&Self> {
		if let Value::Float(v) = v {
			Some(v)
		} else {
			None
		}
	}
}

impl TomlValue for bool {
	const TYPE_NAME :&'static str = "boolean";
	fn try_conversion(v :&Value) -> Option<&Self> {
		if let Value::Boolean(v) = v {
			Some(v)
		} else {
			None
		}
	}
}

impl TomlValue for Datetime {
	const TYPE_NAME :&'static str = "datetime";
	fn try_conversion(v :&Value) -> Option<&Self> {
		v.as_datetime()
	}
}

impl TomlValue for Array {
	const TYPE_NAME :&'static str = "array";
	fn try_conversion(v :&Value) -> Option<&Self> {
		v.as_array()
	}
}

impl TomlValue for Table {
	const TYPE_NAME :&'static str = "table";
	fn try_conversion(v :&Value) -> Option<&Self> {
		v.as_table()
	}
}
