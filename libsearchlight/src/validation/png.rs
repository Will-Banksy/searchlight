use super::{FileValidationType, FileValidator};

const PNG_IHDR: u32 = 0x49484452; // "IHDR" as u32
const PNG_IDAT: u32 = 0x49444154; // "IDAT" as u32
const PNG_PLTE: u32 = 0x504C5445; // "PLTE" as u32
const PNG_IEND: u32 = 0x49454E44; // "IEND" as u32

const PNG_IHDR_LEN: u32 = 13;

pub struct PngValidator;

struct ChunkValidationInfo {
	validation_type: FileValidationType,
	data_length: Option<u32>
}

impl PngValidator {
	pub fn new() -> Self {
		PngValidator
	}

	fn validate_chunk(requires_plte: &mut bool, plte_forbidden: &mut bool, data: &[u8]) -> ChunkValidationInfo {
		let chunk_data_len = u32::from_be_bytes(data[0..4].try_into().unwrap());
		let chunk_type = u32::from_be_bytes(data[4..8].try_into().unwrap());

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

					bit_depth_colour_type_valid && compression_method_valid && filter_method_valid && interlace_method_valid && chunk_data_len != PNG_IHDR_LEN
				};

				ChunkValidationInfo {
					validation_type: if spec_conformant && chunk_intact { FileValidationType::Correct } else if chunk_intact { FileValidationType::FormatError } else { FileValidationType::Corrupted },
					data_length: Some(chunk_data_len)
				}
			},
			PNG_PLTE => {
				let spec_conformant = chunk_data_len % 3 == 0;

				ChunkValidationInfo {
					validation_type: if spec_conformant && chunk_intact { FileValidationType::Correct } else if chunk_intact { FileValidationType::FormatError } else { FileValidationType::Corrupted },
					data_length: Some(chunk_data_len)
				}
			}
			_ => {
				ChunkValidationInfo {
					validation_type: if chunk_intact { FileValidationType::Correct } else { FileValidationType::Corrupted },
					data_length: Some(chunk_data_len),
				}
			}
		}
	}
}

impl FileValidator for PngValidator {
	// Written using https://www.w3.org/TR/png-3/
	fn validate(&self, file_data: &[u8], file_match: &crate::search::pairing::MatchPair) -> super::FileValidationInfo {
		todo!()
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