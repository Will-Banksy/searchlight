pub mod search_common;
#[cfg(feature = "gpu")]
pub mod pfac_gpu;
pub mod ac_cpu;
pub mod pairing;

use self::{search_common::AcTable, ac_cpu::AcCpu};

use super::error::Error;

#[cfg(feature = "gpu")]
use log::warn;
#[cfg(feature = "gpu")]
use pfac_gpu::PfacGpu;

/// A result from searching, includes a start and end, and an id generated from the FNV-1a hash of the bytes of the match.
/// Using the FNV-1a hashing algorithm as it is very simple, with good characteristics, and is fast
#[derive(Debug, PartialEq, Clone)]
pub struct Match {
	/// `id` should be produced by using the `match_id_hash_init` and `match_id_hash_add` functions on the values in a pattern
	pub id: u64,
	/// Refers to the index a match starts from, relative to the start of the file
	pub start_idx: u64,
	/// Refers to the index a match ends at, i.e. the last byte in a pattern, relative to the start of the file
	pub end_idx: u64
}

impl Match {
	/// Create a match record with specified id, start index and end index
	pub fn new(id: u64, start_idx: u64, end_idx: u64) -> Self {
		Match {
			id,
			start_idx,
			end_idx
		}
	}
}

pub struct SearchFuture {
	wait_fn: Box<dyn FnOnce() -> Result<Vec<Match>, Error>>
}

impl SearchFuture {
	pub fn new(wait_fn: impl FnOnce() -> Result<Vec<Match>, Error> + 'static) -> Self {
		SearchFuture {
			wait_fn: Box::new(wait_fn)
		}
	}

	pub fn wait(self) -> Result<Vec<Match>, Error> {
		(self.wait_fn)()
	}
}

pub trait Searcher {
	/// Searches a slice, returning a future that can be awaited upon for the result of the search,
	/// or an error if one occurred. Searches may be overlapping
	/// each other (by `overlap` bytes) and so implementors should either not keep state between
	/// calls or skip the first `overlap` bytes in their search (overlap will only ever be at the
	/// start of the slice)
	fn search(&mut self, data: &[u8], data_offset: u64, overlap: usize) -> Result<SearchFuture, Error>;

	/// The maximum number of bytes that this Searcher implementor can accept at a time for searching,
	/// or None if there is no limit. Default implementation returns None
	fn max_search_size(&self) -> Option<usize> {
		None
	}
}

pub struct DelegatingSearcher {
	search_impl: Box<dyn Searcher>,
	max_search_size: Option<usize>
}

impl DelegatingSearcher {
	/// Automatically chooses between the GPU-accelerated PFAC or the fallback AC.
	/// The GPU-accelerated PFAC implementation will be chosen by default if the
	/// project was compiled with the GPU feature and the a Vulkan implementation
	/// with the necessary features is available. Pass `prefer_cpu` as true to
	/// select the fallback AC implementation by default
	pub fn new(table: AcTable, prefer_cpu: bool) -> Self {
		if !prefer_cpu {
			#[cfg(feature = "gpu")]
			{
				match PfacGpu::new(table.clone()) {
					Ok(pfac_gpu) => {
						return DelegatingSearcher {
							search_impl: Box::new(pfac_gpu),
							max_search_size: Some(pfac_gpu::INPUT_BUFFER_SIZE as usize)
						};
					}
					Err(e) => {
						warn!("Vulkan initialisation failed, falling back to CPU impl of Aho Corasick: {:?}", e);
					}
				}
			}
		}

		return DelegatingSearcher {
			search_impl: Box::new(AcCpu::new(table)),
			max_search_size: None
		};
	}
}

impl Searcher for DelegatingSearcher {
	fn search(&mut self, data: &[u8], data_offset: u64, overlap: usize) -> Result<SearchFuture, Error> {
		match self.search_impl.search(data, data_offset, overlap) {
			Ok(results) => Ok(results),
			Err(e) => {
				Err(Error::from(e))
			}
		}
	}

	fn max_search_size(&self) -> Option<usize> {
		self.max_search_size
	}
}

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// Returns the initial FNV-1a value (FNV_OFFSET_BASIS) to start creating a hash from a sequence of values with. Use `match_id_hash_add` to add
/// values to the hash
pub fn match_id_hash_init() -> u64 {
	FNV_OFFSET_BASIS
}

/// Takes the current FNV-1a hash value, adds a new value into the hash, and returns the new hash
pub fn match_id_hash_add(hash: u64, new_value: u8) -> u64 {
	(hash ^ new_value as u64).wrapping_mul(FNV_PRIME)
}

/// Takes the current FNV-1a hash value, adds a new value into the hash, and returns the new hash. 16-bit version
pub fn match_id_hash_add_u16(hash: u64, new_value: u16) -> u64 {
	(hash ^ new_value as u64).wrapping_mul(FNV_PRIME)
}

/// Calculates the FNV-1a hash of the slice using `match_id_hash_init` and `match_id_hash_add`
pub fn match_id_hash_slice(slice: &[u8]) -> u64 {
	let mut hash = match_id_hash_init();

	for n in slice {
		hash = match_id_hash_add(hash, *n);
	}

	hash
}

/// Calculates the FNV-1a hash of the slice using `match_id_hash_init` and `match_id_hash_add`. 16-bit version
pub fn match_id_hash_slice_u16(slice: &[u16]) -> u64 {
	let mut hash = match_id_hash_init();

	for n in slice {
		hash = match_id_hash_add_u16(hash, *n);
	}

	hash
}

