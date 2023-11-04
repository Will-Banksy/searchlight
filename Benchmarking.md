# DISK I/O

SSD: WD Blue SN570 1TB
- Reported sequential read: 3500 MB/s

## FIO Benchmark

- Parameters: `fio --name TEST --eta-newline=5s --filename=fio-tempfile.dat --rw=read --size=500m --io_size=10g --blocksize=1024k --ioengine=libaio --fsync=10000 --iodepth=32 --direct=1 --numjobs=1 --runtime=60 --group_reporting`
- Environment: Idle system with programs open

Test results: 3508 MB/s