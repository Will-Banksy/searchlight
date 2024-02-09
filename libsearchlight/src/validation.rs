pub mod jpeg;
pub mod png;
pub mod zip;

use std::{collections::HashMap, fmt::Display};

use crate::{search::pairing::MatchPair, searchlight::config::FileTypeId};

use self::{jpeg::JpegValidator, png::PngValidator};

// TODO: Modify to allow for validating a reconstructed fragmented file (e.g. by taking a slice of slices of the file data)
pub trait FileValidator {
	fn validate(&self, file_data: &[u8], file_match: &MatchPair) -> FileValidationInfo;
}

pub struct FileValidationInfo {
	pub validation_type: FileValidationType,
	pub file_len: Option<u64>,
}

#[derive(Debug, PartialEq)]
pub enum FileValidationType {
	Correct,
	Partial,
	FormatError,
	Corrupt,
	Unrecognised,
	Unanalysed
}

impl FileValidationType {
	pub fn worst_of(self, other: FileValidationType) -> FileValidationType {
		if self == FileValidationType::Correct {
			other
		} else if self == FileValidationType::Partial && other != FileValidationType::Correct {
			other
		} else if self == FileValidationType::FormatError && other != FileValidationType::Correct && other != FileValidationType::Partial {
			other
		} else if self == FileValidationType::Corrupt && other != FileValidationType::Correct && other != FileValidationType::Partial && other != FileValidationType::FormatError {
			other
		} else {
			self
		}
	}
}

impl Display for FileValidationType {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", match self {
			FileValidationType::Correct=>"correct",
			FileValidationType::Partial => "partial",
			FileValidationType::FormatError => "format_error",
			FileValidationType::Corrupt => "corrupted",
			FileValidationType::Unrecognised => "unrecognised",
			FileValidationType::Unanalysed => "unanalysed",
		})
	}
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
					FileTypeId::Jpeg,
					Box::new(JpegValidator::new()) as Box<dyn FileValidator>
				),
				(
					FileTypeId::Png,
					Box::new(PngValidator::new()) as Box<dyn FileValidator>
				),
			].into()
		}
	}
}

impl FileValidator for DelegatingValidator {
	fn validate(&self, file_data: &[u8], file_match: &MatchPair) -> FileValidationInfo {
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