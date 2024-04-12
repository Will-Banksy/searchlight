use std::{collections::HashMap, fmt::Display, ops::Deref};

use log::error;
use serde::{Deserialize, Serialize};

use crate::{error::Error, search::{match_id_hash_slice_u16, pairing::MatchPart}, utils::str_parse::parse_match_str};

#[derive(Deserialize, Debug)]
pub struct SearchlightConfig {
	pub max_reconstruction_search_len: Option<u64>,
	#[serde(rename = "file_type")]
	pub file_types: Vec<FileType>,
}

#[derive(Deserialize, Debug, PartialEq, Default)]
pub struct FileType { // TODO: Add minimum length, and use that minimum length when pairing
	pub headers: Vec<MatchString>,
	#[serde(default)]
	pub footers: Vec<MatchString>,
	#[serde(default)]
	pub extension: Option<String>,
	#[serde(default)]
	pub type_id: FileTypeId,
	#[serde(default)]
	pub pairing: PairingStrategy,
	pub max_len: Option<u64>,
	#[serde(default)]
	pub requires_footer: bool
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
#[serde(from = "String")]
pub struct MatchString {
	inner: Vec<u16>
}

impl From<String> for MatchString {
	fn from(value: String) -> Self {
		MatchString {
			inner: parse_match_str(&value)
		}
	}
}

impl From<&str> for MatchString {
	fn from(value: &str) -> Self {
		MatchString {
			inner: parse_match_str(&value)
		}
	}
}

impl Deref for MatchString {
	type Target = Vec<u16>;

	fn deref(&self) -> &Self::Target {
		&self.inner
	}
}

impl Display for MatchString {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let mut sb = String::new();


		for &e in &self.inner {
			if e == 0x8000 {
				sb.push('.');
			} else {
				sb.push_str(&format!("\\x{:02x}", e))
			}
		}

		write!(f, "{}", sb)
	}
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, strum::Display, Clone, Copy)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum FileTypeId {
	Unknown,
	Jpeg,
	Png,
	Zip
}

#[derive(Deserialize, Debug, PartialEq)]
pub enum PairingStrategy {
	#[serde(rename = "next")]
	PairNext,
	#[serde(rename = "last")]
	PairLast
}

impl SearchlightConfig {
	pub fn validate(&self) -> Result<(), Error> {
		let mut error = false;

		for ft in &self.file_types {
			if !ft.has_footer() && ft.max_len.is_none() {
				error!("Config: File type {} has no footers or a configured max length - Configure at least one footer or a max_len", ft.extension.clone().unwrap_or("<no extension>".to_string()));
				error = true;
			}
			if !ft.has_footer() && ft.requires_footer {
				error!("Config: File type {} has no footers but is configured to require a footer - This is an oxymoron", ft.extension.clone().unwrap_or("<no extension>".to_string()));
				error = true;
			}
		}

		let mut collision_sets: HashMap<u64, Vec<(usize, MatchPart, MatchString)>> = HashMap::new();

		// Process the file types to guarantee uniqueness between all headers and footers
		for i in 0..(self.file_types.len()) {
			for header in &self.file_types[i].headers {
				let id = match_id_hash_slice_u16(&header);
				if collision_sets.contains_key(&id) {
					// return Err(Error::ConfigValidationError(format!(
					// 	"Config: Collision detected, matches of this byte sequence may be misattributed (header: {} in type {}) - All byte sequences used in headers and footers should be unique",
					// 	header,
					// 	self.file_types[i].extension.clone().unwrap_or("<no extension>".to_string())
					// )));
					collision_sets.get_mut(&id).unwrap().push((i, MatchPart::Header, header.clone()));
					error = true;
				} else {
					collision_sets.insert(id, vec![(i, MatchPart::Header, header.clone())]);
				}
			}
			for footer in &self.file_types[i].footers {
				let id = match_id_hash_slice_u16(&footer);
				if collision_sets.contains_key(&id) {
					// return Err(Error::ConfigValidationError(format!(
					// 	"Config: Collision detected, matches of this byte sequence may be misattributed (footer: {} in type {}) - All byte sequences used in headers and footers should be unique",
					// 	footer,
					// 	self.file_types[i].extension.clone().unwrap_or("<no extension>".to_string())
					// )));
					collision_sets.get_mut(&id).unwrap().push((i, MatchPart::Footer, footer.clone()));
					error = true;
				} else {
					collision_sets.insert(id, vec![(i, MatchPart::Footer, footer.clone())]);
				}
			}
		}

		// Build report of all collisions
		for (_, collision_set) in collision_sets.iter().filter(|(_, set)| set.len() != 1) {
			let mut detail_sb = String::new();
			let mut first = true;

			detail_sb.push('(');

			for detail_str in collision_set.iter().map(|(ftype_idx, part, _)| {
				format!("{} in type {}", part, self.file_types[*ftype_idx].extension.clone().unwrap_or("<no extension>".to_string()))
			}) {
				if !first {
					detail_sb.push_str(", ");
				}
				first = false;
				detail_sb.push_str(&detail_str);
			};

			detail_sb.push(')');

			error!(
				"Config validation error: Non-unique header/footer \"{}\" {}",
				collision_set[0].2,
				detail_sb
			);
		}

		if error {
			Err(Error::ConfigValidationError)
		} else {
			Ok(())
		}
	}
}

impl FileType {
	pub fn has_footer(&self) -> bool {
		self.footers.len() != 0
	}
}

impl Default for SearchlightConfig {
    fn default() -> Self {
        Self {
			max_reconstruction_search_len: None,
			file_types: Vec::new(),
		}
    }
}

impl Default for FileTypeId {
	fn default() -> Self {
		FileTypeId::Unknown
	}
}

impl Default for PairingStrategy {
	fn default() -> Self {
		PairingStrategy::PairNext
	}
}