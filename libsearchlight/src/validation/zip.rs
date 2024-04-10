use crate::{search::{pairing::MatchPair, Match}, searchlight::config::SearchlightConfig};

use super::{FileValidationInfo, FileValidationType, FileValidator, Fragment};

const ZIP_LOCAL_FILE_HEADER_SIG: u32 = 0x04034b50;
const ZIP_CENTRAL_DIR_HEADER_SIG: u32 = 0x02014b50;
const ZIP_DATA_DESCRIPTOR_SIG: u32 = 0x08074b50;

/// Not a constant directly of ZIP files, but the match id of the local file header signature
const ZIP_LOCAL_FILE_HEADER_SIG_ID: u64 = 0x04034b50; // TODO: Calculate this

const ZIP_LOCAL_FILE_HEADER_SIZE: usize = 30;
const ZIP_CENTRAL_DIR_HEADER_SIZE: usize = 46;
const ZIP_END_OF_CENTRAL_DIR_SIZE: usize = 22;

const ZIP_DATA_DESCRIPTOR_FLAG: u16 = 3;

pub struct ZipValidator;

// struct SegmentValidationInfo {
// 	validation_type: FileValidationType,
// 	segment_size: Option<usize>,
// 	decoded_file_segment: Option<SegmentInfo>
// }

// struct SegmentInfo {
// 	offset: usize,
// 	data_size: usize
// }

struct LocalFileValidationInfo {
	validation_type: FileValidationType,
	frags: Vec<Fragment>
}

struct CentralDirectoryFileHeader<'a> {
	crc: u32,
	compressed_size: u32,
	file_header_offset: u32,
	file_name: &'a [u8],
	extra_field: &'a [u8],
	len: usize
}

struct LocalFileHeader<'a> {
	idx: usize,
	crc: u32,
	compressed_size: u32,
	file_name: &'a [u8],
	extra_field: &'a [u8],
	offset: u32, // From CD
	len: usize
}

impl<'a> CentralDirectoryFileHeader<'a> {
	fn decode(data: &'a [u8]) -> Option<Self> {
		let signature = u32::from_le_bytes(data[0x00..0x04].try_into().unwrap());

		if signature != ZIP_CENTRAL_DIR_HEADER_SIG {
			return None;
		}

		let crc = u32::from_le_bytes(data[0x10..0x14].try_into().unwrap());
		let compressed_size = u32::from_le_bytes(data[0x14..0x18].try_into().unwrap());
		let file_name_len = u16::from_le_bytes(data[0x1c..0x1e].try_into().unwrap()) as usize;
		let extra_field_len = u16::from_le_bytes(data[0x1e..0x1f].try_into().unwrap()) as usize;
		let file_header_offset = u32::from_le_bytes(data[0x2a..0x2e].try_into().unwrap());

		let file_name = &data[0x2e..(0x2e + file_name_len)];
		let extra_field = &data[(0x2e + file_name_len)..(0x2e + file_name_len + extra_field_len)];

		Some(CentralDirectoryFileHeader {
			crc,
			compressed_size,
			file_header_offset,
			file_name,
			extra_field,
			len: ZIP_CENTRAL_DIR_HEADER_SIZE + file_name_len + extra_field_len
		})
	}

	fn same(&self, lfhdr: &LocalFileHeader) -> bool {
		// If the local file header CRC and compressed size are 0, then a data descriptor is present which contains this information instead.
		// In those cases, we're just gonna have to hope that the file name and extra field are good enough indicators
		(self.crc == lfhdr.crc || (lfhdr.crc == 0x00000000 && lfhdr.compressed_size == 0)) &&
		(self.compressed_size == lfhdr.compressed_size || (lfhdr.crc == 0x00000000 && lfhdr.compressed_size == 0)) &&
		self.file_name == lfhdr.file_name &&
		self.extra_field == lfhdr.extra_field
	}
}

impl<'a> LocalFileHeader<'a> {
	fn decode(data: &'a [u8], idx: usize) -> Option<Self> {
		let signature = u32::from_le_bytes(data[0x00..0x04].try_into().unwrap());

		if signature != ZIP_LOCAL_FILE_HEADER_SIG {
			return None;
		}

		let crc = u32::from_le_bytes(data[0x0e..0x12].try_into().unwrap());
		let compressed_size = u32::from_le_bytes(data[0x12..0x16].try_into().unwrap());
		let file_name_len = u16::from_le_bytes(data[0x1a..0x1c].try_into().unwrap()) as usize;
		let extra_field_len = u16::from_le_bytes(data[0x1c..0x1e].try_into().unwrap()) as usize;

		let file_name = &data[0x1e..(0x1e + file_name_len)];
		let extra_field = &data[(0x1e + file_name_len)..(0x1e + file_name_len + extra_field_len)];

		Some(LocalFileHeader {
			idx,
			crc,
			compressed_size,
			file_name,
			extra_field,
			offset: 0,
			len: ZIP_LOCAL_FILE_HEADER_SIZE + file_name_len + extra_field_len
		})
	}

