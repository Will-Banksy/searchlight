use std::{hint::black_box, time::Duration};

use criterion::{Criterion, criterion_main, criterion_group, Bencher, Throughput, BenchmarkId};
use searchlight::lib::io::{IoManager, mmap, filebuf, io_uring, direct, DEFAULT_BLOCK_SIZE, DEFAULT_ALIGNMENT};

#[cfg(target_os = "linux")]
criterion_group!(benches, io_bench, io_uring_bench);
#[cfg(not(target_os = "linux"))]
criterion_group!(benches, io_bench);
criterion_main!(benches);

#[cfg(target_os = "linux")]
fn io_uring_bench(c: &mut Criterion) {
	let mut group = c.benchmark_group("io_uring_readlen");
	group.sample_size(20);
	group.throughput(Throughput::Bytes(5_467_144_192));
	group.measurement_time(Duration::from_secs(42));

	const RS: u64 = DEFAULT_ALIGNMENT as u64;

	for readlen in [ 16 * RS, 24 * RS, 32 * RS, 40 * RS, 48 * RS, 56 * RS, 64 * RS ].iter() {
		group.bench_with_input(BenchmarkId::from_parameter(readlen), readlen, bench_io_uring_readlen);
	}
	group.finish();
}

fn io_bench(c: &mut Criterion) {
	let mut group = c.benchmark_group("io");
	group.sample_size(10);
	group.throughput(Throughput::Bytes(5_467_144_192));

	let block_size = DEFAULT_BLOCK_SIZE;

	group.bench_with_input("filebuf", &block_size, bench_filebuf);
	group.bench_with_input("mmap", &block_size, bench_mmap);
	group.bench_with_input("io_uring", &block_size, bench_io_uring);
	group.bench_with_input("direct", &block_size, bench_direct);

	group.finish();

	// TODO: Create benchmark to test best read size for io_uring backend
	// let mut io_uring_bench = c.bench_function("io_uring_readlen", bench_io_uring_readlen);
}

fn bench_filebuf(b: &mut Bencher, block_size: &u64) {
	// let start = Instant::now();

	b.iter_batched(|| {
		let file_path = "test_data/io_bench.dat";

		let mut ioman = IoManager::new_with(*block_size);

		ioman.open_with_seq(file_path, |file_path, block_size| {
			Ok(filebuf::IoFileBuf::new(file_path, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open test_data/io_bench.dat");

		ioman
	}, bench_ioman, criterion::BatchSize::LargeInput)

	// let secs_elapsed = start.elapsed().as_secs_f32();
	// let throughput_bytes = (ioman.file_len().unwrap() as f32) / secs_elapsed;
	// let throughput_mb = throughput_bytes / 1_000_000.0;

	// println!("\nTook: {} secs\nThroughput: {} bytes/s ({} MB/s)", secs_elapsed, throughput_bytes, throughput_mb);
}

fn bench_mmap(b: &mut Bencher, block_size: &u64) {
	b.iter_batched(|| {
		let file_path = "test_data/io_bench.dat";

		let mut ioman = IoManager::new_with(*block_size);

		ioman.open_with_seq(file_path, |file_path, block_size| {
			Ok(mmap::IoMmap::new(file_path, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open test_data/io_bench.dat");

		ioman
	}, bench_ioman, criterion::BatchSize::LargeInput)
}

#[cfg(target_os = "linux")]
fn bench_io_uring(b: &mut Bencher, block_size: &u64) {
	b.iter_batched(|| {
		let file_path = "test_data/io_bench.dat";

		let mut ioman = IoManager::new_with(*block_size);

		ioman.open_with_seq(file_path, |file_path, block_size| {
			Ok(io_uring::IoUring::new(file_path, block_size, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open test_data/io_bench.dat");

		ioman
	}, bench_ioman, criterion::BatchSize::LargeInput)
}

fn bench_direct(b: &mut Bencher, block_size: &u64) {
	b.iter_batched(|| {
		let file_path = "test_data/io_bench.dat";

		let mut ioman = IoManager::new_with(*block_size);

		ioman.open_with_seq(file_path, |file_path, block_size| {
			Ok(direct::IoDirect::new(file_path, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open test_data/io_bench.dat");

		ioman
	}, bench_ioman, criterion::BatchSize::LargeInput)
}

fn bench_ioman(mut ioman: IoManager) {
	// let mut buf = vec![0; ioman.backend_info().unwrap().block_size as usize];

	loop {
		let eof = ioman.with_next_block(|block| {
			match block {
				Some(block) => {
					// buf[0..block.len()].copy_from_slice(black_box(block));
					black_box(block);

					false
				},
				None => true
			}
		}).unwrap();

		if eof {
			break;
		}
	}
}

#[cfg(target_os = "linux")]
fn bench_io_uring_readlen(b: &mut Bencher, read_len: &u64) {
	b.iter_batched(|| {
		let file_path = "test_data/io_bench.dat";

		let mut ioman = IoManager::new_with(DEFAULT_BLOCK_SIZE);

		ioman.open_with_seq(file_path, |file_path, block_size| {
			Ok(io_uring::IoUring::new(file_path, block_size, *read_len).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open test_data/io_bench.dat");

		ioman
	}, bench_ioman, criterion::BatchSize::LargeInput)
}