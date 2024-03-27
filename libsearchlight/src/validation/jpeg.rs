use crate::search::pairing::MatchPair;

use super::{FileValidationInfo, FileValidationType, FileValidator};

// const JPEG_SOI: u8 = 0xd8;
const JPEG_EOI: u8 = 0xd9;
const JPEG_APP0: u8 = 0xe0;
const JPEG_APP1: u8 = 0xe1;
const JPEG_SOF0: u8 = 0xc0;
const JPEG_SOF2: u8 = 0xc2;
const JPEG_SOS: u8 = 0xda;

pub struct JpegValidator;

impl JpegValidator {
	pub fn new() -> Self {
		JpegValidator
	}
}

impl FileValidator for JpegValidator {
	// Written using https://www.w3.org/Graphics/JPEG/jfif3.pdf,
	// https://www.w3.org/Graphics/JPEG/itu-t81.pdf and https://stackoverflow.com/questions/32873541/scanning-a-jpeg-file-for-markers
	fn validate(&self, file_data: &[u8], file_match: &MatchPair) -> FileValidationInfo {
		let start = file_match.start_idx as usize;
		let end = file_match.end_idx as usize;

		// Mandatory segments for a complete JPEG file
		let mut seen_appn = false; // Whether an APP0 or APP1 segment has been found
		let mut seen_sofn = false; // Whether a SOF0 or SOF2 segment has been found

		let mut i = start;
		'outer: loop {
			// Check if we are on a marker - the current byte should be 0xff and the next byte should not be 0x00
			if file_data[i] == 0xff && file_data[i + 1] != 0x00 {
				// The SOI and EOI markers don't have lengths after them - I did see someone saying that the whole range 0xd0 to 0xd9 has no lengths
				// (https://stackoverflow.com/questions/4585527/detect-end-of-file-for-jpg-images) but I can't find anything in any documentation to back
				// that up. Then again I can't see anything in any documentation to say that segments necessarily have lengths
				if (file_data[i + 1] ^ 0xd0 < 0x09) || file_data[i + 1] == 0x01 {
					// Move on to the next segment
					i += 2;
					continue;
				} else if file_data[i + 1] == JPEG_EOI {
					// Return that this is a complete file with length start - i
					// If any of APPn and SOFn segments haven't been seen though return Format Error
					break FileValidationInfo {
						validation_type: if seen_appn && seen_sofn { FileValidationType::Correct } else { FileValidationType::FormatError },
						fragments: vec![ (file_match.start_idx..(i as u64 + 2)) ]
						// file_len: Some((i - start) as u64 + 2),
						// file_offset: None
					}
				} else if file_data[i + 1] == JPEG_SOS {
					// Helpfully, the SOS marker doesn't have the length right after it, it is just immediately followed by the entropy-coded data
					// However, the entropy-coded data puts 0x00 after any 0xffs so we can just scan for any 0xff that isn't followed by 0x00 to find
					// the next marker
					let scan_end = if let Some(max_len) = file_match.file_type.max_len {
						(start + max_len as usize).min(file_data.len() - 1)
					} else {
						file_data.len() - 1
					};

					for j in (i + 2)..scan_end {
						// Need to skip 0xff00, 0xff01, 0xffd[0-8], according to this stackoverflow answer (https://stackoverflow.com/questions/4585527/detect-end-of-file-for-jpg-images)
						// I haven't seen anything in the docs I've looked at to confirm this, but testing on images does seem to indicate that this is the correct approach
						if file_data[j] == 0xff && file_data[j + 1] != 0x00 && file_data[j + 1] != 0x01 && (file_data[j + 1] ^ 0xd0 > 0x08) {
							i = j;
							continue 'outer;
						}
					}

					break FileValidationInfo {
						validation_type: FileValidationType::Corrupt,
						..Default::default()
					}
				} else {
					if file_data[i + 1] == JPEG_APP0 || file_data[i + 1] == JPEG_APP1 {
						seen_appn = true;
					} else if file_data[i + 1] == JPEG_SOF0 || file_data[i + 1] == JPEG_SOF2 {
						seen_sofn = true;
					}
					// Parse the length and skip the segment
					let segment_len = u16::from_be_bytes(file_data[(i + 2)..=(i + 3)].try_into().unwrap());
					i += segment_len as usize + 2;
					continue;
				}
			} else { // We are not on a marker - We should be. Something has gone wrong - but what, is the difficulty
				// If at least one of the mandatory markers has been seen, this is likely a partial file, and we can return i, which will be where we got up to in decoding
				// But we'll only return i if that would take us beyond where the carver found the footer because, relying on sensible maximum file sizes, we want to carve as much data
				// as possible
				if seen_appn || seen_sofn {
					break FileValidationInfo {
						validation_type: FileValidationType::Partial,
						fragments: if i > end {
							vec![ file_match.start_idx..(i as u64) ]
						} else {
							Vec::new()
						}
					};
				} else {
					break FileValidationInfo {
						validation_type: FileValidationType::Unrecognised,
						..Default::default()
					}
				}
			}
		}
	}
}