# DISK I/O

SSD: WD Blue SN570 1TB
- Reported sequential read: 3500 MB/s

## FIO Benchmark

- Parameters: `fio --name TEST --eta-newline=5s --filename=fio-tempfile.dat --rw=read --size=500m --io_size=10g --blocksize=1024k --ioengine=libaio --fsync=10000 --iodepth=32 --direct=1 --numjobs=1 --runtime=60 --group_reporting`
- Environment: Idle system with programs open

Test results: 3508 MB/s

## Last Benchmark

Each benchmark is run 10 times - 10 samples. The results in the [] are the confidence interval upper and lower bounds, and in the middle, the best guess as to the time taken for each sample.

The io/mmap benchmark is an outlier, as it seems to outperform the reported and FIO benchmarked read speed, which can be put down to caching. posix_fadvise was used to attempt to circumvent this, to no avail. The others weren't affected by this, as they use O_DIRECT.

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