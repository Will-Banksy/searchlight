use std::io::Read;

use log::warn;

use crate::{search::{pairing::MatchPair, Match}, searchlight::config::SearchlightConfig, utils::{self, multi_reader::MultiReader}};

use super::{FileValidationInfo, FileValidationType, FileValidator, Fragment};

const ZIP_LOCAL_FILE_HEADER_SIG: u32 = 0x04034b50;
const ZIP_CENTRAL_DIR_HEADER_SIG: u32 = 0x02014b50;
const ZIP_DATA_DESCRIPTOR_SIG: u32 = 0x08074b50;

/// Not a constant directly of ZIP files, but the match id of the local file header signature
const ZIP_LOCAL_FILE_HEADER_SIG_ID: u64 = 13969706556131510235; // TODO: Check this

const ZIP_LOCAL_FILE_HEADER_SIZE: usize = 30;
const ZIP_DATA_DESCRIPTOR_SIZE: usize = 12;
const ZIP_CENTRAL_DIR_HEADER_SIZE: usize = 46;
const ZIP_END_OF_CENTRAL_DIR_SIZE: usize = 22;

const ZIP_DATA_DESCRIPTOR_FLAG: u16 = 0b1000;

const ZIP_COMPRESSION_METHOD_STORE: u16 = 0;
const ZIP_COMPRESSION_METHOD_DEFLATE: u16 = 8;

const DECOMPRESS_BUFFER_SIZE: usize = 1024 * 1024;

// NOTE: ImHex pattern language for ZIP local file header. Might be useful might not
// struct LocalFileHeader {
//     u32 signature;
//     u16 version;
//     u16 flags;
//     u16 compression;
//     u16 modtime;
//     u16 moddate;
//     u32 crc;
//     u32 compressed_size;
//     u32 uncompressed_size;
//     u16 file_name_len;
//     u16 extra_field_len;
//     char string[file_name_len];
//     u8 extra_field[extra_field_len];
// };

// LocalFileHeader hdr_0 @ 0x00;


pub struct ZipValidator;

struct LocalFileValidationInfo {
	validation_type: FileValidationType,
	frags: Vec<Fragment>
}

enum FileDataReconstructionInfo {
	Success {
		data_frags: Vec<Fragment>,
		end_idx: usize
	},
	Failure
}

#[derive(Debug)]
struct CentralDirectoryFileHeader<'a> {
	crc: u32,
	compressed_size: u32,
	file_header_offset: u32,
	file_name: &'a [u8],
	extra_field: &'a [u8],
	len: usize
}

#[derive(Debug)]
struct LocalFileHeader<'a> {
	idx: usize,
	has_data_descriptor: bool,
	compression_method: u16,
	crc: u32,
	compressed_size: u32,
	file_name: &'a [u8],
	extra_field: &'a [u8],
	offset: u32, // From CD
	len: usize
}

struct DataDescriptor {
	crc: u32,
	len: usize
}

enum CrcCalcError {
	UnsupportedCompressionMethod,
	DecompressionError,
}

/// Calculates the CRC of input data slices, which depends on the compression method: For store, you can just calculate the CRC
/// on the bytes directly, for deflate (or any other compression scheme but we're only supporting deflate cause it's the most
/// widely used) you need to decompress first
fn zip_crc_calc(data_slices: &[&[u8]], compression_method: u16) -> Result<u32, CrcCalcError> {
	match compression_method {
		ZIP_COMPRESSION_METHOD_STORE => {
			let mut hasher = crc32fast::Hasher::new();
			for slice in data_slices {
				hasher.update(&slice);
			}
			Ok(hasher.finalize())
		}
		ZIP_COMPRESSION_METHOD_DEFLATE => {
			let reader = MultiReader::new(data_slices);
			let deflate_reader = flate2::read::DeflateDecoder::new(reader);
			let mut crc_reader = flate2::CrcReader::new(deflate_reader);

			let mut intermediate_buffer = vec![0; DECOMPRESS_BUFFER_SIZE];

			loop {
				let read = crc_reader.read(&mut intermediate_buffer).map_err(|e| CrcCalcError::DecompressionError)?;
				if read == 0 {
					break;
				}
			}

			Ok(crc_reader.crc().sum())
		}
		_ => {
			return Err(CrcCalcError::UnsupportedCompressionMethod)
		}
	}
}

