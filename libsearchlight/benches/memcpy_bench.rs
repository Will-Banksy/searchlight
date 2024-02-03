use std::{alloc::{self, Layout}, fs::{self, File}, hint::black_box};

use criterion::{criterion_group, criterion_main, BatchSize, Bencher, Criterion, Throughput};

criterion_group!(benches, memcpy_bench);
criterion_main!(benches);

const BENCH_FILE: &'static str = "test_data/ubnist1.gen3.raw";

fn memcpy_bench(c: &mut Criterion) {
	let mut group = c.benchmark_group("memcpy");
	group.sample_size(20);

	let bench_file_len = File::open(BENCH_FILE).unwrap().metadata().unwrap().len();
	group.throughput(Throughput::Bytes(bench_file_len));

	group.bench_function("memcpy", memcpy);

	group.finish();
}

fn memcpy(b: &mut Bencher) {
	let file_content = fs::read(BENCH_FILE).unwrap();

	let dst_buf_len = 1024 * 1024;
	let dst_buf_layout = Layout::from_size_align(dst_buf_len, 32).unwrap();
	let dst_buf_ptr = unsafe { alloc::alloc(dst_buf_layout) };
	let dst_buf = unsafe { std::slice::from_raw_parts_mut(dst_buf_ptr, dst_buf_len) };

	b.iter_batched(|| dst_buf.to_vec(), |mut dst_buf| {
		for chunk in file_content.chunks_exact(1024 * 1024) {
			// dst_buf.write(chunk).unwrap();
			dst_buf.copy_from_slice(chunk);
		}
		black_box(dst_buf);
	}, BatchSize::LargeInput);

	unsafe { alloc::dealloc(dst_buf_ptr, dst_buf_layout) };
}