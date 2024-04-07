use crate::{search::pairing::MatchPair, searchlight::config::SearchlightConfig};

use super::{FileValidationInfo, FileValidationType, FileValidator};

const ZIP_LOCAL_FILE_HEADER_SIG: u32 = 0x04034b50;
const ZIP_CENTRAL_DIR_HEADER_SIG: u32 = 0x02014b50;
const ZIP_DATA_DESCRIPTOR_SIG: u32 = 0x08074b50;

const ZIP_CENTRAL_DIR_HEADER_SIZE: usize = 46;
const ZIP_LOCAL_FILE_HEADER_SIZE: usize = 30;

const ZIP_DATA_DESCRIPTOR_FLAG: u16 = 3;

pub struct ZipValidator;

struct SegmentValidationInfo {
	validation_type: FileValidationType,
	segment_size: Option<usize>,
	decoded_file_segment: Option<SegmentInfo>
}

struct SegmentInfo {
	offset: usize,
	data_size: usize
}

impl ZipValidator {
	pub fn new() -> Self {
		ZipValidator
	}

	/// Validates a segment at data\[0..\], a "segment" being a "\[local file header\]\[file data\]\[data descriptor\]"
	/// or "\[central directory header\]" - i.e. anything with a signature, excluding the end of central directory header
	/// because that is handled elsewhere and excluding a few others as they are currently unsupported (data descriptor records
	/// do have signatures but they are not enforced so cannot be relied upon to exist)
	///
	/// For "\[local file header\]\[file data\]\[data descriptor\]" segments, requires a passed-in segment_data_size that was
	/// decoded from the central directory that indicates the size of the compressed file data in order to support data descriptors.
	/// For other segments segment_data_size is ignored.
	fn validate_segment(data: &[u8], segment_data_size: usize) -> SegmentValidationInfo {
		let signature = u32::from_le_bytes(data[0..4].try_into().unwrap());

		if signature == ZIP_LOCAL_FILE_HEADER_SIG {
			let flags = u16::from_le_bytes(data[6..8].try_into().unwrap());

			let has_data_descriptor = (flags & ZIP_DATA_DESCRIPTOR_FLAG) > 0;

			let file_name_len = u16::from_le_bytes(data[26..28].try_into().unwrap());
			let extra_field_len = u16::from_le_bytes(data[28..30].try_into().unwrap());

			let mut next_chunk_offset = ZIP_LOCAL_FILE_HEADER_SIZE + file_name_len as usize + extra_field_len as usize;

			// Calculates where the data descriptor fields start - takes data descriptor signatures into account. If a
			// data descriptor does not exist then this will simply point to the end of the file data but won't actually
			// be used anyway
			let data_descriptor_fields_idx = {
				let end_of_data = next_chunk_offset + segment_data_size;

				if u32::from_le_bytes(data[end_of_data..(end_of_data + 4)].try_into().unwrap()) == ZIP_DATA_DESCRIPTOR_SIG {
					end_of_data + 4
				} else {
					end_of_data
				}
			};

			let crc = {
				if has_data_descriptor {
					u32::from_le_bytes(data[data_descriptor_fields_idx..(data_descriptor_fields_idx + 4)].try_into().unwrap())
				} else {
					u32::from_le_bytes(data[14..18].try_into().unwrap())
				}
			};

			let compressed_size = {
				if has_data_descriptor {
					u32::from_le_bytes(data[(data_descriptor_fields_idx + 4)..(data_descriptor_fields_idx + 8)].try_into().unwrap())
				} else {
					u32::from_le_bytes(data[18..22].try_into().unwrap())
				}
			};

			let mut data_corrupt = false;

			if compressed_size != 0 {
				let file_data = &data[next_chunk_offset..(next_chunk_offset + compressed_size as usize)];
				let file_data_crc = crc32fast::hash(file_data);

				if crc != file_data_crc {
					data_corrupt = true;
				}

				next_chunk_offset += compressed_size as usize;
			}

			if has_data_descriptor {
				next_chunk_offset += 12;
			}

			assert_eq!(next_chunk_offset, segment_data_size);

			SegmentValidationInfo {
				validation_type: if data_corrupt {
					FileValidationType::Corrupt
				} else {
					FileValidationType::Correct
				},
				segment_size: None,
				decoded_file_segment: None
			}
		} else if signature == ZIP_CENTRAL_DIR_HEADER_SIG {
			let file_name_len = u16::from_le_bytes(data[28..30].try_into().unwrap()) as usize;
			let extra_field_len = u16::from_le_bytes(data[30..32].try_into().unwrap()) as usize;
			let comment_len = u16::from_le_bytes(data[32..34].try_into().unwrap()) as usize;

			let file_seg_compressed_size = u32::from_le_bytes(data[20..24].try_into().unwrap());
			let file_seg_header_offset = u32::from_le_bytes(data[20..24].try_into().unwrap());

			SegmentValidationInfo {
				validation_type: FileValidationType::Correct,
				segment_size: Some(file_name_len + extra_field_len + comment_len + ZIP_CENTRAL_DIR_HEADER_SIZE),
				decoded_file_segment: Some(SegmentInfo {
					offset: file_seg_header_offset as usize,
					data_size: file_seg_compressed_size as usize,
				})
			}
		} else {
			SegmentValidationInfo {
				validation_type: FileValidationType::Unrecognised,
				segment_size: None,
				decoded_file_segment: None
			}
		}
	}
}

