use std::fmt::Write;

use colored::Colorize;
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
	#[serde(skip)]
	pub log: Option<String>,
}

#[derive(Deserialize, Debug, PartialEq)]
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
			only_cpu: false,
			verbose: false,
			quiet: false,
			file_types: Vec::new(),
			log: None,
		}
    }
}

// TODO: Just use the log crate in the lib, env_logger in the bin, or something. Don't try make a fucking logging framework as well as a file carving tool, IO framework, and the other 1e20 things I'm trying to do
// TODO: Also just read the CLI applications in rust book

impl SearchlightConfig {
	pub fn log_info(&mut self, source: impl AsRef<str>, msg: impl AsRef<str>) {
		if let Some(ref mut log) = self.log {
			log.write_str(&format!("[{}/INFO]: {}\n", source.as_ref(), msg.as_ref())).unwrap();
		} else {
			eprintln!("[{}/{}]: {}", source.as_ref(), "INFO".blue(), msg.as_ref());
		}
	}

	pub fn log_warn(&mut self, source: impl AsRef<str>, msg: impl AsRef<str>) {
		if let Some(ref mut log) = self.log {
			log.write_str(&format!("[{}/WARN]: {}\n", source.as_ref(), msg.as_ref())).unwrap();
		} else {
			eprintln!("[{}/{}]: {}", source.as_ref(), "WARN".yellow(), msg.as_ref());
		}
	}

	pub fn log_error(&mut self, source: impl AsRef<str>, msg: impl AsRef<str>) {
		if let Some(ref mut log) = self.log {
			log.write_str(&format!("[{}/ERROR]: {}\n", source.as_ref(), msg.as_ref())).unwrap();
		} else {
			eprintln!("[{}/{}]: {}", source.as_ref(), "ERROR".yellow(), msg.as_ref());
		}
	}
}

impl Default for PairingStrategy {
	fn default() -> Self {
		PairingStrategy::PairNext
	}
}