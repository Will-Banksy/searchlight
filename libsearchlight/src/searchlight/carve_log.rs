use std::{fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::validation::{FileValidationType, Fragment};

use super::config::FileTypeId;

#[derive(Serialize, Deserialize)]
pub struct CarveLog { // NOTE: Do any other fields need to be added to this or the entry struct? This is sufficient for carving files after the log is generated at least, but other fields may be useful
	pub image_path: String,
	pub files: Vec<CarveLogEntry>
}

#[derive(Serialize, Deserialize)]
pub struct CarveLogEntry {
	pub file_type_id: FileTypeId,
	pub filename: String,
	pub validation: FileValidationType,
	pub fragments: Vec<Fragment>
}

impl CarveLog {
	pub fn new(image_path: impl Into<String>) -> Self {
		CarveLog {
			image_path: image_path.into(),
			files: Vec::new()
		}
	}

	pub fn add_entry(&mut self, file_type_id: FileTypeId, filename: String, validation: FileValidationType, fragments: Vec<Fragment>) {
		self.files.push(CarveLogEntry {
			file_type_id,
			filename,
			validation,
			fragments
		});
	}

	pub fn write(&self, dir_path: &str) -> Result<(), io::Error> {
		let mut buf = Vec::new();
		let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
		let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
		self.serialize(&mut ser).unwrap(); // This shouldn't fail... right??

		let filename: PathBuf = [ dir_path, "log.json" ].into_iter().collect();

		fs::write(filename, buf)
	}
}