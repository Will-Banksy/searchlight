use criterion::{criterion_group, criterion_main, Criterion, Bencher};
use searchlight::lib::search::{pfac_common::PfacTableBuilder, Pfac};

criterion_group!(benches, search_bench);
criterion_main!(benches);

fn search_bench(c: &mut Criterion) {
	let mut group = c.benchmark_group("search");
	group.sample_size(20);
	group.throughput(criterion::Throughput::Bytes(2_106_589_184));

	group.bench_function("pfac_cpu_c1", pfac_cpu_c1);
	group.bench_function("pfac_gpu_c1", pfac_gpu_c1);

	group.finish();
}

fn pfac_cpu_c1(b: &mut Bencher) {
	let search_buf = std::fs::read("test_data/ubnist1.gen3.raw").unwrap();
	println!("\nSearch buf len: {}", search_buf.len());
	let patterns = &[ [ 0x7f, 0x45, 0x4c, 0x46 ] ];

	b.iter_batched(|| {
		let mut pfac_table = PfacTableBuilder::new(true);
		for pat in patterns {
			pfac_table.add_pattern(pat);
		}
		let pfac_table = pfac_table.build();
		let pfac = Pfac::new(pfac_table, true);
		pfac
	}, |mut pfac: Pfac| {
		let matches = pfac.search_next(&search_buf, 0).unwrap();
		println!("\nNo. matches: {}", matches.len())
	}, criterion::BatchSize::LargeInput);
}

fn _pfac_cpu_c2(b: &mut Bencher) {
	b.iter_batched(|| {
		todo!() // setup
	}, |_| {
		todo!() // process
	}, criterion::BatchSize::LargeInput);
}

fn pfac_gpu_c1(b: &mut Bencher) {
	let search_buf = std::fs::read("test_data/ubnist1.gen3.raw").unwrap();
	println!("\nSearch buf len: {}", search_buf.len());
	let patterns = &[ [ 0x7f, 0x45, 0x4c, 0x46 ] ];

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
		for chunk in search_buf.chunks(1024 * 1024) {
			let mut r = pfac.search_next(chunk, 0).unwrap();
			matches.append(&mut r);
		}
		println!("\nNo. matches: {}", matches.len());
	}, criterion::BatchSize::LargeInput);
}