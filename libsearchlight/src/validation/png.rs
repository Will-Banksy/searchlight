use crate::{search::{pairing::MatchPair, Match}, searchlight::config::SearchlightConfig, utils::{self, fragments_index::FragmentsIndex}};

use super::{FileValidationInfo, FileValidationType, FileValidator, Fragment};

// List of known PNG chunks. Source: https://github.com/ImageMagick/ImageMagick/blob/main/coders/png.c
const PNG_CHUNK_TYPES: [u32; 50] = [
	u32::from_be_bytes(*b"BACK"),
	u32::from_be_bytes(*b"BASI"),
	u32::from_be_bytes(*b"bKGD"),
	u32::from_be_bytes(*b"caNv"),
	u32::from_be_bytes(*b"cHRM"),
	u32::from_be_bytes(*b"CLIP"),
	u32::from_be_bytes(*b"CLON"),
	u32::from_be_bytes(*b"DEFI"),
	u32::from_be_bytes(*b"DHDR"),
	u32::from_be_bytes(*b"DISC"),
	u32::from_be_bytes(*b"ENDL"),
	u32::from_be_bytes(*b"eXIf"),
	u32::from_be_bytes(*b"FRAM"),
	u32::from_be_bytes(*b"gAMA"),
	u32::from_be_bytes(*b"hIST"),
	u32::from_be_bytes(*b"iCCP"),
	u32::from_be_bytes(*b"IDAT"),
	u32::from_be_bytes(*b"IEND"),
	u32::from_be_bytes(*b"IHDR"),
	u32::from_be_bytes(*b"iTXt"),
	u32::from_be_bytes(*b"JdAA"),
	u32::from_be_bytes(*b"JDAA"),
	u32::from_be_bytes(*b"JDAT"),
	u32::from_be_bytes(*b"JHDR"),
	u32::from_be_bytes(*b"JSEP"),
	u32::from_be_bytes(*b"LOOP"),
	u32::from_be_bytes(*b"MAGN"),
	u32::from_be_bytes(*b"MEND"),
	u32::from_be_bytes(*b"MHDR"),
	u32::from_be_bytes(*b"MOVE"),
	u32::from_be_bytes(*b"nEED"),
	u32::from_be_bytes(*b"oFFs"),
	u32::from_be_bytes(*b"orNT"),
	u32::from_be_bytes(*b"PAST"),
	u32::from_be_bytes(*b"pHYg"),
	u32::from_be_bytes(*b"pHYs"),
	u32::from_be_bytes(*b"PLTE"),
	u32::from_be_bytes(*b"SAVE"),
	u32::from_be_bytes(*b"sBIT"),
	u32::from_be_bytes(*b"SEEK"),
	u32::from_be_bytes(*b"SHOW"),
	u32::from_be_bytes(*b"sPLT"),
	u32::from_be_bytes(*b"sRGB"),
	u32::from_be_bytes(*b"sTER"),
	u32::from_be_bytes(*b"TERM"),
	u32::from_be_bytes(*b"tEXt"),
	u32::from_be_bytes(*b"tIME"),
	u32::from_be_bytes(*b"tRNS"),
	u32::from_be_bytes(*b"vpAg"),
	u32::from_be_bytes(*b"zTXt"),
];

// Some particular PNG chunks
const PNG_IHDR: u32 = 0x49484452; // "IHDR" as u32
const PNG_IDAT: u32 = 0x49444154; // "IDAT" as u32
const PNG_PLTE: u32 = 0x504C5445; // "PLTE" as u32
const PNG_IEND: u32 = 0x49454E44; // "IEND" as u32

const PNG_IHDR_LEN: u32 = 13;

pub struct PngValidator;

struct ChunkValidationInfo {
	validation_type: FileValidationType,
	chunk_type: u32,
	chunk_frags: Vec<Fragment>,
	next_chunk_idx: Option<usize>,
}

