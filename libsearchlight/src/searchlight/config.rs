use serde::Deserialize;

use crate::error::Error;

#[derive(Deserialize, Debug)]
pub struct SearchlightConfig {
	#[serde(rename = "file_type")]
	pub file_types: Vec<FileType>,
}

#[derive(Deserialize, Debug, PartialEq, Default)]
pub struct FileType {
	pub headers: Vec<Vec<u8>>,
	#[serde(default)]
	pub footers: Vec<Vec<u8>>,
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

#[derive(Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum FileTypeId {
	Unknown,
	Jpeg,
}

#[derive(Deserialize, Debug, PartialEq)]
pub enum PairingStrategy {
	#[serde(rename = "next")]
	PairNext,
	#[serde(rename = "last")]
	PairLast
}

impl SearchlightConfig {
	pub fn validate(&self) -> Result<(), Error> { // TODO: Check for hash collisions. The id_ftype_map does, but it should ideally be caught earlier. Also, I could build the id_ftype_map here and store it in the config maybe
		for ft in &self.file_types {
			if !ft.has_footer() && ft.max_len.is_none() {
				return Err(Error::ConfigValidationError(format!("File type {} has no footers or a configured max length - Configure at least one footer or a max_len", ft.extension.clone().unwrap_or("<no extension>".to_string()))));
			}
			if !ft.has_footer() && ft.requires_footer {
				return Err(Error::ConfigValidationError(format!("File type {} has no footers but is configured to require a footer - This is an oxymoron", ft.extension.clone().unwrap_or("<no extension>".to_string()))));
			}
		}

		Ok(())
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