impl FileValidator for ZipValidator {
	// Written using: https://pkwaredownloads.blob.core.windows.net/pem/APPNOTE.txt and https://users.cs.jmu.edu/buchhofp/forensics/formats/pkzip.html
	fn validate(&self, file_data: &[u8], file_match: &MatchPair, _cluster_size: u64, _config: &SearchlightConfig) -> FileValidationInfo {
		// Since ZIP files may have multiple headers before 1 footer, and so we can only assume that 1 footer = 1 zip file, this match pair
		// may well span the nth file in the zip to the EOCD signature. We can check the number of entries we come across however against
		// the number of entries in the central directory and if they don't match, and no other problems have been encountered, then we can
		// say it's a partial match
		// Additionally, since ZIP files are somewhat complex, this validation function will not be exhaustive, and may produce
		// incorrect output against some zip files. In particular, the following are not handled: ZIP64 files, ZIP multipart files, encrypted
		// ZIP files, ZIP files containing digital signatures

		let eocd_idx = file_match.end_idx as usize - file_match.file_type.footers[0].len() + 1;

		if (eocd_idx + 22) > file_data.len() {
			return FileValidationInfo {
				validation_type: FileValidationType::Partial,
				..Default::default()
			}
		}

		// Check the signature - we only want to handle the case of EOCD
		let signature = &file_data[eocd_idx..(eocd_idx + 4)];
		assert_eq!(signature, &[ 0x50, 0x4b, 0x05, 0x06 ]);

		// Get the disk number on which this EOCD record resides, and the disk number on which the central directory starts
		let cd_diskno = u16::from_le_bytes(file_data[(eocd_idx + 4)..(eocd_idx + 6)].try_into().unwrap());
		let cd_start_diskno = u16::from_le_bytes(file_data[(eocd_idx + 6)..(eocd_idx + 8)].try_into().unwrap());

		// Explicitly do not analyse the case of multi-disk/-part files
		if cd_diskno != cd_start_diskno || cd_diskno > 0 {
			return FileValidationInfo {
				validation_type: FileValidationType::Unanalysed,
				..Default::default()
			}
		}

		// Get the central directory total entries and size
		let cd_total_entries = u16::from_le_bytes(file_data[(eocd_idx + 10)..(eocd_idx + 12)].try_into().unwrap());
		let cd_size = u32::from_le_bytes(file_data[(eocd_idx + 12)..(eocd_idx + 16)].try_into().unwrap()) as usize;

		// This assumes that the central directory is tightly packed and directly before the EOCD, which as far as I've read,
		// the spec doesn't specify
		let central_directory_idx = eocd_idx - cd_size;

		let mut curr_idx = central_directory_idx;
		let mut file_segments: Vec<SegmentInfo> = Vec::with_capacity(cd_total_entries as usize);

		while curr_idx < eocd_idx {
			let seg_validation = Self::validate_segment(&file_data[curr_idx..], 0);

			if let Some(file_segment) = seg_validation.decoded_file_segment {
				file_segments.push(file_segment);
			} else {
				return FileValidationInfo {
					validation_type: FileValidationType::Unrecognised,
					..Default::default()
				}
			}

			if let Some(segment_size) = seg_validation.segment_size {
				curr_idx += segment_size;
			} else {
				assert!(false);
			}
		}

		// TODO: Some calculations to determine whether the file headers that are listed in the central directory can fit between
		// file_match.start_idx and central_directory_idx - If they can't, ZIP match is partial, and it's probably quite difficult
		// to correct this... as we can't assume that file headers are contiguous. If we could assume that they were, and we could
		// assume that the last file header & associated data ends directly before the central directory starts, then we could calculate
		// the real file starting position. As we can't, well, we could try scanning backwards for file headers... And we can always try
		// assuming the above, as that may work for most ZIP files.

		let mut worst_validation = FileValidationType::Correct;

		let start_of_file = file_match.start_idx as usize;

		while let Some(file_segment) = file_segments.pop() {
			let start_idx = start_of_file + file_segment.offset;
			let seg_validation = Self::validate_segment(&file_data[start_idx..], file_segment.data_size);

			if seg_validation.validation_type == FileValidationType::Unrecognised {
				continue;
			}

			worst_validation = worst_validation.worst_of(seg_validation.validation_type);
		}

		FileValidationInfo {
			validation_type: worst_validation,
			..Default::default()
		}
	}
}