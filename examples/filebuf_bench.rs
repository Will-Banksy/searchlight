use std::time::Instant;

use searchlight::lib::io::{IoManager, filebuf, GenIoBackend, DEFAULT_BLOCK_SIZE, AccessPattern};

const BENCH_FILE: &'static str = "test_data/io_bench.dat";

/// This example is just a filebuf backend benchmark, where I can run it once, as Criterion doesn't like sample sizes less than 10
fn main() {
	let mut ioman = IoManager::new();

	let path = BENCH_FILE;
	let block_size = DEFAULT_BLOCK_SIZE;

	ioman.open_with(path, true, false, {
		GenIoBackend::Seq(
			filebuf::IoFileBuf::new(path, true, false, AccessPattern::Seq, block_size).map(|io_filebuf| Box::new(io_filebuf)).expect(&format!("Failed to open {}", path))
		)
	});

	let start = Instant::now();

	let mut buf = vec![0; ioman.backend_info(path).unwrap().block_size as usize];

	loop {
		let eof = ioman.read_next(path, |block| {
			match block {
				Some(block) => {
					buf[0..block.len()].copy_from_slice(std::hint::black_box(block));

					false
				},
				None => true
			}
		}).unwrap();

		if eof {
			break;
		}
	}

	let secs_elapsed = start.elapsed().as_secs_f32();
	let throughput_bytes = (ioman.backend_info(path).unwrap().file_len as f32) / secs_elapsed;
	let throughput_mb = throughput_bytes / 1_000_000.0;
	let throughput_gib = throughput_bytes / (1024.0 * 1024.0 * 1024.0);

	println!("\nTook: {} secs\nThroughput: {} bytes/s ({} MB/s, {} GiB/s)", secs_elapsed, throughput_bytes, throughput_mb, throughput_gib);
}