use std::{arch::x86_64::{_mm_prefetch, _MM_HINT_T0}, fs::File, hint::black_box};

use criterion::{criterion_group, criterion_main, Criterion, Bencher, Throughput};
use libsearchlight::{search::{ac_cpu::AcCpu, pfac_gpu::PfacGpu, search_common::AcTableBuilder, SearchFuture, Searcher}, searchlight::config::MatchString, utils::iter::ToGappedWindows};

criterion_group!(benches, search_bench);
criterion_main!(benches);

const BENCH_FILE: &'static str = "../test_data/ubnist1.gen3.raw";
const SEARCH_PATTERNS: &'static [&'static str] = &[ "\\x7f\\x45\\x4c\\x46" ];

fn search_bench(c: &mut Criterion) {
	let mut group = c.benchmark_group("search");
	group.sample_size(20);

	let bench_file_len = File::open(BENCH_FILE).unwrap().metadata().unwrap().len();
	group.throughput(Throughput::Bytes(bench_file_len));

	group.bench_function("ac_cpu", ac_cpu);
	group.bench_function("pfac_gpu", pfac_gpu);

	group.finish();
}

fn ac_cpu(b: &mut Bencher) {
	let search_buf = std::fs::read(BENCH_FILE).unwrap();
	let patterns = SEARCH_PATTERNS;

	b.iter_batched(|| {
		let mut pfac_table = AcTableBuilder::new(true);
		for pat in patterns {
			pfac_table.add_pattern(&MatchString::from(*pat));
		}
		let pfac_table = pfac_table.build();
		let ac = AcCpu::new(pfac_table);
		ac
	}, |mut ac: AcCpu| {
		let matches = ac.search(&search_buf, 0, 0).unwrap().wait().unwrap();
		black_box(matches);
	}, criterion::BatchSize::LargeInput);
}

fn pfac_gpu(b: &mut Bencher) {
	let search_buf = std::fs::read(BENCH_FILE).unwrap();
	let patterns = SEARCH_PATTERNS;

	b.iter_batched(|| {
		let mut pfac_table = AcTableBuilder::new(true);
		for pat in patterns {
			pfac_table.add_pattern(&MatchString::from(*pat));
		}
		let pfac_table = pfac_table.build();
		let ac = PfacGpu::new(pfac_table.clone()).unwrap();
		(ac, pfac_table.max_pat_len as usize)
	}, |(mut ac, max_pat_len)| {
		let mut matches = Vec::new();
		let mut result_fut: Option<SearchFuture> = None;

		for (i, window) in search_buf.gapped_windows(1024 * 1024, 1024 * 1024 - 4).enumerate() {
			unsafe { _mm_prefetch::<_MM_HINT_T0>(window.as_ptr() as *const i8) };
			if let Some(prev_result) = result_fut.take() {
				matches.append(&mut prev_result.wait().unwrap());
			}
			let r = {
				if i == 0 {
					ac.search(window, 0, 0).unwrap()
				} else {
					ac.search(window, (i * 1024 * 1024 - 4) as u64, max_pat_len).unwrap()
				}
			};
			result_fut = Some(r);
		}

		if let Some(result) = result_fut.take() {
			matches.append(&mut result.wait().unwrap());
		}

		black_box(matches);
	}, criterion::BatchSize::LargeInput);
}