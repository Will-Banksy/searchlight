const ENTROPY_THRESHOLD: f32 = 0.6;
const FF00_THRESHOLD: u32 = 0; // Larger values seem to cause problems, especially for smaller cluster sizes
const FF00_CERTAINTY_THRESHOLD: u32 = 4;

/// Calculate the Shannon entropy of a slice
fn shannon_entropy(data: &[u8]) -> f32 {
	// Can't calculate the entropy without data so return 0. Would panic otherwise
	if data.len() == 0 {
		return 0.0;
	}

	// Count the values
	let mut counts = [0u32; 256];
	for &byte in data {
		counts[byte as usize] += 1;
	}

	// And calculate the entropy
	let mut entropy = 0.0;
	for count in counts {
		if count != 0 {
			let probability = (count as f32) / (data.len() as f32);
			entropy -= probability * probability.log2();
		}
	}

	entropy
}

/// Attempts to classify a cluster of file data as JPEG scan data or not, by calculating the Shannon entropy
/// and comparing it to a threshold (currently of 0.6), and by doing some analysis on the bytes to check
/// whether 0xff's are followed by valid bytes in a JPEG-compressed datastream, also checking that if RST
/// markers are present that they are correctly ordered. Also counts the number of 0xff00's, and compares
/// that to a threshold.
///
/// Returns a tuple (`is_jpeg_data`, `likely_end`), where the first element contains whether the cluster
/// is likely JPEG scan data, and the second contains the index of the likely end of the JPEG scan data
/// (if it is likely scan data), i.e. the first 0xff that is not followed by 0xd0..=0xd7 or 0x00
pub fn jpeg_data(cluster: &[u8]) -> (bool, Option<usize>) {
	// PERF: Could optimise this by both calculating the entropy and doing the analysis in one pass. Perhaps move the count
	//       calculations out of the shannon_entropy fn
	let entropy = shannon_entropy(cluster);

	let mut count_ff00 = 0;
	// Contains the first instance of a byte sequence that is invalid in a JPEG scan or terminates a JPEG scan,
	// if one has been encountered
	let mut first_ffxx = None;
	let mut curr_rst_marker = None;
	// RST markers have to be encountered in sequence
	let mut rst_marker_ordering_valid = true;
	let mut found_invalid_marker = false;
	for i in 0..(cluster.len() - 1) {
		if cluster[i] == 0xff {
			match cluster[i + 1] {
				0x00 => {
					// If we've encountered an invalid sequence or terminator, don't increment ff00 counts
					if first_ffxx.is_none() {
						count_ff00 += 1;
					}
				}
				val @ 0xd0..=0xd7 => {
					if first_ffxx.is_none() { // We probably don't want to base any decisions on anything that happens after another marker, as it could well be the EOI. Maybe track that
						if let Some(curr_rst) = curr_rst_marker {
							if val == curr_rst + 1 || val == 0xd0 && curr_rst == 0xd7 {
								curr_rst_marker = Some(val);
							} else {
								rst_marker_ordering_valid = false;
							}
						} else {
							curr_rst_marker = Some(val);
						}
					}
				}
				0x01..=0xbf => { // Reserved markers, shouldn't appear (at least, before another valid one). https://stackoverflow.com/a/53062155/11009247
					if first_ffxx.is_none() {
						found_invalid_marker = true;
						break;
					}
				}
				_ => {
					if first_ffxx.is_none() {
						first_ffxx = Some(i);
					}
				}
			}
		}
	}

	let entropy_valid = entropy > ENTROPY_THRESHOLD;
	let contents_valid = count_ff00 >= FF00_THRESHOLD && rst_marker_ordering_valid && !found_invalid_marker;

	let is_likely_jpeg = (entropy_valid || count_ff00 >= FF00_CERTAINTY_THRESHOLD) && contents_valid;

	(
		is_likely_jpeg,
		if is_likely_jpeg {
			first_ffxx
		} else {
			None
		}
	)
}