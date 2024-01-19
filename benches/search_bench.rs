use std::{fs::File, hint::black_box};

use criterion::{criterion_group, criterion_main, Criterion, Bencher, Throughput};
use searchlight::lib::search::{pfac_common::PfacTableBuilder, Pfac, PfacFuture};

criterion_group!(benches, search_bench);
criterion_main!(benches);

const BENCH_FILE: &'static str = "test_data/ubnist1.gen3.raw";
const SEARCH_PATTERNS: &'static [&'static [u8]] = &[ &[ 0x7f, 0x45, 0x4c, 0x46 ] ];

fn search_bench(c: &mut Criterion) {
	let mut group = c.benchmark_group("search");
	group.sample_size(20);

	let bench_file_len = File::open(BENCH_FILE).unwrap().metadata().unwrap().len();
	group.throughput(Throughput::Bytes(bench_file_len));

	group.bench_function("pfac_cpu", pfac_cpu);
	group.bench_function("pfac_gpu", pfac_gpu);

	group.finish();
}

fn pfac_cpu(b: &mut Bencher) {
	let search_buf = std::fs::read(BENCH_FILE).unwrap();
	let patterns = SEARCH_PATTERNS;

	b.iter_batched(|| {
		let mut pfac_table = PfacTableBuilder::new(true);
		for pat in patterns {
			pfac_table.add_pattern(pat);
		}
		let pfac_table = pfac_table.build();
		let pfac = Pfac::new(pfac_table, true);
		pfac
	}, |mut pfac: Pfac| {
		let matches = pfac.search_next(&search_buf, 0).unwrap().wait().unwrap();
		black_box(matches);
		// println!("\nNo. matches: {}", matches.len())
	}, criterion::BatchSize::LargeInput);
}

fn pfac_gpu(b: &mut Bencher) {
	let search_buf = std::fs::read(BENCH_FILE).unwrap();
	let patterns = SEARCH_PATTERNS;

	b.iter_batched(|| {
		let mut pfac_table = PfacTableBuilder::new(true);
		for pat in patterns {
			pfac_table.add_pattern(pat);
		}
		let pfac_table = pfac_table.build();
		let pfac = Pfac::new(pfac_table, false);
		pfac
	}, |mut pfac: Pfac| {
		let mut matches = Vec::new();
		let mut result_fut: Option<PfacFuture> = None;
		for chunk in search_buf.chunks(1024 * 1024) {
			if let Some(prev_result) = result_fut.take() {
				matches.append(&mut prev_result.wait().unwrap());
			}
			let r = pfac.search_next(chunk, 0).unwrap();
			result_fut = Some(r)
		}
		black_box(matches);
		// println!("\nNo. matches: {}", matches.len());
	}, criterion::BatchSize::LargeInput);
}