/// Carry-less multiplication, simply discards the overflowing bits of the result
#[allow(unused)]
fn clmul(mut x: u64, mut y: u64) -> u64 {
	let mut accum: u64 = 0;
	for _ in 0..64 {
		if x & 1 == 1 {
			accum = accum.wrapping_add(y);
		}
		x >>= 1;
		x ^= x & (1 << 63);
		y <<= 1;
		y ^= y & 1;
	}

	accum
}

#[cfg(test)]
mod test {
	#[cfg(feature = "big_tests")]
    use std::{collections::BTreeMap, fs};

	#[cfg(feature = "big_tests")]
	use crate::utils::iter::ToGappedWindows;

	use super::{clmul, FNV_OFFSET_BASIS, FNV_PRIME};

	#[cfg(feature = "big_tests")]
	use log::error;

	#[cfg(feature = "big_tests")]
	use super::{super::utils, ac_cpu::AcCpu, pfac_gpu::PfacGpu, search_common::AcTableBuilder, Searcher, SearchFuture, Match};

	#[cfg(feature = "big_tests")]
	const TEST_FILE: &'static str = "../test_data/nps-2009-canon2-gen6.raw";
	#[cfg(feature = "big_tests")]
	const SEARCH_PATTERNS: &'static [&'static [u16]] = &[ &[ 0xff, 0xd8, 0xff, 0xe0 ], &[ 0xff, 0xd8, 0xff, 0xe1 ] ];

	#[test]
	fn test_clmul() {
		assert_eq!(clmul(FNV_OFFSET_BASIS, FNV_PRIME), (FNV_OFFSET_BASIS as u128 * FNV_PRIME as u128) as u64);
	}

	// TODO: Hash tests, in particular tests to prove the output of the 16-bit and 8-bit hash functions are identical

	/// Runs the search impl across the test data in 1024*1024 byte windows, returning a map of window index to matches found in that window
	#[cfg(feature = "big_tests")]
	fn match_windowed(mut search_impl: Box<dyn Searcher>, test_data: &[u8]) -> BTreeMap<usize, Vec<Match>> {
		let mut matches = BTreeMap::new();

		let mut result_fut: Option<SearchFuture> = None;

		let windows: Vec<(usize, &[u8])> = test_data.gapped_windows(1024 * 1024, 1024 * 1024 - 4).enumerate().collect();

		for (i, window) in &windows {
			if let Some(prev_result) = result_fut.take() {
				let mut prev_result = prev_result.wait().unwrap();
				if !prev_result.is_empty() {
					prev_result.sort_by_key(|e| e.start_idx);
					matches.insert(*i, prev_result);
				}
			}
			let r = {
				if *i == 0 {
					search_impl.search(window, 0).unwrap()
				} else {
					search_impl.search_next(window, (i * 1024 * 1024 - (4 * i)) as u64).unwrap()
				}
			};
			result_fut = Some(r);
		}

		if let Some(result) = result_fut.take() {
			let mut result = result.wait().unwrap();
			if !result.is_empty() {
				result.sort_by_key(|e| e.start_idx);
				matches.insert(windows.len(), result);
			}
		}

		matches
	}

	#[test]
	#[cfg(feature = "big_tests")]
	fn test_search_impls() { // TODO: Revisit. Ideally this would be a super fast test case to run cause nobody likes waiting for tests to run. Ideally it'd also run on CI and not be feature-gated, so I need a smaller file to ship with the code
		utils::init_test_logger();

		let test_data = fs::read(TEST_FILE).unwrap();

		let mut table = AcTableBuilder::new(true);

		for pat in SEARCH_PATTERNS {
			table.add_pattern(pat);
		}

		let table = table.build();

		let mut ac = AcCpu::new(table.clone());
		let pfac = PfacGpu::new(table).unwrap();

		let ac_once_matches = ac.search(&test_data, 0).unwrap().wait().unwrap();

		let ac_windowed_matches = match_windowed(Box::new(ac), &test_data);

		let pfac_windowed_matches = match_windowed(Box::new(pfac), &test_data);

		for window_idx in ac_windowed_matches.keys() {
			if !pfac_windowed_matches.contains_key(window_idx) {
				error!(target: "test_search_impls", "Key {} is not contained in pfac_windowed_matches", window_idx);
				continue;
			}
			for (i, (ac_match, pfac_match)) in ac_windowed_matches[window_idx].iter().zip(pfac_windowed_matches[window_idx].iter()).enumerate() {
				if ac_match != pfac_match {
					error!(target: "test_search_impls", "Matches at idx {} in window {} do not match", i, window_idx);
				}
			}
		}

		let mut ac_windowed_matches_flat: Vec<Match> = ac_windowed_matches.into_values().flatten().collect();
		ac_windowed_matches_flat.sort_unstable_by_key(|e| e.start_idx);

		let mut pfac_windowed_matches_flat: Vec<Match> = pfac_windowed_matches.into_values().flatten().collect();
		pfac_windowed_matches_flat.sort_unstable_by_key(|e| e.start_idx);

		assert_eq!(ac_windowed_matches_flat, pfac_windowed_matches_flat);
		assert_eq!(ac_once_matches, ac_windowed_matches_flat);
	}
}