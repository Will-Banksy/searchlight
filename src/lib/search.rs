use vulkano::VulkanError;

pub mod pfac_common;
pub mod pfac_cpu;
pub mod pfac_gpu;

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

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// Returns the initial value to start creating a hash from a sequence of values with. Use `match_id_hash_add` to add
/// values to the hash
///
/// Using FNV-1a hashing algorithm as it is very simple, with good characteristics, and is fast
pub fn match_id_hash_init() -> u64 {
	FNV_OFFSET_BASIS
}

/// Takes the current hash value, adds a new value into the hash, and returns the new hash
///
/// Using FNV-1a hashing algorithm as it is very simple, with good characteristics, and is fast
pub fn match_id_hash_add(hash: u64, new_value: u8) -> u64 {
	(hash ^ new_value as u64).wrapping_mul(FNV_PRIME)
}

/// Calculates the hash of the slice using `match_id_hash_init` and `match_id_hash_add`
///
/// Using FNV-1a hashing algorithm as it is very simple, with good characteristics, and is fast
pub fn match_id_hash_slice(slice: &[u8]) -> u64 {
	let mut hash = match_id_hash_init();

	for n in slice {
		hash = match_id_hash_add(hash, *n);
	}

	hash
}

pub enum SearchError {
	VulkanError(VulkanError)
}