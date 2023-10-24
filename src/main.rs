use std::time::Instant;

use searchlight::lib::io::IoManager;

fn main() {
	let start = Instant::now();

	let file_path = "test_data/urandom_file.dat";

	let mut ioman = IoManager::new(None);

	ioman.open(file_path).expect("Failed to open file");

	ioman.start().expect("Failed to start IoManager");

	loop {
		if let Ok(eof) = ioman.load_next_block() {
			if eof {
				break;
			}
		}

		ioman.with_current_block(|_| {
			()
		});
	}

	let secs_elapsed = start.elapsed().as_secs_f32();
	let throughput_bytes = (ioman.file_len().unwrap() as f32) / secs_elapsed;
	let throughput_mb = throughput_bytes / 1_000_000.0;

	println!("\nTook: {} secs\nThroughput: {} bytes/s ({} MB/s)", secs_elapsed, throughput_bytes, throughput_mb);
}