	fn update_with(self, cdfh: &CentralDirectoryFileHeader) -> Self {
		LocalFileHeader {
			crc: cdfh.crc,
			compressed_size: cdfh.compressed_size,
			offset: cdfh.file_header_offset,
			..self
		}
	}
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
	// fn validate_segment(data: &[u8], segment_data_size: usize) -> SegmentValidationInfo {
	// 	let signature = u32::from_le_bytes(data[0..4].try_into().unwrap());

	// 	if signature == ZIP_LOCAL_FILE_HEADER_SIG {
	// 		let flags = u16::from_le_bytes(data[6..8].try_into().unwrap());

	// 		let has_data_descriptor = (flags & ZIP_DATA_DESCRIPTOR_FLAG) > 0;

	// 		let file_name_len = u16::from_le_bytes(data[26..28].try_into().unwrap());
	// 		let extra_field_len = u16::from_le_bytes(data[28..30].try_into().unwrap());

	// 		let mut next_chunk_offset = ZIP_LOCAL_FILE_HEADER_SIZE + file_name_len as usize + extra_field_len as usize;

	// 		// Calculates where the data descriptor fields start - takes data descriptor signatures into account. If a
	// 		// data descriptor does not exist then this will simply point to the end of the file data but won't actually
	// 		// be used anyway
	// 		let data_descriptor_fields_idx = {
	// 			let end_of_data = next_chunk_offset + segment_data_size;

	// 			if u32::from_le_bytes(data[end_of_data..(end_of_data + 4)].try_into().unwrap()) == ZIP_DATA_DESCRIPTOR_SIG {
	// 				end_of_data + 4
	// 			} else {
	// 				end_of_data
	// 			}
	// 		};

	// 		let crc = {
	// 			if has_data_descriptor {
	// 				u32::from_le_bytes(data[data_descriptor_fields_idx..(data_descriptor_fields_idx + 4)].try_into().unwrap())
	// 			} else {
	// 				u32::from_le_bytes(data[14..18].try_into().unwrap())
	// 			}
	// 		};

	// 		let compressed_size = {
	// 			if has_data_descriptor {
	// 				u32::from_le_bytes(data[(data_descriptor_fields_idx + 4)..(data_descriptor_fields_idx + 8)].try_into().unwrap())
	// 			} else {
	// 				u32::from_le_bytes(data[18..22].try_into().unwrap())
	// 			}
	// 		};

	// 		let mut data_corrupt = false;

	// 		if compressed_size != 0 {
	// 			let file_data = &data[next_chunk_offset..(next_chunk_offset + compressed_size as usize)];
	// 			let file_data_crc = crc32fast::hash(file_data);

	// 			if crc != file_data_crc {
	// 				data_corrupt = true;
	// 			}

	// 			next_chunk_offset += compressed_size as usize;
	// 		}

	// 		if has_data_descriptor {
	// 			next_chunk_offset += 12;
	// 		}

	// 		assert_eq!(next_chunk_offset, segment_data_size);

	// 		SegmentValidationInfo {
	// 			validation_type: if data_corrupt {
	// 				FileValidationType::Corrupt
	// 			} else {
	// 				FileValidationType::Correct
	// 			},
	// 			segment_size: None,
	// 			decoded_file_segment: None
	// 		}
	// 	} else if signature == ZIP_CENTRAL_DIR_HEADER_SIG {
	// 		let file_name_len = u16::from_le_bytes(data[28..30].try_into().unwrap()) as usize;
	// 		let extra_field_len = u16::from_le_bytes(data[30..32].try_into().unwrap()) as usize;
	// 		let comment_len = u16::from_le_bytes(data[32..34].try_into().unwrap()) as usize;

	// 		let file_seg_compressed_size = u32::from_le_bytes(data[20..24].try_into().unwrap());
	// 		let file_seg_header_offset = u32::from_le_bytes(data[20..24].try_into().unwrap());