impl<'a> CentralDirectoryFileHeader<'a> {
	fn decode(data: &'a [u8]) -> Option<Self> {
		let signature = u32::from_le_bytes(data[0x00..0x04].try_into().unwrap());

		if signature != ZIP_CENTRAL_DIR_HEADER_SIG {
			return None;
		}

		let crc = u32::from_le_bytes(data[0x10..0x14].try_into().unwrap());
		let compressed_size = u32::from_le_bytes(data[0x14..0x18].try_into().unwrap());
		let file_name_len = u16::from_le_bytes(data[0x1c..0x1e].try_into().unwrap()) as usize;
		let extra_field_len = u16::from_le_bytes(data[0x1e..0x20].try_into().unwrap()) as usize;
		let file_header_offset = u32::from_le_bytes(data[0x2a..0x2e].try_into().unwrap());

		let file_name = &data[0x2e..(0x2e + file_name_len)];
		let extra_field = &data[(0x2e + file_name_len)..(0x2e + file_name_len + extra_field_len)];

		Some(CentralDirectoryFileHeader {
			crc,
			compressed_size,
			file_header_offset,
			file_name,
			extra_field,
			len: ZIP_CENTRAL_DIR_HEADER_SIZE + file_name_len + extra_field_len
		})
	}

	fn same(&self, lfhdr: &LocalFileHeader) -> bool {
		warn!("ZIP: same(): Comparing:\n-> {self:x?}\nWITH\n-> {lfhdr:x?}");
		// If the local file header CRC and compressed size are 0, then a data descriptor is present which contains this information instead.
		// In those cases, we're just gonna have to hope that the file name and extra field are good enough indicators
		(self.crc == lfhdr.crc || lfhdr.has_data_descriptor) &&
		(self.compressed_size == lfhdr.compressed_size || lfhdr.has_data_descriptor) &&
		self.file_name == lfhdr.file_name
		// self.extra_field == lfhdr.extra_field // NOTE: Apparently (according to samples I have examined) the extra field is not necessarily the same between Central Directory File Header and Local File Header
	}
}

impl<'a> LocalFileHeader<'a> {
	fn decode(data: &'a [u8], idx: usize) -> Option<Self> {
		let signature = u32::from_le_bytes(data[0x00..0x04].try_into().unwrap());

		if signature != ZIP_LOCAL_FILE_HEADER_SIG {
			return None;
		}

		let flags = u16::from_le_bytes(data[0x06..0x08].try_into().unwrap());
		let has_data_descriptor = (flags & ZIP_DATA_DESCRIPTOR_FLAG) > 0;

		let compression_method = u16::from_le_bytes(data[0x08..0x0a].try_into().unwrap());
		let crc = u32::from_le_bytes(data[0x0e..0x12].try_into().unwrap());
		let compressed_size = u32::from_le_bytes(data[0x12..0x16].try_into().unwrap());
		let file_name_len = u16::from_le_bytes(data[0x1a..0x1c].try_into().unwrap()) as usize;
		let extra_field_len = u16::from_le_bytes(data[0x1c..0x1e].try_into().unwrap()) as usize;

		let file_name = &data[0x1e..(0x1e + file_name_len)];
		let extra_field = &data[(0x1e + file_name_len)..(0x1e + file_name_len + extra_field_len)];

		Some(LocalFileHeader {
			idx,
			has_data_descriptor,
			compression_method,
			crc,
			compressed_size,
			file_name,
			extra_field,
			offset: 0,
			len: ZIP_LOCAL_FILE_HEADER_SIZE + file_name_len + extra_field_len
		})
	}

	fn update_with(self, cdfh: &CentralDirectoryFileHeader) -> Self {
		LocalFileHeader {
			crc: cdfh.crc,
			compressed_size: cdfh.compressed_size,
			offset: cdfh.file_header_offset,
			..self
		}
	}
}

