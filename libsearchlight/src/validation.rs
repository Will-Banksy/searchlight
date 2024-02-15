pub mod jpeg;
pub mod png;
pub mod zip;

use std::{collections::HashMap, fmt::Display};

use crate::{search::pairing::MatchPair, searchlight::config::FileTypeId};

use self::{jpeg::JpegValidator, png::PngValidator, zip::ZipValidator};

// TODO: Modify to allow for validating a reconstructed fragmented file (e.g. by taking a slice of slices of the file data)
// NOTE: Could I get the validate impls to basically perform all the carving? They could just return an array of slices that are then written to the appropriate files
//       This may involve duplication of work though - But it should be fairly minimal tbh. I could maybe use some classification process to aid the validation, such
//       as the calculation of Shannon entropy for sectors/clusters, because JPEG image data is usually quite high entropy and so finding low entropy data in the middle
//       of that could indicate fragmentation and help with finding the other fragments
pub trait FileValidator {
	fn validate(&self, file_data: &[u8], file_match: &MatchPair) -> FileValidationInfo;
}

pub struct FileValidationInfo {
	pub validation_type: FileValidationType,
	pub file_len: Option<u64>,
	pub file_offset: Option<u64>
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
				(
					FileTypeId::Zip,
					Box::new(ZipValidator::new()) as Box<dyn FileValidator>
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
				file_len: None,
				file_offset: None
			}
		}
	}
}