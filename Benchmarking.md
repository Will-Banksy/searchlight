# DISK I/O

SSD: WD Blue SN570 1TB
Reported sequential read: 3500 MB/s

## FIO Test

Parameters: `fio --name TEST --eta-newline=5s --filename=fio-tempfile.dat --rw=read --size=500m --io_size=10g --blocksize=1024k --ioengine=libaio --fsync=10000 --iodepth=32 --direct=1 --numjobs=1 --runtime=60 --group_reporting`
Environment: Idle system with programs open

Test results: 3508 MB/s

## Searchlight Test #1

Parameters: block size 1 GiB; file RPiZ_DevEnv.qcow2 (25 GiB)
Environment: Idle system with programs open

Test results: 1751.8976 MB/s

## Searchlight Test #2

Parameters: block size 1 MiB; file RPiZ_DevEnv.qcow2 (25 GiB)
Environment: Idle system with programs open

Test results: 1750.118 MB/s

## Searchlight Test #3

Parameters: block size 1 KiB; file RPiZ_DevEnv.qcow2 (25 GiB)
Environment: Idle system with programs open

Test results: 187.63667 MB/s

## Searchlight Test #4

Parameters: block size 2 GiB; file RPiZ_DevEnv.qcow2 (25 GiB)
Environment: Idle system with programs open

Test results: 1602.2048 MB/s

## Searchlight Test #5

Parameters: block size 1 GiB; file urandom_file.dat (5.1 GiB)
Environment: Idle system with programs open

Test results: 1241.8044 MB/s