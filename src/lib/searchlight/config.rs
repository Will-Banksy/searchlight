use serde::Deserialize;

use crate::lib::error::Error;

#[derive(Deserialize, Debug)]
pub struct SearchlightConfig {
	#[serde(default)]
	pub only_cpu: bool,
	#[serde(default)]
	pub verbose: bool,
	#[serde(default)]
	pub quiet: bool,
	#[serde(rename = "file_type")]
	pub file_types: Vec<FileType>,
}

#[derive(Deserialize, Debug)]
pub struct FileType {
	pub headers: Vec<Vec<u8>>,
	#[serde(default)]
	pub footers: Vec<Vec<u8>>,
	#[serde(default)]
	pub extension: Option<String>,
	#[serde(default)]
	pub pairing: PairingStrategy,
	pub max_len: Option<u64>,
	#[serde(default)]
	pub requires_footer: bool
}

#[derive(Deserialize, Debug)]
pub enum PairingStrategy {
	#[serde(rename = "next")]
	PairNext,
	#[serde(rename = "last")]
	PairLast
}

impl SearchlightConfig {
	pub fn validate(&self) -> Result<(), Error> {
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
			only_cpu: false,
			verbose: false,
			quiet: false,
			file_types: Vec::new()
		}
    }
}

impl Default for PairingStrategy {
	fn default() -> Self {
		PairingStrategy::PairNext
	}
}