impl ChunkValidationInfo {
	pub fn new_unfragmented(validation_type: FileValidationType, chunk_type: u32, chunk_idx: usize, data_len: u32, should_continue: bool) -> Self {
		let next_chunk_idx = chunk_idx + 12 + data_len as usize;

		ChunkValidationInfo {
			validation_type,
			chunk_type,
			chunk_frags: vec![chunk_idx..next_chunk_idx],
			next_chunk_idx: if should_continue { Some(chunk_idx + 12 + data_len as usize) } else { None }
		}
	}

	pub fn new_fragmented(validation_type: FileValidationType, chunk_type: u32, fragments: Vec<Fragment>, next_chunk_idx: Option<usize>) -> Self {
		ChunkValidationInfo {
			validation_type,
			chunk_type,
			chunk_frags: fragments,
			next_chunk_idx
		}
	}
}

enum ChunkReconstructionInfo {
	Success {
		chunk_frags: Vec<Fragment>,
		next_chunk_idx: usize
	},
	Failure
}

impl PngValidator {
	pub fn new() -> Self {
		PngValidator
	}

	/// Validates and reconstructs PNG chunk at `chunk_idx` in `file_data`, where `file_data` has a cluster size of `cluster_size`, so files can be assumed
	/// to be allocated in blocks of `cluster_size`. `chunk_idx` refers to the very start of a chunk, where a chunk is \[`len`\]\[`type`\]\[`data`\]\[`crc`\].
	fn validate_chunk(requires_plte: &mut bool, plte_forbidden: &mut bool, file_data: &[u8], chunk_idx: usize, cluster_size: usize, max_search_len: usize) -> ChunkValidationInfo {
		/// Macro to make extracting fields a bit more readable: file_data[(chunk_idx + 4)..(chunk_idx + 8)] -> chunk_data[4, 8]
		macro_rules! chunk_data {
			[$start: expr, $end: expr] => {
				file_data[(chunk_idx + $start)..(chunk_idx + $end)]
			};
		}

		let chunk_data_len = u32::from_be_bytes(chunk_data![0, 4].try_into().unwrap());
		let chunk_type = u32::from_be_bytes(chunk_data![4, 8].try_into().unwrap());

		let chunk_type_valid = Self::validate_chunk_type(&chunk_data![4, 8]);

		if !chunk_type_valid || chunk_idx + chunk_data_len as usize + 12 > file_data.len() {
			// trace!("Chunk unrecognised: type {chunk_type}")
			return ChunkValidationInfo::new_unfragmented(
				FileValidationType::Unrecognised,
				chunk_type,
				chunk_idx,
				0,
				false
			);
		}

		let unfrag_crc_offset = chunk_idx + chunk_data_len as usize + 8;

		let crc = u32::from_be_bytes(file_data[unfrag_crc_offset..(unfrag_crc_offset + 4)].try_into().unwrap());

		let calc_crc = crc32fast::hash(&chunk_data![4, 8 + chunk_data_len as usize]);

		// Collect the fragments of the chunk data to be validated, using either reconstruction techniques if possible, or in the case
		// of unfragmented chunks, just grab that range
		let (chunk_frags, next_chunk_idx) = if crc != calc_crc {
			// If the read crc and calculated CRC don't match, then unless this is a IEND chunk in which we can just say "end is here but is some is missing"
			// then we try and find the next chunk label
			// Note that we only try handle in-order fragmentations

			// If IEND, just return partial cause we're at the end anyway
			if chunk_type == PNG_IEND {
				return ChunkValidationInfo::new_unfragmented(
					FileValidationType::Partial,
					chunk_type,
					chunk_idx,
					0,
					false
				);
			}

			// Attempt to reconstruct the chunk
			let recons_info = Self::reconstruct_chunk(file_data, chunk_idx, chunk_data_len as usize, cluster_size, max_search_len);

			match recons_info {
				ChunkReconstructionInfo::Failure => {
					// If reconstruction failure, return the chunk as if it was unfragmented, with whatever data is past the chunk, and
					// give the signal to not continue reconstruction
					return ChunkValidationInfo::new_unfragmented(
						FileValidationType::Partial,
						chunk_type,
						chunk_idx,
						chunk_data_len,
						false
					);
				}
				ChunkReconstructionInfo::Success { chunk_frags, next_chunk_idx } => {
					// If success simply return the found fragments and next chunk index
					(chunk_frags, next_chunk_idx)
				}
			}
		} else {
			(
				vec![
					chunk_idx..(unfrag_crc_offset + 4)
				],
				unfrag_crc_offset + 4
			)
		};

		let chunk_data_validation = if chunk_data_len > 0 {
			// Wrap the chunk data fragments in a FragmentsIndex with the file data to be able to transparently index into fragmented chunk data,
			// then pass that to the validate_chunk_data function
			let chunk_data_indexable = FragmentsIndex::new_sliced(file_data, &chunk_frags, 8, 4);
			Self::validate_chunk_data(chunk_type, chunk_data_indexable, requires_plte, plte_forbidden)
		} else {
			true
		};

		// Return a successful chunk recovery
		ChunkValidationInfo::new_fragmented(
			if chunk_data_validation { FileValidationType::Correct } else { FileValidationType::FormatError },
			chunk_type,
			chunk_frags,
			Some(next_chunk_idx)
		)
	}

