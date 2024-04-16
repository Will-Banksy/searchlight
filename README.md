[![rust-linux](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-linux.yml/badge.svg)](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-linux.yml)
[![rust-windows](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-windows.yml/badge.svg)](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-windows.yml)
[![rust-macos](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-macos.yml/badge.svg)](https://github.com/Will-Banksy/searchlight/actions/workflows/rust-macos.yml)

# Searchlight

A WIP high performance, precise file carving tool developed for my honours project at uni.

TODO: Write more of README

## Scope

This tool will focus on carving non-fragmented and in-order fragmented files, as this makes up the most significant proportion of files according to a study in 2021 by Vincent van der Meer, Hugo Jonker and Jeroen van den Bos. Although the framework won't mandate a specific carving/reconstruction strategy or focus.

Additionally, the following file formats are in scope (more may be added, it is unlikely any will be removed):

- PNG
- JPEG
- ZIP

## How It Works

Broadly, the tool works much like Scalpel (Richard and Roussev, 2005), going through the target disk image in two passes - One, to find signatures, the second, to carve the found pairs of header-footer matches, or carve from headers with the defined max size for that file type. Inspired by OpenForensics (Bayne, 2017), a GPU-accelerated dictionary search algorithm, PFAC (Lin *et al*, 2013) is used for the searching phase, implemented with Vulkan compute shaders.

The main way Searchlight differs from Scalpel is the validation framework, which also doubles as the framework for reconstructing fragmented files. Validators are implemented for file types, and due to the flexible nature of the framework, each validator can define it's scope and method for validation/reconstruction independently - a validator can validate and reconstruct file data in whichever way it chooses, using whichever techniques it chooses. The framework is designed this way as there is as of writing no consensus on any universal best way to carve any file type, and not subscribing to a specific strategy allows highly file-type specific techniques to be used, which have been shown to be successful compared to more general techniques (Hilgert *et al*, 2019; Uzun and Sencar, 2020; Ali and Mohamad, 2021; Boiko and Moskalenko, 2023).

Currently, there are 3 validators for PNG, JPEG, and ZIP files.

Once pairs of header-footer/header-max size are found, Searchlight then goes over each pair and runs the appropriate validator on them, which outputs a validation result along with a list of the fragments of the file, if found. If the validation result is positive, the fragments are written to disk.

### PNG Validator

The PNG validator (libsearchlight/src/validation/png.rs) works by reading chunk lengths, skipping forwards, and computing the CRC of the chunk data and comparing that with the stored CRC. It also performs some analysis on metadata such as checking field values are to spec, checking that chunks are correctly ordered, etc.

The reconstruction strategy implemented is based on that presented by Hilgert *et al* (2019), inspired by Garfinkel's (2007) bifragment gap carving algorithm, where if a computed CRC doesn't match the stored CRC, a valid chunk type is looked for in the same local cluster offset in subsequent clusters. Once found, possible arrangements of fragments of that chunk are generated, and for each, the CRC is computed and compared to the stored CRC. The implementation can currently handle any number of bi-fragmented chunks, with the limitation that it cannot deal with chunk metadata being on fragmentation boundaries.

### JPEG Validator

The JPEG validator (libsearchlight/src/validation/jpeg.rs) works very similarly to the PNG validator - By reading chunk lengths, and skipping forward. JPEG files don't have CRCs like PNGs, so metadata is relied upon to perform the validation. The main thing checked for JPEG files is that the necessary chunks are present.

Fragmentation in JPEG files is only handled in the entropy-coded scan data chunk - A classifier is used to classify clusters after the SOS marker (and look for terminating markers at the same time) to figure out which clusters are JPEG or not, and so, assuming the JPEG fragments are in-order, this approach can handle any number of fragments. However, JPEG data from a different file mixed in with the data from the current file can cause image corruption, as the validator/classifier cannot distinguish between JPEG data for the current file and JPEG data for a different file.

### ZIP Validator

The ZIP validator (libsearchlight/src/validation/zip.rs) works by jumping to the End Of Central Directory (EOCD), jumping backwards to the supposed Central Directory (CD), decoding that, and then jumping further backwards to each file header indicated by the CD, and calculating and comparing CRCs for each file header.

## Benchmarks

See [Benchmarking.md](Benchmarking.md) for benchmarks & performance notes.

## Test Data

In the test_data/corpus directory are some sample files for testing the tool with, and there is a config file [Stoneblock.toml](Stoneblock.toml) for usage with my test image generation tool [stoneblock](https://github.com/Will-Banksy/stoneblock) that uses these test files to build a test image.

3.png, 7.zip, 9.png, and g6-1.jpg are from or derived from data provided by Digital Corpora (Garfinkel *et al*, 2009), in particular the disk image "[nps-2009-canon2](https://corp.digitalcorpora.org/corpora/drives/nps-2009-canon2)".

All other files are authored by me.

## References

- Ali, R.R. and Mohamad, K.M. (2021) ‘RX_myKarve carving framework for reassembling complex fragementations of JPEG images’, *Journal of King Saud University. Computer and information sciences*, 33(1), pp. 21-32. doi: 10.1016/j.jksuci.2018.12.007.
- Bayne, E. (2017) *Accelerating Digital Forensic Searching Through GPGPU Parallel Processing Techniques*.
- Boiko, M. and Moskalenko, V. (2023) ‘Syntactical Method for reconstructing highly fragmented OOXML files’, *Radioelectronic and Computer Systems*, 0(1), pp. 166-182. doi: 10.32620/reks.2023.1.14.
- Garfinkel, S.L. (2007) ‘Carving contiguous and fragmented files with fast object validation’, *Digital Investigation*, 4(1), pp. 2-12. doi: 10.1016/j.diin.2007.06.017.
- Garfinkel, S.L., Farrell, P., Roussev, V., and Dinolt, G. (2009) ‘Bringing science to digital forensics with standardized forensic corpora’, *Digital Investigation*, 6(1), pp. 2-11. doi: 10.1016/j.diin.2009.06.016.
- Hilgert, J-N., Lambertz, M., Rybalka, M., Schell, R. (2019) ‘Syntactical Carving of PNGs and Automated Generation of Reproducible Datasets’, *Digital Investigation*, 29(1), pp. 22-30. doi: 10.1016/j.diin.2019.04.014.
- Lin, C-H., Liu, C-H., Chien, L-S., and Chang, S-C. (2013) ‘Accelerating Pattern Matching Using a Novel Parallel Algorithm on GPUs’, *IEEE Transactions on Computers*, 62(10), pp. 1906-1916. doi: 10.1109/TC.2012.254.
- Richard, G. and Roussev, V. (2005) ‘Scalpel: A Frugal, High Performance File Carver’, *Proceedings of the 2005 Digital Forensic Research Workshop*. New Orleans, LA: DFRWS, pp. 1-10.
- Uzun, E. and Sencar, H.T. (2020) ‘JpgScraper: An Advanced Carver for JPEG Files’, *IEEE Transactions on Information Forensics and Security*, 15(1), pp. 1846-1857. doi: 10.1109/TIFS.2019.2953382.
- Van der Meer, V., Jonker, H. and Van den Bos, J. (2021) ‘A Contemporary Investigation of NTFS File Fragmentation’, *Forensic Science International*, 38, pp. 1–11. doi: 10.1016/j.fsidi.2021.301125.
