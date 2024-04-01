pub mod iter;
pub mod str_parse;

use std::{collections::BTreeMap, fs::File, io::{self, Seek}};

use crate::search::Match;

#[cfg(test)]
pub fn init_test_logger() {
	let _ = env_logger::builder().is_test(true).try_init();
}

/// Get the length of the file, by querying metadata and as a last resort seeking to the end of the file and getting the offset
pub fn file_len(file: &mut File) -> Result<u64, io::Error> {
	if let Ok(metadata) = file.metadata() {
		Ok(metadata.len())
	} else {
		let size = file.seek(io::SeekFrom::End(0))?;
		file.seek(io::SeekFrom::Start(0))?;
		Ok(size)
	}
}

/// Calculates the next multiple of `multiple` from `num`. E.g. `next_multiple_of(7, 3) == 9`,
/// `next_multiple_of(9, 3) == 12`
pub fn next_multiple_of(num: u64, multiple: u64) -> u64 {
	((num / multiple) + 1) * multiple
}

/// Calculates the previous multiple of `multiple` from `num`. E.g. `prev_multiple_of(7, 3) == 6`,
/// `prev_multiple_of(9, 3) == 9`
pub fn prev_multiple_of(num: u64, multiple: u64) -> u64 {
	(num / multiple) * multiple
}

/// Estimates the cluster size by iterating over each found header and collecting the number of times each header is divisible by
/// each power of two between 512 and 65,536, taking the mode of those counts. Also counts the number of times a header is not divisible
/// by any power of two and if that is more common than a power of two, None is returned to indicate an estimate that most files are not
/// allocated on cluster boundaries
///
/// If there are equal counts of multiple cluster sizes, or no cluster size, then the largest is chosen
pub fn estimate_cluster_size<'a>(headers: impl IntoIterator<Item = &'a Match>) -> Option<u64> {
	const MIN_CLUSTER_SIZE: u64 = 0b00000000_00000010_00000000; // 512
	const MAX_CLUSTER_SIZE: u64 = 0b00000001_00000000_00000000; // 65,536 (64 KiB)

	let mut histogram: BTreeMap<u64, u64> = BTreeMap::new();

	for header in headers {
		let mut cluster_size = MIN_CLUSTER_SIZE;
		let mut found_candidate = false;
		while cluster_size <= MAX_CLUSTER_SIZE {
			if header.start_idx % cluster_size == 0 {
				if let Some(count) = histogram.get_mut(&cluster_size) {
					*count += 1;
				} else {
					histogram.insert(cluster_size, 1);
				}
				found_candidate = true;
			}

			cluster_size <<= 1;
		}

		if !found_candidate {
			if let Some(count) = histogram.get_mut(&0) {
				*count += 1;
			} else {
				histogram.insert(0, 1);
			}
		}
	}

	let mut max = 0;
	let mut cluster_size = 0;
	for (k, v) in histogram {
		if v >= max {
			max = v;
			cluster_size = k;
		}
	}

	if cluster_size > 0 {
		Some(cluster_size)
	} else {
		None
	}
}

#[cfg(test)]
mod test {
    use crate::{search::Match, utils::estimate_cluster_size};

	#[test]
	fn test_cluster_size_estimates() {
		macro_rules! simple_match {
			($start_idx: expr) => {
				Match {
					start_idx: $start_idx,
					end_idx: $start_idx + 2,
					id: 0
				}
			};
		}

		let headers = [
			// simple_match!(512),
			simple_match!(1024),
			// simple_match!(1536),
			simple_match!(3),
			simple_match!(7),
			simple_match!(8192)
		];

		let est_cs = estimate_cluster_size(headers.iter());

		// println!("est_cs: {:?}", est_cs);

		assert_eq!(est_cs, Some(1024))
	}
}