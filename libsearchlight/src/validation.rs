pub mod jpeg;
pub mod png;
pub mod zip;

use std::{collections::HashMap, ops::Range};

use crate::{search::pairing::MatchPair, searchlight::config::FileTypeId};

use self::{jpeg::JpegValidator, png::PngValidator, zip::ZipValidator};

pub trait FileValidator {
	/// Attempts to reconstruct and validate a potential file indicated by a given header-footer pair as belonging to a particular file format, decided per implementor (although there
	/// is nothing stopping one from making a master validator). This function should return a validation type, indicating the level of validity of the data (see
	/// FileValidationType variant docs for details) as well as an optional Vec listing all the fragments of the reconstructed file, in order.
	///
	/// `cluster_size` is given to aid reconstruction logic. It must not be assumed that cluster_size is any sensible value, as users can pass in anything. Additionally, a cluster size of
	/// 1 indicates that files in the image aren't allocated on cluster boundaries
	fn validate(&self, file_data: &[u8], file_match: &MatchPair, cluster_size: u64) -> FileValidationInfo;
}

pub struct FileValidationInfo {
	/// The result of validating the data - Whether it is recognised as fully present and correct, partial, corrupted, etc
	pub validation_type: FileValidationType,
	/// The fragment(s) of file content, expressed in terms of a range of indexes into the file data array, or an empty Vec if there are no recoverable fragments
	pub fragments: Vec<Range<u64>>
}

impl Default for FileValidationInfo {
	fn default() -> Self {
		FileValidationInfo {
			validation_type: FileValidationType::Unanalysed,
			fragments: Vec::new()
		}
	}
}

#[derive(Debug, PartialEq, strum::Display)]
#[strum(serialize_all = "snake_case")]
pub enum FileValidationType {
	/// Data is recognised as completely valid for the file format
	Correct,
	/// There is some data missing, but what has been recovered is correct
	Partial,
	/// Mostly correct, but the data doesn't conform to the expectations of the file format in some way(s)
	FormatError,
	/// The data is partially recognised, but there are miscellaneous/unknown errors
	Corrupt,
	/// The data does not resemble the file format it was supposed to be at all
	Unrecognised,
	/// The data has not been analysed, usually due to a missing implementation
	Unanalysed
}

impl FileValidationType {
	/// Like a min for FileValidationType, but Unrecognised and Unanalysed are treated the same, and are always the worst outcome
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
	fn validate(&self, file_data: &[u8], file_match: &MatchPair, cluster_size: u64) -> FileValidationInfo {
		if let Some(validator) = self.validators.get(&file_match.file_type.type_id) {
			validator.validate(file_data, file_match, cluster_size)
		} else {
			FileValidationInfo {
				validation_type: FileValidationType::Unanalysed,
				fragments: Vec::new()
			}
		}
	}
}