	/// Attempts to reconstruct a fragmented PNG chunk, assuming that the length, chunk type, and CRC are not fragmented and that all
	/// fragments of the chunk are in-order (limitations) by searching forwards for a valid chunk type, decoding the CRC that should occur just before it,
	/// and enumerating the possible cluster arrangements between the start of the chunk data and the decoded CRC for a matching calculated CRC
	fn reconstruct_chunk(file_data: &[u8], chunk_idx: usize, chunk_data_len: usize, cluster_size: usize, max_search_len: usize) -> ChunkReconstructionInfo {
		let unfrag_crc_offset = chunk_idx + chunk_data_len + 8;

		let mut next_chunk_type_offset = unfrag_crc_offset + 8;

		// Find the next valid chunk type
		// NOTE: Currently, we're checking against a list of known valid chunk types. This can't be exhaustive though so will miss valid chunks
		//       Perhaps an alternative method that could stop text files being counted be checking that the CRC and length are not ASCII (alphabetical?)?
		//       Course, they may be in a valid file, but are unlikely to be
		while !Self::validate_chunk_type(&file_data[next_chunk_type_offset..(next_chunk_type_offset + 4)]) {
			next_chunk_type_offset += cluster_size as usize;

			// If we're now out of bounds (or will be upon attempting to read the chunk data len) then return with failure
			if next_chunk_type_offset + 4 >= file_data.len() || next_chunk_type_offset + 4 >= max_search_len as usize { // BUG: We're still not paying attention to the max file size, butwe've got a max search len at least
				return ChunkReconstructionInfo::Failure;
			}
		}

		// Load the (what we assume is) the CRC
		let stored_crc = u32::from_be_bytes(file_data[(next_chunk_type_offset - 8)..(next_chunk_type_offset - 4)].try_into().unwrap());

		// Calculate the fragmentation points
		let fragmentation_start = utils::next_multiple_of(chunk_idx + 8, cluster_size) as usize;
		let fragmentation_end = utils::prev_multiple_of(next_chunk_type_offset - 8, cluster_size) as usize;

		// Calculate the number of clusters that were skipped, i.e. the number of irrelevant chunks
		let clusters_skipped = (next_chunk_type_offset - (unfrag_crc_offset + 8)) / cluster_size as usize;
		let clusters_needed = ((fragmentation_end - fragmentation_start) / cluster_size as usize) - clusters_skipped;

		// Some asserts to make sure our calculations are correct and assumptions are upheld as the code is written
		assert_eq!((next_chunk_type_offset - (unfrag_crc_offset + 8)) % cluster_size as usize, 0);
		assert_eq!((fragmentation_end - fragmentation_start) % cluster_size as usize, 0);

		let fragmentations = utils::generate_fragmentations(cluster_size as usize, fragmentation_start..fragmentation_end, clusters_needed);

		let mut correct_fragmentation = None;

		// Initialise CRC hasher with the chunk type, and chunk data up to the fragmentation point
		let mut hasher = crc32fast::Hasher::new();
		hasher.update(&file_data[(chunk_idx + 4)..fragmentation_start]);

		for data_frags in fragmentations {
			// Clone the hasher and hash the fragments
			let mut hasher = hasher.clone();
			for range in &data_frags {
				hasher.update(&file_data[range.start as usize..range.end as usize]);
			}

			// Finish hashing with the chunk data from the fragmentation end to the stored CRC
			hasher.update(&file_data[fragmentation_end..(next_chunk_type_offset - 8)]);

			// Then check whether the calculated CRC matches the stored one
			let calc_crc = hasher.finalize();
			if calc_crc == stored_crc {
				correct_fragmentation = Some(data_frags);
				break;
			}
		}

		if let Some(mut data_frags) = correct_fragmentation {
			data_frags.insert(0, chunk_idx..fragmentation_start);
			data_frags.push(fragmentation_end..(next_chunk_type_offset - 4));

			utils::simplify_ranges(&mut data_frags);

			ChunkReconstructionInfo::Success { chunk_frags: data_frags, next_chunk_idx: next_chunk_type_offset - 4 }
		} else {
			ChunkReconstructionInfo::Failure
		}
	}