impl DataDescriptor {
	fn decode(data: &[u8]) -> Self {
		let first_field = u32::from_le_bytes(data[0x00..0x04].try_into().unwrap());

		if first_field == ZIP_DATA_DESCRIPTOR_SIG {
			let crc = u32::from_le_bytes(data[0x04..0x08].try_into().unwrap());
			// let compressed_size = u32::from_le_bytes(data[0x08..0x0c].try_into().unwrap());

			DataDescriptor {
				crc,
				len: ZIP_DATA_DESCRIPTOR_SIZE + 4
			}
		} else {
			let crc = first_field;
			// let compressed_size = u32::from_le_bytes(data[0x04..0x08].try_into().unwrap());

			DataDescriptor {
				crc,
				len: ZIP_DATA_DESCRIPTOR_SIZE
			}
		}
	}
}

impl ZipValidator {
	pub fn new() -> Self {
		ZipValidator
	}

	fn validate_file(file_data: &[u8], header: &LocalFileHeader, next_header_idx: usize, cluster_size: usize) -> LocalFileValidationInfo {
		let data_idx = header.idx + header.len;

		// let unfrag_crc = crc32fast::hash(&file_data[data_idx..(data_idx + header.compressed_size as usize)]);

		let data_descriptor_len = if header.has_data_descriptor {
			let data_descriptor_idx = data_idx + header.compressed_size as usize;
			let data_descriptor = DataDescriptor::decode(&file_data[data_descriptor_idx..]);

			// If the data descriptor CRC is equal to the file content CRC, and the CRC from the central directory is not equal to the content CRC, then return with unrecognised. This
			// will, admittedly, be the case barely any of the time since all of the compressed size, name, and extra field will have to be the same between this file and a file in the
			// central directory
			// NOTE: Disabled for now cause we need the data descriptor length for the CRC return on error which introduces a cyclic dependency
			// if unfrag_crc != header.crc && unfrag_crc == data_descriptor.crc {
			// 	return LocalFileValidationInfo {
			// 		validation_type: FileValidationType::Unrecognised,
			// 		frags: Vec::new()
			// 	}
			// }

			data_descriptor.len
		} else {
			0
		};

		let unfrag_end = data_idx + header.compressed_size as usize + data_descriptor_len;

		let unfrag_crc = match zip_crc_calc(&[&file_data[data_idx..(data_idx + header.compressed_size as usize)]], header.compression_method) {
			Ok(crc) => crc,
			Err(CrcCalcError::UnsupportedCompressionMethod) => {
				// If we encounter an unsupported compression method, just return the data as if it was unfragmented cause we can't reconstruct it
				warn!("ZIP: Unsupported compression method ({}) may cause errors", header.compression_method);
				return LocalFileValidationInfo {
					validation_type: FileValidationType::Unanalysed,
					frags: vec![ (header.idx as usize..unfrag_end) ]
				}
			}
			Err(CrcCalcError::DecompressionError) => {
				// A decompression error almost certainly means that the file data is not intact or that it is fragmented, so just return any number that is not equal to the header CRC
				if header.crc == 0 {
					1
				} else {
					0
				}
			}
		};

		if unfrag_crc != header.crc { // TODO: In this case, fragmentation is likely. Take an approach for reconstructing the file data like the PNG one. Remember to take data descriptors into account
			warn!("Unfrag CRC != header CRC");

			// For cases we're not trying to tackle (out-of-order segment fragments or the fragment being past the central directory in the image), just return corrupted (cause it may also just be corrupted)
			if unfrag_end >= next_header_idx {
				return LocalFileValidationInfo {
					validation_type: FileValidationType::Corrupt,
					frags: vec![ (header.idx as usize..unfrag_end) ]
				}
			}

			let recons_info = Self::reconstruct_file_data(file_data, header, data_idx, next_header_idx, cluster_size);

			match recons_info {
				FileDataReconstructionInfo::Success { mut data_frags, end_idx } => {
					warn!("ZIP: Reconstruction success!");
					let header_frag = header.idx..data_idx;
					data_frags.insert(0, header_frag);

					if header.has_data_descriptor {
						let data_descriptor = DataDescriptor::decode(&file_data[end_idx..]);
						let data_desc_frag = end_idx..(end_idx + data_descriptor.len);
						data_frags.push(data_desc_frag);
					}

					utils::simplify_ranges(&mut data_frags);

					LocalFileValidationInfo {
						validation_type: FileValidationType::Correct,
						frags: data_frags
					}
				}
				FileDataReconstructionInfo::Failure => {
					warn!("ZIP: Reconstruction failure");
					LocalFileValidationInfo {
						validation_type: FileValidationType::Partial,
						frags: vec![ (header.idx as usize..unfrag_end) ]
					}
				}
			}
		} else {
			warn!("ZIP: CRCs are correct...?");

			LocalFileValidationInfo {
				validation_type: FileValidationType::Correct,
				frags: vec![ (header.idx as usize..unfrag_end) ]
			}
		}
	}

