use std::ops::Range;

use crate::search::pairing::MatchPair;

use super::{FileValidationInfo, FileValidationType, FileValidator};

const PNG_IHDR: u32 = 0x49484452; // "IHDR" as u32
const PNG_IDAT: u32 = 0x49444154; // "IDAT" as u32
const PNG_PLTE: u32 = 0x504C5445; // "PLTE" as u32
const PNG_IEND: u32 = 0x49454E44; // "IEND" as u32

const PNG_IHDR_LEN: u32 = 13;

pub struct PngValidator;

struct ChunkValidationInfo {
	validation_type: FileValidationType,
	data_length: u32,
	chunk_type: u32,
}

impl PngValidator {
	pub fn new() -> Self {
		PngValidator
	}

	fn validate_chunk(requires_plte: &mut bool, plte_forbidden: &mut bool, file_data: &[u8], chunk_idx: usize, cluster_size: u64) -> ChunkValidationInfo {
		// Macro to make extracting fields a bit more readable: file_data[(chunk_idx + 4)..(chunk_idx + 8)] -> chunk_data[4, 8]
		macro_rules! chunk_data {
			[$start: expr, $end: expr] => {
				file_data[(chunk_idx + $start)..(chunk_idx + $end)]
			};
		}

		let chunk_data_len = u32::from_be_bytes(chunk_data![0, 4].try_into().unwrap());
		let chunk_type = u32::from_be_bytes(chunk_data![4, 8].try_into().unwrap());

		let chunk_type_valid = Self::validate_chunk_type(&chunk_data![4, 8]);

		if !chunk_type_valid || chunk_idx + chunk_data_len as usize + 12 >= file_data.len() {
			return ChunkValidationInfo {
				validation_type: FileValidationType::Unrecognised,
				data_length: 0,
				chunk_type
			};
		}

		let unfrag_crc_offset = chunk_idx + chunk_data_len as usize + 8;

		let crc = u32::from_be_bytes(file_data[unfrag_crc_offset..(unfrag_crc_offset + 4)].try_into().unwrap());

		let calc_crc = crc32fast::hash(&chunk_data![4, 8 + chunk_data_len as usize]);

		if crc != calc_crc {
			// If the read crc and calculated CRC don't match, then unless this is a IEND chunk in which we can just say "end is here but is some is missing"
			// then we try and find the next chunk label

			// If IEND, just return partial cause we're at the end anyway
			if chunk_type == PNG_IEND {
				return ChunkValidationInfo {
					validation_type: FileValidationType::Partial,
					data_length: 0,
					chunk_type
				}
			}

			let mut next_chunk_type_offset = unfrag_crc_offset + 8;

			// Find the next valid chunk type
			// TODO: Improvements could be made, such as using a list of known valid chunk types. This can't be exhaustive though so will miss valid chunks
			while !Self::validate_chunk_type(&file_data[next_chunk_type_offset..4]) {
				next_chunk_type_offset += cluster_size as usize;

				// If we're now out of bounds (or will be upon attempting to read the chunk data len) then return with partial
				if next_chunk_type_offset + 4 >= file_data.len() { // BUG: We're not paying any attention to file max size here. We should also maybe add something additional, like max_reconstruction_search_length
					return ChunkValidationInfo {
						validation_type: FileValidationType::Partial,
						data_length: chunk_data_len,
						chunk_type
					}
				}
			}

			// TODO: We've found the next chunk type (hopefully). Now, decode the stored CRC, and find the arrangements of clusters from the fragmentation start point to this point
			//       that result in the calculated CRC matching the decoded CRC
		}

		// TODO: Do chunk validation once found fragmentation
		// let chunk_data_validation = Self::validate_chunk_data(chunk_type, data, requires_plte, plte_forbidden);

		todo!()
	}

	/// In the PNG spec, a valid chunk type must have each byte match \[a-zA-Z\] - this method
	/// checks that that is the case for a given chunk type (passed as byte slice)
	fn validate_chunk_type(chunk_type: &[u8]) -> bool {
		for b in chunk_type {
			if !b.is_ascii_alphabetic() {
				return false;
			}
		}

		return true;
	}

