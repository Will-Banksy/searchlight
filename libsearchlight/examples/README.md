# Examples

| Example                  | Description                                                                                      |
| ------------------------ | ------------------------------------------------------------------------------------------------ |
| direct_bench             | Reads test_data/io_bench.dat once fully with the io::IoDirect backend, timing how long it takes  |
| filebuf_bench            | Reads test_data/io_bench.dat once fully with the io::IoFileBuf backend, timing how long it takes |
| io_uring_bench           | Reads test_data/io_bench.dat once fully with the io::IoUring backend, timing how long it takes   |
| mmap_bench               | Reads test_data/io_bench.dat once fully with the io::IoMmap backend, timing how long it takes    |
| generate_io_bench_dat    | Generates test_data/io_bench.dat with random data with a set seed                                |
