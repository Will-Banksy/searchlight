pub mod search_common;
#[cfg(feature = "gpu")]
pub mod pfac_gpu;
pub mod ac_cpu;
pub mod pairing;

use self::{search_common::AcTable, ac_cpu::AcCpu};

#[cfg(feature = "gpu")]
use crate::lib::utils::logging::sl_warn;

use super::error::Error;

#[cfg(feature = "gpu")]
use pfac_gpu::PfacGpu;

/// A result from searching, includes a start and end, and an id generated from the FNV-1a hash of the bytes of the match.
/// Using the FNV-1a hashing algorithm as it is very simple, with good characteristics, and is fast
#[derive(Debug, PartialEq)]
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
	fn search_next(&mut self, data: &[u8], data_offset: u64) -> Result<SearchFuture, Error>;
	fn search(&mut self, data: &[u8], data_offset: u64) -> Result<SearchFuture, Error>;
}

pub struct Search {
	search_impl: Box<dyn Searcher>
}

impl Search {
	/// Automatically selects
	///
	/// The GPU-accelerated PFAC implementation will be chosen by default if available
	pub fn new(table: AcTable, prefer_cpu: bool) -> Self {
		if !prefer_cpu {
			#[cfg(feature = "gpu")]
			{
				match PfacGpu::new(table.clone()) {
					Ok(pfac_gpu) => {
						return Search {
							search_impl: Box::new(pfac_gpu)
						};
					}
					Err(e) => {
						sl_warn!("Search", format!("Vulkan initialisation failed, falling back to CPU impl of Aho Corasick {:?}", e));
					}
				}
			}
		}

		return Search {
			search_impl: Box::new(AcCpu::new(table))
		};
	}
}

impl Searcher for Search {
	/// Searches the provided buffer through the used searching implementation
	fn search_next(&mut self, data: &[u8], data_offset: u64) -> Result<SearchFuture, Error> {
		match self.search_impl.search_next(data, data_offset) {
			Ok(results) => Ok(results),
			Err(e) => {
				Err(Error::from(e))
			}
		}
	}

	/// Searches the provided buffer through the used searching implementation
	///
	/// This should normally be called on ordered contiguous buffers, one after the other, but does not track progress
	fn search(&mut self, data: &[u8], data_offset: u64) -> Result<SearchFuture, Error> {
		match self.search_impl.search(data, data_offset) {
			Ok(results) => Ok(results),
			Err(e) => {
				Err(Error::from(e))
			}
		}
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

/// Calculates the FNV-1a hash of the slice using `match_id_hash_init` and `match_id_hash_add`
pub fn match_id_hash_slice(slice: &[u8]) -> u64 {
	let mut hash = match_id_hash_init();

	for n in slice {
		hash = match_id_hash_add(hash, *n);
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
#[allow(unused)]
mod test {
    use std::{collections::BTreeMap, fs};

    use crate::{lib::{search::{clmul, pfac_gpu::PfacGpu, search_common::AcTableBuilder, Match, SearchFuture, FNV_OFFSET_BASIS, FNV_PRIME}, utils::iter::ToGappedWindows}, sl_error};

    use super::{ac_cpu::AcCpu, Searcher};

	const TEST_FILE: &'static str = "test_data/ubnist1.gen3.raw";
	const SEARCH_PATTERNS: &'static [&'static [u8]] = &[ &[ 0x7f, 0x45, 0x4c, 0x46 ] ];

	#[test]
	fn test_clmul() {
		assert_eq!(clmul(FNV_OFFSET_BASIS, FNV_PRIME), (FNV_OFFSET_BASIS as u128 * FNV_PRIME as u128) as u64);
	}

	/// Runs the search impl across the test data in 1024*1024 byte windows, returning a map of window index to matches found in that window
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
	fn test_search_impls() { // TODO: Revisit. Ideally this would be a super fast test case to run cause nobody likes waiting for tests to run. Ideally it'd also run on CI, so I need a smaller file to ship with the code
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
				sl_error!("test_search_impls", format!("Key {} is not contained in pfac_windowed_matches", window_idx));
				continue;
			}
			for (i, (ac_match, pfac_match)) in ac_windowed_matches[window_idx].iter().zip(pfac_windowed_matches[window_idx].iter()).enumerate() {
				if ac_match != pfac_match {
					sl_error!("test_search_impls", format!("Matches at idx {} in window {} do not match", i, window_idx));
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