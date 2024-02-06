use crate::search::pairing::MatchPair;

use super::{FileValidationInfo, FileValidationType, FileValidator};

pub struct JpgValidator {
}

impl JpgValidator {
	pub fn new() -> Self {
		JpgValidator {}
	}
}

impl FileValidator for JpgValidator {
	fn validate(&self, file_data: &[u8], file_match: MatchPair) -> FileValidationInfo {
		// TODO: Validate JPG file

		FileValidationInfo {
			validation_type: FileValidationType::Unrecognised,
			file_len: None,
		}
	}
}