	/// Attempts to reconstruct ZIP file data, given an assumed unfragmented local file header, and the index of either the next header, assuming ZIP segments
	/// are tightly packed, or the central directory if no header was found after this one, by enumerating some possible cluster arrangements between the start
	/// of the file data and the next header index for a calculated CRC that matches that in the header
	fn reconstruct_file_data(file_data: &[u8], header: &LocalFileHeader, data_idx: usize, next_header_idx: usize, cluster_size: usize) -> FileDataReconstructionInfo {
		let data_descriptor_len = {
			let data_descriptor_sig_idx = next_header_idx - (ZIP_DATA_DESCRIPTOR_SIZE + 4);
			if u32::from_le_bytes(file_data[data_descriptor_sig_idx..(data_descriptor_sig_idx + 4)].try_into().unwrap()) == ZIP_DATA_DESCRIPTOR_SIG {
				ZIP_DATA_DESCRIPTOR_SIZE + 4
			} else {
				ZIP_DATA_DESCRIPTOR_SIZE
			}
		};

		let fragmentation_start = utils::next_multiple_of(data_idx, cluster_size);
		let fragmentation_end = utils::prev_multiple_of(next_header_idx - data_descriptor_len, cluster_size);

		let bytes_skipped = next_header_idx - (data_idx + header.compressed_size as usize + data_descriptor_len);

		// If the next header index (as supplied - may also be the central directory index) is not at the same cluster-local offset as the end of this segment would be, then it is probably not
		// the actual next header after this, or this file segment doesn't belong, or something. Either way, it's not in scope to try and reconstruct it as of yet
		if bytes_skipped % cluster_size != 0 {
			warn!("ZIP: Skipped a non-multiple of cluster size? {}", bytes_skipped % cluster_size);
			return FileDataReconstructionInfo::Failure
		}

		// Calculate the numbers of clustes in the fragmentation range that are not ZIP, and that are
		let clusters_skipped = bytes_skipped / cluster_size;
		let clusters_needed = ((fragmentation_end - fragmentation_start) / cluster_size) - clusters_skipped;

		warn!("ZIP: Clusters needed: {clusters_needed}; Clusters skipped: {clusters_skipped}");
		warn!("ZIP: Fragmentation range: {fragmentation_start}..{fragmentation_end}");

		let fragmentations = utils::generate_fragmentations(cluster_size, fragmentation_start..fragmentation_end, clusters_needed);

		let mut correct_fragmentation = None;

		// Initialise CRC hasher with the file data up to the fragmentation point
		// let mut hasher = crc32fast::Hasher::new();
		// hasher.update(&file_data[data_idx..fragmentation_start]);

		let data_slices = vec![ &file_data[data_idx..fragmentation_start] ];

		for data_frags in fragmentations {
			// Clone the slices vec and add to it the slices for each fragment in this fragmentation
			let mut data_slices = data_slices.clone();
			for range in &data_frags {
				data_slices.push(&file_data[range.start as usize..range.end as usize]);
			}

			warn!("ZIP: Trying fragmentation: {data_frags:?}");

			// Add the last part of the file data, the bit outside the fragmentation range
			data_slices.push(&file_data[fragmentation_end..(next_header_idx - data_descriptor_len)]);

			// Now we can calculate the CRC
			let calc_crc = match zip_crc_calc(&[&file_data[data_idx..(data_idx + header.compressed_size as usize)]], header.compression_method) {
				Ok(crc) => crc,
				Err(CrcCalcError::UnsupportedCompressionMethod) => {
					unimplemented!(); // This should not happen
				}
				Err(CrcCalcError::DecompressionError) => {
					// A decompression error almost certainly means that the file data is not intact or that it is fragmented, so just return any number that is not equal to the header CRC
					if header.crc == 0 {
						1
					} else {
						0
					}
				}
			};
			if calc_crc == header.crc {
				warn!("ZIP: Found correct fragmentation!");
				correct_fragmentation = Some(data_frags);
				break;
			}
		}

		if let Some(mut data_frags) = correct_fragmentation {
			data_frags.insert(0, data_idx..fragmentation_start);
			data_frags.push(fragmentation_end..(next_header_idx - data_descriptor_len));

			utils::simplify_ranges(&mut data_frags);

			FileDataReconstructionInfo::Success { data_frags, end_idx: next_header_idx }
		} else {
			warn!("ZIP: Exhausted all fragmentations, found no solution");
			FileDataReconstructionInfo::Failure
		}
	}
}

