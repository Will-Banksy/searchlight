[![Rust-Linux](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-linux.yml/badge.svg)](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-linux.yml)
[![Rust-Windows](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-windows.yml/badge.svg)](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-windows.yml)

# Searchlight

A WIP high performance, precise file carving tool developed for my honours project at uni.

TODO: Write more of README

## Scope

This tool will focus on carving non-fragmented and in-order bi-fragmented files, as this makes up the most significant proportion of files according to a study in 2021 by Vincent van der Meer, Hugo Jonker and Jeroen van den Bos. Although the framework won't mandate a specific carving/reconstruction strategy or focus.

Additionally, the following file formats are in scope (more may be added, it is unlikely any will be removed):

- PNG
- JPEG
- ZIP

## Benchmarks

See [Benchmarking.md](Benchmarking.md) for benchmarks & performance notes.

## Test Data

In the test_data/corpus directory are some sample files for testing the tool with. My test image generation tool [stoneblock](https://github.com/Will-Banksy/stoneblock) currently contains a config file that uses these test files to build a test image (this may be removed or changed in the future however).

3.png, 7.zip, 9.png, and g6-1.jpg are from or derived from data provided by Digital Corpora (Garfinkel *et al*, 2009), in particular the disk image "[nps-2009-canon2](https://corp.digitalcorpora.org/corpora/drives/nps-2009-canon2)".

All other files are authored by me.

## References

- Garfinkel, S.L., Farrell, P., Roussev, V., and Dinolt, G. (2009) ‘Bringing science to digital forensics with standardized forensic corpora’, *Digital Investigation*, 6(1), pp. 2-11. doi: 10.1016/j.diin.2009.06.016.
- Van der Meer, V., Jonker, H. and Van den Bos, J. (2021) ‘A Contemporary Investigation of NTFS File Fragmentation’, *Forensic Science International*, 38, pp. 1–11. doi: 10.1016/j.fsidi.2021.301125.
