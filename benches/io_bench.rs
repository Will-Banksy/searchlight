use criterion::{Criterion, black_box, criterion_main, criterion_group, Bencher, Throughput};
use searchlight::lib::io::{IoManager, mmap, filebuf};

criterion_group!(benches, criterion);
criterion_main!(benches);

fn criterion(c: &mut Criterion) {
	let mut group = c.benchmark_group("io");
	group.sample_size(10);
	group.throughput(Throughput::Bytes(5_467_144_192));

	group.bench_function("io.filebuf", bench_filebuf);
	group.bench_function("io.mmap", bench_mmap);
}

fn bench_filebuf(b: &mut Bencher) {
	// let start = Instant::now();

	b.iter_batched(|| {
		let file_path = "test_data/urandom_file.dat";

		let mut ioman = IoManager::new();

		ioman.open_with(file_path, |file, file_len, block_size| {
			Ok(filebuf::IoFileBuf::new(file, file_len, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open Cargo.toml");

		ioman
	}, |mut ioman| {
		let mut buf = vec![0; ioman.block_size() as usize];

		loop {
			let eof = ioman.with_next_block(|block| {
				match block {
					Some(block) => {
						buf.copy_from_slice(black_box(block));

						false
					},
					None => true
				}
			}).unwrap();

			if eof {
				break;
			}
		}
	}, criterion::BatchSize::LargeInput)

	// let secs_elapsed = start.elapsed().as_secs_f32();
	// let throughput_bytes = (ioman.file_len().unwrap() as f32) / secs_elapsed;
	// let throughput_mb = throughput_bytes / 1_000_000.0;

	// println!("\nTook: {} secs\nThroughput: {} bytes/s ({} MB/s)", secs_elapsed, throughput_bytes, throughput_mb);
}

fn bench_mmap(b: &mut Bencher) {
	b.iter_batched(|| {
		let file_path = "test_data/urandom_file.dat";

		let mut ioman = IoManager::new();

		ioman.open_with(file_path, |file, file_len, block_size| {
			Ok(mmap::IoMmap::new(file, file_len, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open Cargo.toml");

		ioman
	}, |mut ioman| {
		let mut buf = vec![0; ioman.block_size() as usize];

		loop {
			let eof = ioman.with_next_block(|block| {
				match block {
					Some(block) => {
						buf.copy_from_slice(black_box(block));

						false
					},
					None => true
				}
			}).unwrap();

			if eof {
				break;
			}
		}
	}, criterion::BatchSize::LargeInput)
}