impl FileValidator for ZipValidator {
	// Written using: https://pkwaredownloads.blob.core.windows.net/pem/APPNOTE.txt and https://users.cs.jmu.edu/buchhofp/forensics/formats/pkzip.html
	fn validate(&self, file_data: &[u8], file_match: &MatchPair, all_matches: &[Match], cluster_size: usize, _config: &SearchlightConfig) -> FileValidationInfo {
		// Since ZIP files may have multiple headers before 1 footer, and so we can only assume that 1 footer = 1 zip file, this match pair
		// may well span the nth file in the zip to the EOCD signature. We can check the number of entries we come across however against
		// the number of entries in the central directory and if they don't match, and no other problems have been encountered, then we can
		// say it's a partial match
		// Additionally, since ZIP files are somewhat complex, this validation function will not be exhaustive, and may produce
		// incorrect output against some zip files. In particular, the following are not handled: ZIP64 files, ZIP multipart files, encrypted
		// ZIP files, ZIP files containing digital signatures

		// NOTE: Okay so new approach for dealing with ZIPs:
		//       1. Decode the central directory (Boiko and Moskalenko didn't try tackle a fragmented central directory so neither do I have to)
		//       2. Search the passed-in match list for ZIP local file headers, and find the ones with metadata corresponding to the metadata in the central directory (again not trying to tackle fragmented metadata)
		//       3. For each local file header, tackle fragmentation of that header in the same way that we would in a PNG file (we are going to, for ease, assume that the ZIP file segments are tightly packed)
		//       4. For each file, put their fragments in order of the offsets in the central directory
		//       5. As one last thing, go through the fragments and check that all the offsets are correct. If they are not, validate the ZIP as either Partial or Corrupted

		let eocd_idx = file_match.end_idx - file_match.file_type.footers[0].len() + 1;

		if (eocd_idx + ZIP_END_OF_CENTRAL_DIR_SIZE) > file_data.len() {
			return FileValidationInfo {
				validation_type: FileValidationType::Partial,
				..Default::default()
			}
		}

		let eocd_comment_len = u16::from_le_bytes(file_data[(eocd_idx + 0x14)..(eocd_idx + 0x16)].try_into().unwrap()) as usize;
		let eocd_len = eocd_comment_len + ZIP_END_OF_CENTRAL_DIR_SIZE;

		// Check the signature - we only want to handle the case of EOCD
		let signature = &file_data[eocd_idx..(eocd_idx + 4)];
		assert_eq!(signature, &[ 0x50, 0x4b, 0x05, 0x06 ]);

		// Get the disk number on which this EOCD record resides, and the disk number on which the central directory starts
		let cd_diskno = u16::from_le_bytes(file_data[(eocd_idx + 4)..(eocd_idx + 6)].try_into().unwrap());
		let cd_start_diskno = u16::from_le_bytes(file_data[(eocd_idx + 6)..(eocd_idx + 8)].try_into().unwrap());

		// Explicitly do not analyse the case of multi-disk/-part files
		if cd_diskno != cd_start_diskno || cd_diskno > 0 {
			return FileValidationInfo {
				validation_type: FileValidationType::Unanalysed,
				..Default::default()
			}
		}

		// Get the central directory total entries and size
		let cd_total_entries = u16::from_le_bytes(file_data[(eocd_idx + 10)..(eocd_idx + 12)].try_into().unwrap()); // NOTE: Do we want to make use of the total entries? Perhaps to check that the central directory is as expected?
		let cd_size = u32::from_le_bytes(file_data[(eocd_idx + 12)..(eocd_idx + 16)].try_into().unwrap()) as usize;

		// This assumes that the central directory is tightly packed and directly before the EOCD, which as far as I've read,
		// the spec doesn't specify
		let central_directory_idx = eocd_idx - cd_size;

		let central_directory = {
			let mut cd = Vec::new();

			let mut i = central_directory_idx;
			while i < eocd_idx {
				if let Some(record) = CentralDirectoryFileHeader::decode(&file_data[i..]) {
					i += record.len;
					cd.push(record);
				} // NOTE: Do we want any logic in the case that a central directory file header did not have the correct signature?
			}

			cd
		};

		warn!("ZIP: Central directory len: {}", central_directory.len());

		let zip_header_matches: Vec<&Match> = all_matches.iter().filter(|m| m.id == ZIP_LOCAL_FILE_HEADER_SIG_ID).collect();

		warn!("ZIP: Header matches len: {}", zip_header_matches.len());

		let local_file_headers = {
			let mut lfhs = Vec::new();

			for m in zip_header_matches {
				if let Some(record) = LocalFileHeader::decode(&file_data[m.start_idx as usize..], m.start_idx as usize) {
					// Search the central directory for a central directory record that matches this local file header, and add information from that central directory
					// file header to the local file header
					if let Some(cdfh) = central_directory.iter().find(|cdfh| cdfh.same(&record)) {
						lfhs.push(record.update_with(cdfh));
					}
				} // NOTE: We should never come across a local file header that doesn't have the correct signature, assuming it has been set correctly in the config. That's a thing tbf, the config not really being a config in the case where we have code to handle a file type...
			}

			lfhs.sort_by_key(|header| header.offset);

			lfhs
		};

		warn!("ZIP: Local files len: {}", local_file_headers.len());

		let frag_cd_eocd = central_directory_idx..(eocd_idx + eocd_len);

		let mut file_frags = vec![ frag_cd_eocd ];
		let mut worst_file_validation = FileValidationType::Correct;

		for i in 0..local_file_headers.len() {
			let mut validation_info = Self::validate_file(file_data, &local_file_headers[i], local_file_headers.get(i + 1).map(|header| header.offset as usize).unwrap_or(central_directory_idx), cluster_size); // TODO: Take max reconstruction search len into account

			if validation_info.validation_type != FileValidationType::Unrecognised {
				file_frags.append(&mut validation_info.frags);
				worst_file_validation = worst_file_validation.worst_of(validation_info.validation_type);
			}
		}

		// Since the files could be in any order, we sort by start of fragment, and then simplify of course
		file_frags.sort_by_key(|range| range.start);
		utils::simplify_ranges(&mut file_frags);

		if cd_total_entries as usize != local_file_headers.len() {
			warn!("ZIP: Not all files were found for ZIP archive starting at {}", file_match.start_idx);
			worst_file_validation = worst_file_validation.worst_of(FileValidationType::Corrupt);
		}

		FileValidationInfo {
			validation_type: worst_file_validation,
			fragments: file_frags
		}
	}
}