use crate::search::pairing::MatchPair;

use super::{FileValidationInfo, FileValidator};

pub struct ZipValidator;

impl ZipValidator {
	pub fn new() -> Self {
		ZipValidator
	}
}

impl FileValidator for ZipValidator {
	fn validate(&self, file_data: &[u8], file_match: &MatchPair) -> FileValidationInfo {
		todo!()
	}
}