	/// In the PNG spec, a valid chunk type must have each byte match \[a-zA-Z\]. However, this could mean that plain text files are caught,
	/// so instead of simply checking whether a chunk type is \[a-zA-Z\] we check it against a list of known PNG chunk types
	fn validate_chunk_type(chunk_type: &[u8]) -> bool {
		let chunk_type_u32 = u32::from_be_bytes(chunk_type.try_into().unwrap());
		return PNG_CHUNK_TYPES.contains(&chunk_type_u32);
	}

	fn validate_chunk_data(chunk_type: u32, data: FragmentsIndex, requires_plte: &mut bool, plte_forbidden: &mut bool) -> bool {
		let spec_conformant = match chunk_type {
			PNG_IHDR => {
				let bit_depth: u8 = data[8];
				let colour_type: u8 = data[9];
				let compression_method: u8 = data[10];
				let filter_method: u8 = data[11];
				let interlace_method: u8 = data[12];

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

		spec_conformant
	}
}

impl FileValidator for PngValidator {
	// Written using https://www.w3.org/TR/png-3/
	fn validate(&self, file_data: &[u8], file_match: &MatchPair, _all_matches: &[Match], cluster_size: usize, config: &SearchlightConfig) -> FileValidationInfo {
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

		// Initialise fragments to contain the signature
		let mut fragments: Vec<Fragment> = vec![ file_match.start_idx..(file_match.start_idx + 8) ];

		loop {
			let mut chunk_info = Self::validate_chunk(&mut requires_plte, &mut plte_forbidden, &file_data, chunk_idx, cluster_size, config.max_reconstruction_search_len.unwrap_or(u64::MAX) as usize);

			fragments.append(&mut chunk_info.chunk_frags);
			utils::simplify_ranges(&mut fragments);

			worst_chunk_validation = worst_chunk_validation.worst_of(chunk_info.validation_type);

			if worst_chunk_validation == FileValidationType::Unrecognised {
				break FileValidationInfo {
					validation_type: FileValidationType::Partial,
					fragments
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
				PNG_IEND => { // If we've reached the end of the image...
					let validation_type = {
						if seen_ihdr && seen_idat && ((!seen_plte && !requires_plte) || (seen_plte && !plte_forbidden)) && !idat_out_of_order {
							FileValidationType::Correct
						} else {
							FileValidationType::FormatError
						}
					};

					break FileValidationInfo {
						validation_type: validation_type.worst_of(worst_chunk_validation),
						fragments
					};
				}
				_ => ()
			}

			prev_chunk_type = Some(chunk_info.chunk_type);

			// If there is an available next_chunk_index from the chunk validation/reconstruction, then set the chunk_idx to that.
			// Otherwise, we shouldn't/can't continue, so exit early with validation Partial
			chunk_idx = if let Some(next_chunk_idx) = chunk_info.next_chunk_idx {
				next_chunk_idx as usize
			} else {
				break FileValidationInfo {
					validation_type: FileValidationType::Partial,
					fragments
				}
			};

			if (chunk_idx + 12) >= max_idx {
				break FileValidationInfo {
					validation_type: FileValidationType::Partial,
					fragments
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