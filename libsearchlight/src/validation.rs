pub mod jpg;

use std::collections::HashMap;

use crate::{search::pairing::MatchPair, searchlight::config::FileTypeId};

use self::jpg::JpgValidator;

// TODO: Modify to allow for validating a reconstructed fragmented file (e.g. by taking a slice of slices of the file data)
pub trait FileValidator {
	fn validate(&self, file_data: &[u8], file_match: MatchPair) -> FileValidationInfo;
}

pub struct FileValidationInfo {
	pub validation_type: FileValidationType,
	pub file_len: Option<u64>,
}

pub enum FileValidationType {
	Correct,
	Partial,
	FormatError,
	Corrupted,
	Unrecognised,
	Unanalysed
}

/// This validator, upon construction, instantiates all defined validators and when `validate` is called it will read the file type id from
/// the file match pair and delegate validation to the appropriate validator, if one is implemented for that type
pub struct DelegatingValidator {
	validators: HashMap<FileTypeId, Box<dyn FileValidator>>
}

impl DelegatingValidator {
	pub fn new() -> Self {
		DelegatingValidator {
			validators: [
				(
					FileTypeId::Jpg,
					Box::new(JpgValidator::new()) as Box<dyn FileValidator>
				)
			].into()
		}
	}
}

impl FileValidator for DelegatingValidator {
	fn validate(&self, file_data: &[u8], file_match: MatchPair) -> FileValidationInfo {
		if let Some(validator) = self.validators.get(&file_match.file_type.type_id) {
			validator.validate(file_data, file_match)
		} else {
			FileValidationInfo {
				validation_type: FileValidationType::Unanalysed,
				file_len: None
			}
		}
	}
}