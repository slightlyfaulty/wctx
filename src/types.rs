use std::collections::HashMap;
use std::num::ParseIntError;
use std::str::FromStr;
use serde::{Deserialize, Serialize};
use strum::VariantNames;
use zbus::fdo;
use zbus::zvariant::{Type, Value};

pub type DictMap<'a> = HashMap<String, Value<'a>>;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[zvariant(signature = "s")]
pub enum WindowContext {
	Both,
	Active,
	Pointer,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, strum::Display, strum::EnumString, strum::VariantNames)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum WindowType {
	#[serde(rename = "")]
	#[strum(to_string = "")]
	None,
	#[default]
	Normal,
	Combo,
	Desktop,
	Dialog,
	#[serde(rename = "DND")]
	DND,
	Dock,
	DropdownMenu,
	Menu,
	Notification,
	PopupMenu,
	Splash,
	Toolbar,
	Tooltip,
	Utility,
	Override, // GNOME non-standard
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, strum::Display, strum::EnumString, strum::VariantNames)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum WindowState {
	#[serde(rename = "")]
	#[strum(to_string = "")]
	None,
	#[default]
	Normal,
	Maximized,
	Fullscreen,
}

#[derive(Clone, Debug, Serialize)]
pub struct WindowDict {
	pub id: String,
	pub name: String,
	pub class: String,
	pub pid: u32,
	pub title: String,
	pub r#type: WindowType,
	pub role: String,
	pub state: WindowState,
	pub display: String,
}

impl WindowDict {
	const FIELDS: &'static [&'static str] = &[
		"id",
		"name",
		"class",
		"pid",
		"title",
		"type",
		"role",
		"state",
		"display",
	];
	
	pub fn new(
		id: &str,
		name: &str,
		class: &str,
		pid: u32,
		title: &str,
		r#type: WindowType,
		role: &str,
		state: WindowState,
		display: &str,
	) -> Self {
		Self {
			id: id.into(),
			name: name.into(),
			class: class.into(),
			pid,
			title: title.into(),
			r#type,
			role: role.into(),
			state,
			display: display.into(),
		}
	}

	pub fn as_map(&self) -> DictMap {
		HashMap::from([
			("id".to_string(), Value::from(&self.id)),
			("name".to_string(), Value::from(&self.name)),
			("class".to_string(), Value::from(&self.class)),
			("pid".to_string(), Value::from(&self.pid)),
			("title".to_string(), Value::from(&self.title)),
			("type".to_string(), Value::from(self.r#type.to_string())),
			("role".to_string(), Value::from(&self.role)),
			("state".to_string(), Value::from(self.state.to_string())),
			("display".to_string(), Value::from(&self.display)),
		])
	}

	pub fn update(&mut self, key: &str, value: &str) -> fdo::Result<()> {
		match key {
			"id" => self.id = value.into(),
			"name" => self.name = value.into(),
			"class" => self.class = value.into(),
			"pid" => self.pid = parse_int_string(value).map_err(|_| fdo::Error::InvalidArgs(format!("Expected integer value for `{}`", key)))?,
			"title" => self.title = value.into(),
			"type" => self.r#type = WindowType::from_str(value).map_err(|_| fdo::Error::InvalidArgs(format!("Expected valid value for `{}` (\"\"{})", key, WindowType::VARIANTS.join(", "))))?,
			"role" => self.role = value.into(),
			"state" => self.state = WindowState::from_str(value).map_err(|_| fdo::Error::InvalidArgs(format!("Expected valid value for `{}` (\"\"{})", key, WindowState::VARIANTS.join(", "))))?,
			"display" => self.display = value.into(),
			_ => {
				return Err(fdo::Error::InvalidArgs(format!("Key must be one of: {}", Self::FIELDS.join(", "))));
			}
		}

		Ok(())
	}
}

impl Default for WindowDict {
	fn default() -> Self {
		Self {
			id: Default::default(),
			name: Default::default(),
			class: Default::default(),
			pid: Default::default(),
			title: Default::default(),
			r#type: WindowType::None,
			role: Default::default(),
			state: WindowState::None,
			display: Default::default(),
		}
	}
}

impl TryFrom<DictMap<'_>> for WindowDict {
	type Error = fdo::Error;

	fn try_from(map: DictMap) -> Result<Self, Self::Error> {
		Ok(Self {
			id: map.extract("id")?,
			name: map.extract("name")?,
			class: map.extract("class")?,
			pid: map.extract("pid")?,
			title: map.extract("title")?,
			r#type: map.extract("type")?,
			role: map.extract("role")?,
			state: map.extract("state")?,
			display: map.extract("display")?,
		})
	}
}

impl<'a> Into<DictMap<'a>> for WindowDict {
	fn into(self) -> DictMap<'a> {
		HashMap::from([
			("id".to_string(), Value::from(self.id)),
			("name".to_string(), Value::from(self.name)),
			("class".to_string(), Value::from(self.class)),
			("pid".to_string(), Value::from(self.pid)),
			("title".to_string(), Value::from(self.title)),
			("type".to_string(), Value::from(self.r#type.to_string())),
			("role".to_string(), Value::from(self.role)),
			("state".to_string(), Value::from(self.state.to_string())),
			("display".to_string(), Value::from(self.display)),
		])
	}
}

trait ValueExt<T> {
	fn extract(&self, key: &str) -> fdo::Result<T>;
}

impl ValueExt<String> for DictMap<'_> {
	fn extract(&self, key: &str) -> fdo::Result<String> {
		match self.get(key) {
			Some(v) => String::try_from(v)
				.map_err(|_| fdo::Error::InvalidArgs(format!("Expected string value for `{}`", key))),
			None => Ok(String::default()),
		}
	}
}

impl ValueExt<u32> for DictMap<'_> {
	fn extract(&self, key: &str) -> fdo::Result<u32> {
		match self.get(key) {
			Some(v) => match i32::try_from(v) {
				Ok(v) => Ok(0u32.saturating_add_signed(v)),
				Err(_) => u32::try_from(v)
					.map_err(|_| fdo::Error::InvalidArgs(format!("Expected integer value for `{}`", key))),
			},
			None => Ok(u32::default()),
		}
	}
}

macro_rules! impl_from_str_enum {
    ($type:ty) => {
        impl ValueExt<$type> for DictMap<'_> {
            fn extract(&self, key: &str) -> fdo::Result<$type> {
                let s: String = self.extract(key)?;
                <$type>::from_str(&s).map_err(|_| fdo::Error::InvalidArgs(format!("Expected valid value for `{}` ({})", key, <$type>::VARIANTS.join(", "))))
            }
        }
    };
}

impl_from_str_enum!(WindowType);
impl_from_str_enum!(WindowState);

fn parse_int_string(value: &str) -> Result<u32, ParseIntError> {
	if value == "" {
		Ok(0)
	} else {
		value.parse::<u32>()
	}
}
