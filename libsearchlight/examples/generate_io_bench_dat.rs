use std::fs;

use tinyrand::{StdRand, Rand};

fn main() {
	let mut file_data = vec![0u8; 1024 * 1024 * 512];

	let mut rng = StdRand::default();

	file_data.chunks_mut(8).for_each(|elems| unsafe {
		*(elems.as_mut_ptr() as *mut u128) = rng.next_u128()
	});

	fs::write("test_data/io_bench.dat", file_data).unwrap();
}