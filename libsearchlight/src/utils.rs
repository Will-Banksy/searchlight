pub mod iter;
pub mod str_parse;
pub mod fragments_index;
pub mod subrange;

use std::{collections::BTreeMap, fs::File, io::{self, Seek}, ops::Range};

use crate::{search::Match, utils::subrange::IntoSubrangesExact, validation::Fragment};

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

/// Generates a list of lists of fragments, as candidates for reconstructing fragmented data in `fragmentation_range`. That is, for fragmented data in
/// `fragmentation_range`, occupying a known `num_file_clusters` clusters, this function will generate some possible arrangements of clusters that the
/// fragmented data can occupy, assuming that the fragmented data is in-order. To reiterate, this function is non-exhaustive, but aims to tackle common
/// cases, such as bifragmentation/a single gap.
///
/// # Panics
/// Panics if the fragmentation range is not on cluster boundaries.
pub fn generate_fragmentations(cluster_size: usize, fragmentation_range: Range<usize>, num_file_clusters: usize) -> Vec<Vec<Fragment>> {
	assert_eq!(fragmentation_range.start % cluster_size, 0);
	assert_eq!(fragmentation_range.end % cluster_size, 0);

	// Get the range for each cluster
	let clusters = fragmentation_range.clone().into_subranges_exact(cluster_size);
	assert_eq!(*clusters.remainder(), None);
	assert_eq!(clusters.len(), fragmentation_range.len() / cluster_size);

	// NOTE: While for now we're just tackling the simple bifragmented case, the problem of finding all possible in-order cases is laid out below
	//       In an ordered set of N numbers, we need to find G non-adjacent groups of continous elements such that the count of elements across each of the G groups is equal to C
	//       1, 2, 3, 4, 5; N = 5, G = 1, C = 3
	//       ->  [1, 2, 3], [2, 3, 4], [3, 4, 5]
	//       1, 2, 3, 4, 5; N = 5, G = 2, C = 3
	//       ->  [1, 2][4], [1, 2][5], [2, 3][5], [1][3, 4], [1][4, 5], [2][4, 5]
	//
	//       Number of solutions = G * C (N should factor in this...?)

	let mut gap_idx = 0;
	let gap_len = clusters.len() - num_file_clusters;

	let mut res = Vec::new();

	while gap_idx <= clusters.len() - gap_len {
		// Get all the clusters that are not in the gap, and simplify
		let mut file_clusters: Vec<Range<u64>> = clusters.iter().enumerate().filter(|(i, _)| *i < gap_idx || *i >= (gap_idx + gap_len)).map(|(_, c)| c.start as u64..c.end as u64).collect();
		simplify_ranges(&mut file_clusters);

		res.push(file_clusters);

		gap_idx += 1;
	}

	res
}

/// Takes a vec of assumed in-order, non-overlapping ranges, and where the end of a range is equal to the start of the next range, merges
/// the two ranges into one
pub fn simplify_ranges<T>(ranges: &mut Vec<Range<T>>) where T: PartialEq {
	let mut i = 1;
	while i < ranges.len() {
		if ranges[i - 1].end == ranges[i].start {
			ranges[i - 1].end = ranges.remove(i).end;
			i -= 1;
		}

		i += 1;
	}
}

/// Combines a list of ranges of indexes and a slice of data that is referred to by those indexes to produce a list of slices of that data
// NOTE: Is this useful?
pub fn idxs_to_slice<'a, T>(data: &'a [T], idxs: &[Range<usize>]) -> Vec<&'a [T]> {
	let mut res = Vec::with_capacity(idxs.len());

	for range in idxs {
		res.push(&data[range.clone()])
	}

	res
}

#[cfg(test)]
mod test {
    use crate::{search::Match, utils::estimate_cluster_size};

    use super::{generate_fragmentations, simplify_ranges};

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

		assert_eq!(est_cs, Some(1024))
	}

	#[test]
	fn test_generate_fragmentations() {
		let cluster_size = 2;

		let fragmentation_range = 10..20;

		let num_file_clusters = 3;

		// 10..12, 12..14, 14..16, 16..18, 18..20

		let expected = vec![
			vec![
				14..20
			],
			vec![
				10..12,
				16..20
			],
			vec![
				10..14,
				18..20
			],
			vec![
				10..16
			]
		];

		let calc_fragmentations = generate_fragmentations(cluster_size, fragmentation_range, num_file_clusters);

		assert_eq!(calc_fragmentations, expected);
	}

	#[test]
	fn test_simplify_ranges() {
		let mut test_data = vec![
			0..5,
			5..10,
			11..15,
			14..20,
			20..30,
			30..40
		];

		let expected = vec![
			0..10,
			11..15,
			14..40
		];

		simplify_ranges(&mut test_data);

		assert_eq!(test_data, expected);
	}
}