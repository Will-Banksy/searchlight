use std::{hash::Hasher, ops::Range};

use crate::{search::pairing::MatchPair, validation::FileValidationType};

use super::{FileValidationInfo, FileValidator};

const PNG_IHDR: u32 = 0x49484452; // "IHDR" as u32
const PNG_IDAT: u32 = 0x49444154; // "IDAT" as u32
const PNG_PLTE: u32 = 0x504C5445; // "PLTE" as u32
const PNG_IEND: u32 = 0x49454E44; // "IEND" as u32

#[derive(Debug)]
enum State {
	/// Currently reading the chunk data length
	ReadingChunkLen { data_len: u32 },
	/// Currently reading the chunk type. Contained is the chunk type data currently stored, as a u32
	ReadingChunkType { data_len: u32, c_type: u32, data_crc_hasher: crc32fast::Hasher },
	/// Currently reading chunk data of the contained type. Contains the chunk type for additional validation to be done
	ReadingChunkData { data_len: u32, c_type: u32, data_crc_hasher: crc32fast::Hasher },
	/// Currently reading the crc
	ReadingCrc { c_type: u32, data_crc: u32, crc: u32 },
	/// Reached the end of the file
	EndOfFile
}

pub struct Png2Validator;

impl Png2Validator {
	pub fn new() -> Self {
		Png2Validator
	}
}

impl FileValidator for Png2Validator {
	// PERF: This way of validating file data, byte by byte, may be extremely inefficient, so in that case just do things the other, less fine-grained way
	//       Or, if there's ways of optimising this, such as calculating CRCs with 1 crc32fast::hash call, then try that, but don't waste too much time on it
	fn validate(&self, file_data: &[u8], file_match: &MatchPair, cluster_size: Option<u64>) -> FileValidationInfo {
		// Keeps track of the current state, i.e. tracks what the new byte is. Initialised to start reading the length of the IHDR chunk
		let mut state = State::ReadingChunkLen { data_len: 0 };
		// The counter sorta keeps track of the bytes "consumed"
		let mut counter: usize = 0;

		// Keeps track of the fragments - This is only populated when fragmentation is discovered
		let frags: Vec<Range<u64>> = Vec::new();

		for i in (file_match.start_idx as usize + 8)..file_data.len() {
			let validated: bool = validate_byte(&mut state, &mut counter, file_data[i]);

			if !validated {
				panic!("Not validated: state: {:#x?}", state);
			}

			match state {
				State::EndOfFile => {
					return FileValidationInfo {
						validation_type: FileValidationType::Correct,
						fragments: vec![0..(i + 1) as u64],
					}
				}
				_ => ()
			}
		}

		return FileValidationInfo {
			validation_type: FileValidationType::Partial,
			..Default::default()
		}
	}
}

/// Validates a byte according to the state, which may change according to counter,
/// which should be only incremented by this function when a valid byte is read
fn validate_byte(state: &mut State, counter: &mut usize, byte: u8) -> bool {
	let mut new_state = None;

	let validated = match state {
		State::ReadingChunkLen { data_len } => {
			*data_len <<= 8;
			*data_len ^= byte as u32;

			if *counter == 3 {
				new_state = Some(State::ReadingChunkType { data_len: *data_len, c_type: 0, data_crc_hasher: crc32fast::Hasher::new() });
			}

			true
		}
		State::ReadingChunkType { data_len, c_type, data_crc_hasher } => {
			if byte.is_ascii_alphabetic() {
				*c_type <<= 8;
				*c_type ^= byte as u32;

				data_crc_hasher.update(&[byte]);

				if *counter == 3 {
					if *data_len == 0 {
						new_state = Some(State::ReadingCrc { c_type: *c_type, data_crc: data_crc_hasher.finish() as u32, crc: 0 });
					} else {
						new_state = Some(State::ReadingChunkData { data_len: *data_len, c_type: *c_type, data_crc_hasher: data_crc_hasher.clone() })
					}
				}

				true
			} else {
				false
			}
		}
		State::ReadingChunkData { data_len, c_type, data_crc_hasher } => {
			data_crc_hasher.update(&[byte]);

			if *counter + 1 == *data_len as usize {
				new_state = Some(State::ReadingCrc { c_type: *c_type, data_crc: data_crc_hasher.finish() as u32, crc: 0 });
			}

			true
		}
		State::ReadingCrc { c_type, data_crc, crc } => {
			assert!(*counter <= 3);
			let byte_idx = 3 - *counter;

			*crc = *crc ^ ((byte as u32) << byte_idx * 8);

			if (*crc >> (byte_idx * 8)) == (*data_crc >> (byte_idx * 8)) {
				if *counter == 3 {
					if *c_type == PNG_IEND {
						new_state = Some(State::EndOfFile)
					} else {
						new_state = Some(State::ReadingChunkLen { data_len: 0 })
					}
				}

				true
			} else {
				false
			}
		}
		State::EndOfFile => {
			unimplemented!()
		}
	};

	if validated {
		*counter += 1;
	}

	if let Some(new_state) = new_state {
		*state = new_state;
		*counter = 0;
	}

	validated
}