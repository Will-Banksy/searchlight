use std::{fs, io, path::PathBuf};

use serde::Serialize;

use crate::validation::{FileValidationType, Fragment};

use super::config::FileTypeId;

#[derive(Serialize)]
pub struct CarveLog {
	files: Vec<CarveLogEntry>
}

#[derive(Serialize)]
pub struct CarveLogEntry {
	file_type_id: FileTypeId,
	filename: String,
	validation: FileValidationType,
	fragments: Vec<Fragment>
}

impl CarveLog {
	pub fn new() -> Self {
		CarveLog {
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

		let filename: PathBuf = [ dir_path, "log.txt" ].into_iter().collect();

		fs::write(filename, buf)
	}
}