# Benchmarking

This file keeps records of benchmarks that have been done, across different benchmark targets/types, hardware and test cases.

# Disk Read

SSD: WD Blue SN570 1TB
- Reported sequential read: 3500 MB/s (~3.260 GiB)

## FIO Benchmark

- Parameters: `fio --name TEST --eta-newline=5s --filename=fio-tempfile.dat --rw=read --size=500m --io_size=10g --blocksize=1024k --ioengine=libaio --fsync=10000 --iodepth=32 --direct=1 --numjobs=1 --runtime=60 --group_reporting`
- Environment: Idle system with programs open

Test results: 3508 MB/s (~3.267 GiB)

## Searchlight Benchmarks

Each benchmark is run 10 times - 10 samples. The results in the [] are the confidence interval upper and lower bounds, and in the middle, the best guess as to the time taken for each sample, as reported by criterion.

See benches/io_bench.rs for the benchmark file.

### No-op

Each read block was simply passed through a black_box, not copied or processed in any way. These scores are closer to what you see on drive benchmarks - closer to the 3.267 GiB/s that the used SSD is rated at, but not representative of real scenarios where you want to do processing on each block.

The io/mmap benchmark is a significant outlier here - as no bytes are actually read from the loaded block, the kernel doesn't actually need to do any I/O operations, so it's super fast.

io/filebuf
- time:   [1.8220 s 1.8266 s 1.8311 s]
- thrpt:  [2.7806 GiB/s 2.7876 GiB/s 2.7945 GiB/s]

io/mmap
- time:   [2.7183 µs 2.7619 µs 2.8084 µs]
- thrpt:  [1813006 GiB/s 1843511 GiB/s 1873141 GiB/s]

io/io_uring
- time:   [1.8113 s 1.8782 s 1.9389 s]
- thrpt:  [2.6260 GiB/s 2.7110 GiB/s 2.8111 GiB/s]

io/direct
- time:   [1.9234 s 1.9433 s 1.9661 s]
- thrpt:  [2.5897 GiB/s 2.6201 GiB/s 2.6472 GiB/s]

### Memcpy

Each read block was memcpy'd into a buffer. These throughput scores are closer to what you'd expect when doing processing on each loaded block.

The io/mmap benchmark is an outlier, as it seems to outperform the reported and FIO benchmarked read speed, which can be put down to caching. posix_fadvise was used to attempt to circumvent this, to no avail. The others weren't affected by this, as they use O_DIRECT. A single run was able to be timed (see examples/mmap_bench.rs) for a result of ~0.786 GiB/s - interestingly far slower than expected, and far slower than the rest.

io/filebuf
- time:   [2.4441 s 2.4578 s 2.4734 s]
- thrpt:  [2.0586 GiB/s 2.0716 GiB/s 2.0832 GiB/s]

io/mmap
- time:   [999.03 ms 1.0051 s 1.0141 s]
- thrpt:  [5.0209 GiB/s 5.0660 GiB/s 5.0966 GiB/s]

io/io_uring
- time:   [2.5286 s 2.5356 s 2.5433 s]
- thrpt:  [2.0020 GiB/s 2.0081 GiB/s 2.0136 GiB/s]

io/direct
- time:   [2.5214 s 2.5372 s 2.5521 s]
- thrpt:  [1.9951 GiB/s 2.0068 GiB/s 2.0194 GiB/s]

### Sequential Read

Each read block was looped through, passing each byte through a black_box. These throughput scores, showing speeds slower than the memcpy benchmark, show that, at least on the tested-on system, that drive I/O is not a single bottleneck for operations slower than a sequential, single-threaded, non-SIMD read of the entire block. Memory copies are very fast in comparison, as there are many optimisations and tricks available to optimise them.

The io/mmap benchmark here is faster than the rest, but within the drive read limit. It can be assumed this is due to caching, but is bottlenecked by the sequential read.

io/filebuf
- time:   [3.5759 s 3.6031 s 3.6402 s]
- thrpt:  [1.3987 GiB/s 1.4131 GiB/s 1.4239 GiB/s]

io/mmap
- time:   [1.7763 s 1.7795 s 1.7830 s]
- thrpt:  [2.8557 GiB/s 2.8612 GiB/s 2.8664 GiB/s]

io/io_uring
- time:   [3.4978 s 3.5224 s 3.5493 s]
- thrpt:  [1.4345 GiB/s 1.4455 GiB/s 1.4557 GiB/s]

io/direct
- time:   [3.5381 s 3.5619 s 3.5868 s]
- thrpt:  [1.4196 GiB/s 1.4295 GiB/s 1.4391 GiB/s]

# Searching

## Test Environment

- Low activity linux environment
- Hardware:
	- CPU: AMD Ryzen 7 7700X
	- GPU: AMD Radeon RTX 6950 XT (PCIe 5)

## Test Case

The file used for benchmarking the searching algorithm implementations can be downloaded from Digital Corpora, at this link: [https://digitalcorpora.s3.amazonaws.com/corpora/drives/nps-2009-ubnist1/ubnist1.gen3.raw](https://digitalcorpora.s3.amazonaws.com/corpora/drives/nps-2009-ubnist1/ubnist1.gen3.raw)

Only one pattern was used: `[ 0x7f, 0x45, 0x4c, 0x46 ]` (this will be changed)

## Searchlight Benchmark

2 algorithms are implemented - Base Aho-Corasick (AC), on the CPU, and Parallel Failureless Aho-Corasick (PFAC) on the GPU (with Vulkan via vulkano). Unsurprisingly, the GPU-accelerated PFAC
outperforms AC on the CPU. AC serves as a good fallback for when an appropriate Vulkan implementation is not available, or GPU-acceleration is opted out of, however.

Each benchmark is run 20 times - 20 samples. The results in the [] are the confidence interval upper and lower bounds, and in the middle, the best guess as to the time taken for each sample, as reported by criterion.

See benches/search_bench.rs for the benchmark file.

Of note is that profiling the benchmark case for pfac_gpu showed that memory copies were where the CPU spent ~73% of its time - Optimising or circumventing these copies, if possible, will have significant performance impact.

search/pfac_gpu
- time:   [578.90 ms 580.43 ms 581.98 ms]
- thrpt:  [3.3711 GiB/s 3.3801 GiB/s 3.3890 GiB/s]

search/ac_cpu
- time:   [1.8327 s 1.8351 s 1.8375 s]
- thrpt:  [1.0677 GiB/s 1.0691 GiB/s 1.0705 GiB/s]