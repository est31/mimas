use toml::value::{Value, Datetime, Array, Table};
use crate::StrErr;

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
				format!("expected type {} for {}",
					<T as TomlValue>::TYPE_NAME, key)
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

impl TomlValue for Value {
	const TYPE_NAME :&'static str = "VALUE";
	fn try_conversion(v :&Value) -> Option<&Self> {
		Some(v)
	}
}

macro_rules! impl_toml_value {
	($ty:ty, $name:literal, $variant:ident) => {
		impl TomlValue for $ty {
			const TYPE_NAME :&'static str = $name;
			fn try_conversion(v :&Value) -> Option<&Self> {
				if let Value::$variant(v) = v {
					Some(v)
				} else {
					None
				}
			}
		}
	};
}

impl_toml_value!(str, "string", String);
impl_toml_value!(i64, "integer", Integer);
impl_toml_value!(f64, "float", Float);
impl_toml_value!(bool, "bool", Boolean);
impl_toml_value!(Datetime, "datetime", Datetime);
impl_toml_value!(Array, "array", Array);
impl_toml_value!(Table, "table", Table);
