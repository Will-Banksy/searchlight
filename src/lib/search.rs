use self::{pfac_cpu::PfacCpu, pfac_common::PfacTable};

use crate::lib::utils::logging::sl_warn;

use super::error::Error;

#[cfg(feature = "gpu")]
use pfac_gpu::PfacGpu;

pub mod pfac_common;
pub mod pfac_cpu;
#[cfg(feature = "gpu")]
pub mod pfac_gpu;

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

enum PfacImpl {
	Cpu(PfacCpu),
	#[cfg(feature = "gpu")]
	Gpu(PfacGpu)
}

pub struct Pfac {
	pfac_impl: PfacImpl
}

impl Pfac {
	/// Initialises one of the two available implementations of the PFAC dictionary searching algorithm:
	/// either the GPU-accelerated PFAC algorithm with Vulkan through vulkano or PFAC on the CPU,
	/// using a thread pool.
	///
	/// The GPU-accelerated implementation will be chosen by default if available
	pub fn new(table: PfacTable, prefer_cpu: bool) -> Self {
		if !prefer_cpu {
			#[cfg(feature = "gpu")]
			{
				match PfacGpu::new(table.clone()) {
					Ok(pfac_gpu) => {
						return Pfac {
							pfac_impl: PfacImpl::Gpu(pfac_gpu)
						};
					}
					Err(e) => {
						sl_warn!("Pfac", format!("Vulkan initialisation failed, falling back to CPU impl of PFAC {:?}", e));
					}
				}
			}
		}

		return Pfac {
			pfac_impl: PfacImpl::Cpu(PfacCpu::new(table))
		};
	}

	/// Searches the provided buffer through the used PFAC implementation
	///
	/// This should normally be called on ordered contiguous buffers, one after the other, as it tracks matching progress
	/// - to discard progress and correctly match on a non-contiguous or out of order buffer, call `discard_progress` between
	/// calling this method
	pub fn search_next(&mut self, data: &[u8], data_offset: u64) -> Result<Vec<Match>, Error> {
		match &mut self.pfac_impl {
			PfacImpl::Cpu(pfac_cpu) => {
				Ok(pfac_cpu.search_next(data, data_offset))
			}
			#[cfg(feature = "gpu")]
			PfacImpl::Gpu(pfac_gpu) => {
				match pfac_gpu.search_next(data, data_offset) {
					Ok(results) => Ok(results),
					Err(e) => {
						Err(Error::from(e))
					}
				}
			}
		}
	}

	pub fn discard_progress(&mut self) -> Result<(), Error> {
		match &mut self.pfac_impl {
			PfacImpl::Cpu(pfac_cpu) => {
				Ok(pfac_cpu.discard_progress())
			}
			#[cfg(feature = "gpu")]
			PfacImpl::Gpu(pfac_gpu) => {
				match pfac_gpu.discard_progress() {
					Ok(_) => Ok(()),
					Err(e) => {
						Err(Error::from(e))
					}
				}
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
#[test]
fn test_clmul() {
	assert_eq!(clmul(FNV_OFFSET_BASIS, FNV_PRIME), (FNV_OFFSET_BASIS as u128 * FNV_PRIME as u128) as u64);

	// let fnvob_simd = unsafe { std::mem::transmute(FNV_OFFSET_BASIS as u128) };//_mm_set_epi64x(0, FNV_OFFSET_BASIS as i64) };
	// let fnvp_simd = unsafe { std::mem::transmute(FNV_PRIME as u128) };//_mm_set_epi64x(0, FNV_PRIME as i64) };

	// println!("wrapping_mul:    {:#018x}", u64::wrapping_mul(FNV_OFFSET_BASIS, FNV_PRIME));
	// println!("truncated *:     {:#018x}", (FNV_OFFSET_BASIS as u128 * FNV_PRIME as u128) as u64);
	// println!("intrinsic clmul: {:#018x}", unsafe { std::mem::transmute::<__m128i, u128>(_mm_clmulepi64_si128::<8>(fnvob_simd, fnvp_simd)) } as u64);

	// println!("non-truncated *: {:#034x}", (FNV_OFFSET_BASIS as u128 * FNV_PRIME as u128));

	// println!("clmul:           {:#018x}", clmul(FNV_OFFSET_BASIS, FNV_PRIME));
}