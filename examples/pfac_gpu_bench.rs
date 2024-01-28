
#[cfg(feature = "gpu")]
use searchlight::lib::{search::{search_common::AcTableBuilder, pfac_gpu::PfacGpu, SearchFuture, Searcher}, utils::iter::ToGappedWindows};

const BENCH_FILE: &'static str = "test_data/ubnist1.gen3.raw";
const SEARCH_PATTERNS: &'static [&'static [u8]] = &[ &[ 0x7f, 0x45, 0x4c, 0x46 ] ];

#[cfg(not(feature = "gpu"))]
fn main() {
	println!("GPU feature necessary for this example is not enabled");
}

#[cfg(feature = "gpu")]
fn main() {
	let search_buf = std::fs::read(BENCH_FILE).unwrap();
	let patterns = SEARCH_PATTERNS;

	let producer = || {
		let mut pfac_table = AcTableBuilder::new(true);
		for pat in patterns {
			pfac_table.add_pattern(pat);
		}
		let pfac_table = pfac_table.build();
		let ac = PfacGpu::new(pfac_table).unwrap();
		ac
	};

	let consumer = |mut ac: PfacGpu| {
		let mut matches = Vec::new();
		let mut result_fut: Option<SearchFuture> = None;

		for (i, window) in search_buf.gapped_windows(1024 * 1024, 1024 * 1024 - 4).enumerate() {
			if let Some(prev_result) = result_fut.take() {
				matches.append(&mut prev_result.wait().unwrap());
			}
			let r = ac.search_next(window, (i * 1024 * 1024 - 4) as u64).unwrap();
			result_fut = Some(r);
		}
		println!("\nNo. matches: {}", matches.len());
	};

	for _ in 0..20 {
		let ac = producer();
		consumer(ac);
	}
}