	fn validate_chunk_data(chunk_type: u32, data: &[u8], requires_plte: &mut bool, plte_forbidden: &mut bool) -> FileValidationType {
		let spec_conformant = match chunk_type {
			PNG_IHDR => {
				let bit_depth: u8 = data[0];
				let colour_type: u8 = data[17];
				let compression_method: u8 = data[18];
				let filter_method: u8 = data[19];
				let interlace_method: u8 = data[20];

				if colour_type == 3 {
					*requires_plte = true;
				} else if colour_type == 0 || colour_type == 4 {
					*plte_forbidden = true;
				}

				let spec_conformant = {
					// Whether the colour type and bit depth are in one of the specified valid combinations
					let bit_depth_colour_type_valid = {
							colour_type == 0 && (bit_depth == 1 || bit_depth == 2 || bit_depth == 4 || bit_depth == 8 || bit_depth == 16)
						|| ((colour_type == 2 || colour_type == 4 || colour_type == 6) && (bit_depth == 8 || bit_depth == 16))
						|| (colour_type == 3 && (bit_depth == 1 || bit_depth == 2 || bit_depth == 4 || bit_depth == 8))
					};

					let compression_method_valid = compression_method == 0;

					let filter_method_valid = filter_method == 0;

					let interlace_method_valid = interlace_method < 2;

					bit_depth_colour_type_valid && compression_method_valid && filter_method_valid && interlace_method_valid && data.len() as u32 == PNG_IHDR_LEN
				};

				spec_conformant
			},
			PNG_PLTE => {
				let spec_conformant = data.len() % 3 == 0;

				spec_conformant
			}
			_ => { // Just assume true for unknown chunks
				true
			}
		};

		if spec_conformant {
			FileValidationType::Correct
		} else {
			FileValidationType::FormatError
		}
	}

	// /// Attempts to find the next chunk by skipping forward the cluster size bytes and attempting to validate the chunk found there, up until the `max_idx`.
	// /// `fragmentation_idx` should be the point at which a chunk was expected to be found, but wasn't.
	// fn attempt_reconstruction(frags: &mut Vec<Range<u64>>, fragmentation_idx: usize, prev_chunk_idx: usize, cluster_size: Option<usize>, max_idx: usize) -> Option<usize> {
	// 	if let Some(cluster_size) = cluster_size {
	// 		let mut chunk_idx = fragmentation_idx;
	// 		chunk_idx += cluster_size;
	// 		while chunk_idx < max_idx {
	// 		}
	// 		todo!()
	// 	} else {
	// 		None
	// 	}
	// }
}

impl FileValidator for PngValidator {
	// Written using https://www.w3.org/TR/png-3/
	fn validate(&self, file_data: &[u8], file_match: &MatchPair, cluster_size: u64) -> FileValidationInfo {
		let mut chunk_idx = file_match.start_idx as usize + 8;

		let mut requires_plte = false;
		let mut plte_forbidden = false;

		let mut seen_ihdr = false;
		let mut seen_plte = false;
		let mut seen_idat = false;

		let mut idat_out_of_order = false;

		let mut prev_chunk_type = None;

		let mut worst_chunk_validation = FileValidationType::Correct;

		let max_idx = if let Some(max_len) = file_match.file_type.max_len {
			file_match.start_idx as usize + max_len as usize
		} else {
			file_data.len()
		};

		loop {
			let chunk_info = Self::validate_chunk(&mut requires_plte, &mut plte_forbidden, &file_data, chunk_idx, cluster_size);

			worst_chunk_validation = worst_chunk_validation.worst_of(chunk_info.validation_type);

			if worst_chunk_validation == FileValidationType::Unrecognised {
				break FileValidationInfo {
					validation_type: FileValidationType::Partial,
					fragments: vec![ (file_match.start_idx..(chunk_idx as u64 - 12)) ]
				}
			}

			match chunk_info.chunk_type {
				PNG_IHDR => {
					seen_ihdr = true;
				}
				PNG_PLTE => {
					seen_plte = true;
				}
				PNG_IDAT => {
					if seen_idat && !prev_chunk_type.is_some_and(|t| t == PNG_IDAT) {
						idat_out_of_order = true;
					}
					seen_idat = true;
				}
				PNG_IEND => {
					let validation_type = {
						if seen_ihdr && seen_idat && ((!seen_plte && !requires_plte) || (seen_plte && !plte_forbidden)) && !idat_out_of_order {
							FileValidationType::Correct
						} else {
							FileValidationType::FormatError
						}
					};

					break FileValidationInfo {
						validation_type: validation_type.worst_of(worst_chunk_validation),
						fragments: vec![ (file_match.start_idx..(chunk_idx as u64 + 12)) ]
					};
				}
				_ => ()
			}

			prev_chunk_type = Some(chunk_info.chunk_type);
			chunk_idx += chunk_info.data_length as usize + 12;

			if (chunk_idx + 12) >= max_idx {
				break FileValidationInfo {
					validation_type: FileValidationType::Corrupt,
					..Default::default()
				}
			}
		}
	}
}

#[cfg(test)]
mod test {
	#[test]
	fn test_crc32() {
		let ihdr_dat: [u8; 17] = [ 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x06, 0x40, 0x00, 0x00, 0x04, 0xB0, 0x08, 0x02, 0x00, 0x00, 0x00 ];

		let expected_crc = 0x2C6311C0u32;

		let calc_crc = crc32fast::hash(&ihdr_dat);

		assert_eq!(expected_crc, calc_crc);
	}
}