	// 		SegmentValidationInfo {
	// 			validation_type: FileValidationType::Correct,
	// 			segment_size: Some(file_name_len + extra_field_len + comment_len + ZIP_CENTRAL_DIR_HEADER_SIZE),
	// 			decoded_file_segment: Some(SegmentInfo {
	// 				offset: file_seg_header_offset as usize,
	// 				data_size: file_seg_compressed_size as usize,
	// 			})
	// 		}
	// 	} else {
	// 		SegmentValidationInfo {
	// 			validation_type: FileValidationType::Unrecognised,
	// 			segment_size: None,
	// 			decoded_file_segment: None
	// 		}
	// 	}
	// }

	fn validate_file(file_data: &[u8], header: &LocalFileHeader, next_header_idx: usize) -> LocalFileValidationInfo {
		let data_idx = header.idx + header.len;

		let unfrag_crc = crc32fast::hash(&file_data[data_idx..(data_idx + header.compressed_size as usize)]);

		if unfrag_crc != header.crc {
			todo!(); // TODO: In this case, fragmentation is likely. Take an approach for reconstructing the file data like the PNG one. Remember to take data descriptors into account
		} else {
			todo!(); // TODO: Just return the fragment for the segment
		}
	}
}

impl FileValidator for ZipValidator {
	// Written using: https://pkwaredownloads.blob.core.windows.net/pem/APPNOTE.txt and https://users.cs.jmu.edu/buchhofp/forensics/formats/pkzip.html
	fn validate(&self, file_data: &[u8], file_match: &MatchPair, all_matches: &[Match], _cluster_size: usize, _config: &SearchlightConfig) -> FileValidationInfo {
		// Since ZIP files may have multiple headers before 1 footer, and so we can only assume that 1 footer = 1 zip file, this match pair
		// may well span the nth file in the zip to the EOCD signature. We can check the number of entries we come across however against
		// the number of entries in the central directory and if they don't match, and no other problems have been encountered, then we can
		// say it's a partial match
		// Additionally, since ZIP files are somewhat complex, this validation function will not be exhaustive, and may produce
		// incorrect output against some zip files. In particular, the following are not handled: ZIP64 files, ZIP multipart files, encrypted
		// ZIP files, ZIP files containing digital signatures

		// NOTE: Okay so new approach for dealing with ZIPs:
		//       1. Decode the central directory (Boiko and Moskalenko didn't try tackle a fragmented central directory so neither do I have to)
		//       2. Search the passed-in match list for ZIP local file headers, and find the ones with metadata corresponding to the metadata in the central directory (again not trying to tackle fragmented metadata)
		//       3. For each local file header, tackle fragmentation of that header in the same way that we would in a PNG file (we are going to, for ease, assume that the ZIP file segments are tightly packed)
		//       4. For each file, put their fragments in order of the offsets in the central directory
		//       5. As one last thing, go through the fragments and check that all the offsets are correct. If they are not, validate the ZIP as either Partial or Corrupted

		let eocd_idx = file_match.end_idx - file_match.file_type.footers[0].len() + 1;

		if (eocd_idx + ZIP_END_OF_CENTRAL_DIR_SIZE) > file_data.len() {
			return FileValidationInfo {
				validation_type: FileValidationType::Partial,
				..Default::default()
			}
		}

		let eocd_comment_len = u16::from_le_bytes(file_data[(eocd_idx + 0x14)..(eocd_idx + 0x16)].try_into().unwrap()) as usize;
		let eocd_len = eocd_comment_len + ZIP_END_OF_CENTRAL_DIR_SIZE;

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
		let cd_total_entries = u16::from_le_bytes(file_data[(eocd_idx + 10)..(eocd_idx + 12)].try_into().unwrap()); // NOTE: Do we want to make use of the total entries? Perhaps to check that the central directory is as expected?
		let cd_size = u32::from_le_bytes(file_data[(eocd_idx + 12)..(eocd_idx + 16)].try_into().unwrap()) as usize;

		// This assumes that the central directory is tightly packed and directly before the EOCD, which as far as I've read,
		// the spec doesn't specify
		let central_directory_idx = eocd_idx - cd_size;

		let central_directory = {
			let mut cd = Vec::new();

			let mut i = central_directory_idx;
			while i < eocd_idx {
				if let Some(record) = CentralDirectoryFileHeader::decode(&file_data[i..]) {
					i += record.len;
					cd.push(record);
				} // NOTE: Do we want any logic in the case that a central directory file header did not have the correct signature?
			}

			cd
		};

		let zip_header_matches: Vec<&Match> = all_matches.iter().filter(|m| m.id == ZIP_LOCAL_FILE_HEADER_SIG_ID).collect();

		let local_file_headers = {
			let mut lfhs = Vec::new();

			for m in zip_header_matches {
				if let Some(record) = LocalFileHeader::decode(&file_data[m.start_idx as usize..], m.start_idx as usize) {
					// Search the central directory for a central directory record that matches this local file header, and add information from that central directory
					// file header to the local file header
					if let Some(cdfh) = central_directory.iter().find(|cdfh| cdfh.same(&record)) {
						lfhs.push(record.update_with(cdfh));
					}
				} // NOTE: We should never come across a local file header that doesn't have the correct signature, assuming it has been set correctly in the config. That's a thing tbf, the config not really being a config in the case where we have code to handle a file type...
			}

			lfhs
		};

		let frag_eocd = eocd_idx..(eocd_idx + eocd_len);
		let frag_cd = central_directory_idx..eocd_idx;

		// TODO: Go through the local file headers and validate/reconstruct each file data segment

		todo!()

		// let eocd_idx = file_match.end_idx as usize - file_match.file_type.footers[0].len() + 1;

		// if (eocd_idx + 22) > file_data.len() {
		// 	return FileValidationInfo {
		// 		validation_type: FileValidationType::Partial,
		// 		..Default::default()
		// 	}
		// }

		// // Check the signature - we only want to handle the case of EOCD
		// let signature = &file_data[eocd_idx..(eocd_idx + 4)];
		// assert_eq!(signature, &[ 0x50, 0x4b, 0x05, 0x06 ]);

		// // Get the disk number on which this EOCD record resides, and the disk number on which the central directory starts
		// let cd_diskno = u16::from_le_bytes(file_data[(eocd_idx + 4)..(eocd_idx + 6)].try_into().unwrap());
		// let cd_start_diskno = u16::from_le_bytes(file_data[(eocd_idx + 6)..(eocd_idx + 8)].try_into().unwrap());

		// // Explicitly do not analyse the case of multi-disk/-part files
		// if cd_diskno != cd_start_diskno || cd_diskno > 0 {
		// 	return FileValidationInfo {
		// 		validation_type: FileValidationType::Unanalysed,
		// 		..Default::default()
		// 	}
		// }

		// // Get the central directory total entries and size
		// let cd_total_entries = u16::from_le_bytes(file_data[(eocd_idx + 10)..(eocd_idx + 12)].try_into().unwrap());
		// let cd_size = u32::from_le_bytes(file_data[(eocd_idx + 12)..(eocd_idx + 16)].try_into().unwrap()) as usize;

		// // This assumes that the central directory is tightly packed and directly before the EOCD, which as far as I've read,
		// // the spec doesn't specify
		// let central_directory_idx = eocd_idx - cd_size;

		// let mut curr_idx = central_directory_idx;
		// let mut file_segments: Vec<SegmentInfo> = Vec::with_capacity(cd_total_entries as usize);

		// while curr_idx < eocd_idx {
		// 	let seg_validation = Self::validate_segment(&file_data[curr_idx..], 0);

		// 	if let Some(file_segment) = seg_validation.decoded_file_segment {
		// 		file_segments.push(file_segment);
		// 	} else {
		// 		return FileValidationInfo {
		// 			validation_type: FileValidationType::Unrecognised,
		// 			..Default::default()
		// 		}
		// 	}

		// 	if let Some(segment_size) = seg_validation.segment_size {
		// 		curr_idx += segment_size;
		// 	} else {
		// 		assert!(false);
		// 	}
		// }

		// // TODO: Some calculations to determine whether the file headers that are listed in the central directory can fit between
		// // file_match.start_idx and central_directory_idx - If they can't, ZIP match is partial, and it's probably quite difficult
		// // to correct this... as we can't assume that file headers are contiguous. If we could assume that they were, and we could
		// // assume that the last file header & associated data ends directly before the central directory starts, then we could calculate
		// // the real file starting position. As we can't, well, we could try scanning backwards for file headers... And we can always try
		// // assuming the above, as that may work for most ZIP files.

		// let mut worst_validation = FileValidationType::Correct;

		// let start_of_file = file_match.start_idx as usize;

		// while let Some(file_segment) = file_segments.pop() {
		// 	let start_idx = start_of_file + file_segment.offset;
		// 	let seg_validation = Self::validate_segment(&file_data[start_idx..], file_segment.data_size);

		// 	if seg_validation.validation_type == FileValidationType::Unrecognised {
		// 		continue;
		// 	}

		// 	worst_validation = worst_validation.worst_of(seg_validation.validation_type);
		// }

		// FileValidationInfo {
		// 	validation_type: worst_validation,
		// 	..Default::default()
		// }
	}
}