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

	fn validate_chunk(requires_plte: &mut bool, plte_forbidden: &mut bool, data: &[u8]) -> ChunkValidationInfo {
		let chunk_data_len = u32::from_be_bytes(data[0..4].try_into().unwrap());
		let chunk_type = u32::from_be_bytes(data[4..8].try_into().unwrap());

		// In the PNG spec, a valid chunk type must have each byte match [a-zA-Z]
		let chunk_type_valid = chunk_type.to_ne_bytes().iter().all(|&b| (b'a' <= b && b <= b'z') || (b'A' <= b && b <= b'Z'));

		if !chunk_type_valid || chunk_data_len as usize + 12 >= data.len() {
			return ChunkValidationInfo {
				validation_type: FileValidationType::Unrecognised,
				data_length: 0,
				chunk_type
			};
		}

		let crc = u32::from_be_bytes(data[(chunk_data_len as usize + 8)..(chunk_data_len as usize + 12)].try_into().unwrap());

		let calc_crc = crc32fast::hash(&data[4..(8 + chunk_data_len as usize)]);

		let chunk_intact = crc == calc_crc;

		match chunk_type {
			PNG_IHDR => {
				let bit_depth: u8 = data[16];
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

					bit_depth_colour_type_valid && compression_method_valid && filter_method_valid && interlace_method_valid && chunk_data_len == PNG_IHDR_LEN
				};

				ChunkValidationInfo {
					validation_type: if spec_conformant && chunk_intact { FileValidationType::Correct } else if chunk_intact { FileValidationType::FormatError } else { FileValidationType::Corrupt },
					data_length: chunk_data_len,
					chunk_type
				}
			},
			PNG_PLTE => {
				let spec_conformant = chunk_data_len % 3 == 0;

				ChunkValidationInfo {
					validation_type: if spec_conformant && chunk_intact { FileValidationType::Correct } else if chunk_intact { FileValidationType::FormatError } else { FileValidationType::Corrupt },
					data_length: chunk_data_len,
					chunk_type
				}
			}
			_ => {
				ChunkValidationInfo {
					validation_type: if chunk_intact { FileValidationType::Correct } else { FileValidationType::Corrupt },
					data_length: chunk_data_len,
					chunk_type
				}
			}
		}
	}
}

impl FileValidator for PngValidator {
	// Written using https://www.w3.org/TR/png-3/
	fn validate(&self, file_data: &[u8], file_match: &MatchPair) -> FileValidationInfo {
		let mut chunk_idx = file_match.start_idx as usize + 8;

		let mut requires_plte = false;
		let mut plte_forbidden = false;

		let mut seen_ihdr = false;
		let mut seen_plte = false;
		let mut seen_idat = false;

		let mut idat_out_of_order = false;

		let mut prev_chunk_type = None;

		let mut worst_chunk_validation = FileValidationType::Correct;

		loop {
			let chunk_info = Self::validate_chunk(&mut requires_plte, &mut plte_forbidden, &file_data[chunk_idx..]);

			worst_chunk_validation = worst_chunk_validation.worst_of(chunk_info.validation_type);

			if worst_chunk_validation == FileValidationType::Unrecognised {
				break FileValidationInfo {
					validation_type: FileValidationType::Partial,
					file_len: Some(chunk_idx as u64 - file_match.start_idx + 12),
					file_offset: None
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
						file_len: Some(chunk_idx as u64 - file_match.start_idx + 12),
						file_offset: None
					};
				}
				_ => ()
			}

			prev_chunk_type = Some(chunk_info.chunk_type);
			chunk_idx += chunk_info.data_length as usize + 12;

			let max_idx = if let Some(max_len) = file_match.file_type.max_len {
				file_match.start_idx as usize + max_len as usize
			} else {
				file_data.len()
			};
			if (chunk_idx + 12) >= max_idx {
				break FileValidationInfo {
					validation_type: FileValidationType::Corrupt,
					file_len: None,
					file_offset: None
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