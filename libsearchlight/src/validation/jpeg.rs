use crate::{classifiers, search::{pairing::MatchPair, Match}, searchlight::config::SearchlightConfig, utils};

use super::{FileValidationInfo, FileValidationType, FileValidator, Fragment};

// const JPEG_SOI: u8 = 0xd8;
const JPEG_EOI: u8 = 0xd9;
const JPEG_APP0: u8 = 0xe0;
const JPEG_APP1: u8 = 0xe1;
const JPEG_SOF0: u8 = 0xc0;
const JPEG_SOF2: u8 = 0xc2;
const JPEG_SOS: u8 = 0xda;

pub struct JpegValidator;

enum JpegScanReconstructionInfo {
	Success {
		chunk_frags: Vec<Fragment>,
		next_chunk_idx: usize
	},
	Failure {
		failure_idx: usize
	}
}

impl JpegValidator {
	pub fn new() -> Self {
		JpegValidator
	}

	/// Attempt to reconstruct JPEG scan data, assuming that all fragments are in-order, by looping through clusters and attempting to classify them
	/// as either JPEG scan data or not
	fn reconstruct_scan_data(file_data: &[u8], scan_marker_idx: usize, cluster_size: usize, config: &SearchlightConfig) -> JpegScanReconstructionInfo {
		let fragmentation_start = utils::next_multiple_of(scan_marker_idx + 1, cluster_size) as usize;

		let mut fragments = vec![
			scan_marker_idx..fragmentation_start
		];

		let mut cluster_idx = fragmentation_start;

		loop {
			// Check we're in bounds of the reconstruction search length and file
			let search_offset = (cluster_idx + cluster_size) - scan_marker_idx;
			if search_offset > config.max_reconstruction_search_len.unwrap_or(u64::MAX) as usize || (cluster_idx + cluster_size) > file_data.len() {
				return JpegScanReconstructionInfo::Failure {
					failure_idx: cluster_idx
				}
			}

			let cluster = &file_data[cluster_idx..(cluster_idx + cluster_size)];

			let classification_info = classifiers::jpeg_data(cluster);

			match classification_info {
				(false, None) => {
					()
				}
				(true, None) => {
					fragments.push(cluster_idx..(cluster_idx + cluster_size));
				}
				(true, Some(next_marker)) => {
					fragments.push((cluster_idx)..(next_marker + cluster_idx));
					utils::simplify_ranges(&mut fragments);

					return JpegScanReconstructionInfo::Success {
						chunk_frags: fragments,
						next_chunk_idx: next_marker + cluster_idx
					}
				}
				_ => {
					assert!(false);
				}
			}

			cluster_idx += cluster_size;
		}
	}
}

impl FileValidator for JpegValidator {
	// Written using https://www.w3.org/Graphics/JPEG/jfif3.pdf,
	// https://www.w3.org/Graphics/JPEG/itu-t81.pdf and https://stackoverflow.com/questions/32873541/scanning-a-jpeg-file-for-markers
	fn validate(&self, file_data: &[u8], file_match: &MatchPair, _all_matches: &[Match], cluster_size: usize, config: &SearchlightConfig) -> FileValidationInfo {
		let start = file_match.start_idx as usize;

		// Mandatory segments for a complete JPEG file
		let mut seen_appn = false; // Whether an APP0 or APP1 segment has been found
		let mut seen_sofn = false; // Whether a SOF0 or SOF2 segment has been found

		let mut fragments = Vec::new();

		let mut i = start;
		loop {
			// Check if we are on a marker - the current byte should be 0xff and the next byte should not be 0x00
			if file_data[i] == 0xff && file_data[i + 1] != 0x00 {
				// The SOI and EOI markers don't have lengths after them - I did see someone saying that the whole range 0xd0 to 0xd9 has no lengths
				// (https://stackoverflow.com/questions/4585527/detect-end-of-file-for-jpg-images) but I can't find anything in any documentation to back
				// that up. Then again I can't see anything in any documentation to say that segments necessarily have lengths
				if (file_data[i + 1] ^ 0xd0 < 0x09) || file_data[i + 1] == 0x01 {
					// Move on to the next segment
					fragments.push(i..(i + 2));
					utils::simplify_ranges(&mut fragments);
					i += 2;
					continue;
				} else if file_data[i + 1] == JPEG_EOI {
					fragments.push(i..(i + 2 + cluster_size)); // NOTE: We're carving an extra cluster here which isn't necessary for the image but often metadata is stored past EOI so this will catch (some of) that
					utils::simplify_ranges(&mut fragments);

					// Return that this is a complete file with length start - i
					// If any of APPn and SOFn segments haven't been seen though return Format Error
					break FileValidationInfo {
						validation_type: if seen_appn && seen_sofn { FileValidationType::Correct } else { FileValidationType::FormatError },
						fragments
					}
				} else if file_data[i + 1] == JPEG_SOS {
					// Since we have no way of knowing, really, we treat the following data as if it might be fragmented
					let recons_info = Self::reconstruct_scan_data(file_data, i, cluster_size as usize, config);

					match recons_info {
						JpegScanReconstructionInfo::Success { mut chunk_frags, next_chunk_idx } => {
							fragments.append(&mut chunk_frags);
							i = next_chunk_idx;
						},
						JpegScanReconstructionInfo::Failure { failure_idx } => {
							fragments.push(i..failure_idx);

							break FileValidationInfo {
								validation_type: FileValidationType::Partial,
								fragments
							}
						}
					}
				} else {
					if file_data[i + 1] == JPEG_APP0 || file_data[i + 1] == JPEG_APP1 {
						seen_appn = true;
					} else if file_data[i + 1] == JPEG_SOF0 || file_data[i + 1] == JPEG_SOF2 {
						seen_sofn = true;
					}
					// Parse the length and skip the segment
					let segment_len = u16::from_be_bytes(file_data[(i + 2)..=(i + 3)].try_into().unwrap());

					fragments.push(i..(i + segment_len as usize + 2));
					utils::simplify_ranges(&mut fragments);

					i += segment_len as usize + 2;
					continue;
				}
			} else { // We are not on a marker - We should be. Something has gone wrong - but what, is the difficulty
				// If at least one of the mandatory markers has been seen, this is likely a partial file
				if seen_appn || seen_sofn {
					break FileValidationInfo {
						validation_type: FileValidationType::Partial,
						fragments
					};
				} else {
					break FileValidationInfo {
						validation_type: FileValidationType::Unrecognised,
						fragments
					}
				}
			}
		}
	}
}