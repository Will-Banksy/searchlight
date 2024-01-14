use criterion::{criterion_group, criterion_main, Criterion, Bencher};
use searchlight::lib::search::{pfac_common::PfacTableBuilder, pfac_cpu::PfacCpu};

criterion_group!(benches, search_bench);
criterion_main!(benches);

fn search_bench(c: &mut Criterion) {
	let mut group = c.benchmark_group("search");
	group.sample_size(10);
	group.throughput(criterion::Throughput::Bytes(2_106_589_184));

	group.bench_function("pfac_cpu_c1", pfac_cpu_c1);

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
		let pfac = PfacCpu::new(pfac_table);
		pfac
	}, |mut pfac: PfacCpu| {
		let matches = pfac.search_next(&search_buf, 0);
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