[![Rust-Linux](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-linux.yml/badge.svg)](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-linux.yml)
[![Rust-Windows](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-windows.yml/badge.svg)](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-windows.yml)

# Searchlight

TODO: Write README

## Scope

This tool will focus on carving non-fragmented and in-order bi-fragmented files, as this makes up the most significant proportion of files according to a study in 2021 by Vincent van der Meer, Hugo Jonker and Jeroen van den Bos.

Additionally, the following file formats are in scope (more may be added, it is unlikely any will be removed):

- PNG
- JPEG
- ZIP

## Benchmarks

See [Benchmarking.md](Benchmarking.md) for benchmarks & performance notes.

## References

- Van der Meer, V., Jonker, H. and Van den Bos, J. (2021) ‘A Contemporary Investigation of NTFS File Fragmentation’, Forensic science international, 38, pp. 1–11. doi: 10.1016/j.fsidi.2021.301125.