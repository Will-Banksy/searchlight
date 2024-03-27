pub mod config;

use std::{arch::x86_64::{_mm_prefetch, _MM_HINT_T0}, collections::VecDeque, fs::{self, File}, io::{IoSlice, Write}};

use log::{debug, info, log_enabled, trace, Level};
use memmap::MmapOptions;

use crate::{error::Error, utils::file_len, search::{pairing::{self, pair, MatchPart}, search_common::AcTableBuilder, Search, SearchFuture, Searcher}, utils::{estimate_cluster_size, iter::ToGappedWindows}, validation::{DelegatingValidator, FileValidationType, FileValidator}};

use self::config::SearchlightConfig;

/// Default size of the blocks to load and search disk image data in
pub const DEFAULT_BLOCK_SIZE: usize = 1024 * 1024;

pub struct DiskImageInfo {
	pub path: String,
	pub cluster_size: Option<Option<u64>>
}

/// The main mediator of the library, this struct manages state
pub struct Searchlight {
	config: SearchlightConfig,
	queue: VecDeque<DiskImageInfo>,
}

impl Searchlight {
	/// Creates a new `Searchlight` instance with the specified config, validating it and returning an error if it
	/// did not successfully validate
	pub fn new(config: SearchlightConfig) -> Result<Self, Error> {
		match config.validate() {
			Ok(_) => Ok(Searchlight {
				config,
				queue: VecDeque::new(),
			}),
			Err(e) => Err(e)
		}
	}

	/// Add a file to the queue of files to be processed
	pub fn with_file(mut self, info: DiskImageInfo) -> Self {
		self.add_file(info);
		self
	}

	/// Add a file to the queue of files to be processed
	pub fn add_file(&mut self, info: DiskImageInfo) {
		self.queue.push_back(info);
	}

	/// Processes the file at the front of the queue, returning true if one was processed, and false if there were none to be processed.
	/// Returns an error if one occurred.
	pub fn process_file(&mut self, output_dir: impl AsRef<str>) -> Result<bool, Error> {
		if let Some(info) = self.queue.pop_front() {
			let (mmap, file_len) = {
				let mut file = File::open(&info.path)?;

				let file_len = file_len(&mut file)?;

				info!("Opened file {} (size: {} bytes)", &info.path, file_len);

				(
					unsafe { MmapOptions::new().map(&file)? },
					file_len
				)
			};

			assert_eq!(file_len, mmap.len() as u64);

			let (mut searcher, max_pat_len) = {
				let ac_table = AcTableBuilder::from_config(&self.config).build();

				(
					Search::new(ac_table.clone(), false),
					ac_table.max_pat_len as usize
				)
			};

			let block_size = searcher.max_search_size().unwrap_or(DEFAULT_BLOCK_SIZE);

			assert!(max_pat_len < block_size);

			let num_blocks = {
				let num_blocks = (file_len as usize - max_pat_len) / (block_size - max_pat_len);
				if file_len % num_blocks as u64 != 0 {
					num_blocks + 1
				} else {
					num_blocks
				}
			};

			info!("Starting search phase, searching {} bytes in {} blocks of (at most) {} bytes each", file_len, num_blocks, block_size);

			let mut matches = Vec::new();
			let mut result_fut: Option<SearchFuture> = None;

			// PERF: Perhaps use a by-block loading method when doing the sequential search and then go back to the memory map for the random-access carving.
			//       If possible, when using the GPU search impl, write directly into the vulkan-allocated host-side buffer to avoid a memcpy
			// PERF: Queuing read operations with io_uring might have a more substantial performance improvement for HDDs, as it may be able to reduce the
			//       amount of disk rotations - but for a single file, would it be any better? Perhaps look into this
			for (i, window) in mmap.gapped_windows(block_size, block_size - max_pat_len).enumerate() {
				// This probably doesn't do a lot but there seems no reason to not have it
				#[cfg(target_arch = "x86_64")]
				unsafe { _mm_prefetch::<_MM_HINT_T0>(window.as_ptr() as *const i8) };

				if let Some(prev_result) = result_fut.take() {
					matches.append(&mut prev_result.wait().unwrap());
				}
				let fut = {
					if i == 0 {
						searcher.search(window, 0).unwrap()
					} else {
						searcher.search_next(window, (i * (block_size - max_pat_len)) as u64).unwrap()
					}
				};
				result_fut = Some(fut);

				if log_enabled!(Level::Info) {
					eprint!("\rProgress: {:.2}", (i as f32 / num_blocks as f32) * 100.0);
				}
			}

			if log_enabled!(Level::Info) {
				eprintln!("\rProgress: 100.00%");
			}

			if let Some(result) = result_fut.take() {
				matches.append(&mut result.wait().unwrap());
			}

			let num_matches = matches.len();

			matches.sort_by_key(|m| m.start_idx);

			let id_ftype_map = &pairing::preprocess_config(&self.config);

			// Get the user-supplied cluster size or estimate it based off of headers
			// A None for cluster size here will indicate that the headers appear to be mostly not allocated on any usual cluster boundaries, or that
			// has been passed in as the case
			let cluster_size = info.cluster_size.unwrap_or_else(|| {
				estimate_cluster_size(matches.iter().filter(|m| {
					if let Some((_, _, part)) = id_ftype_map.get(&m.id) {
						*part == MatchPart::Header
					} else {
						assert!(false);
						panic!() // assert!(false) is not detected as a control flow terminator/does not return ! but is more semantically correct
					}
				}))
			});

			if log_enabled!(Level::Trace) {
				for m in &matches {
					if let Some((_, ftype, part)) = id_ftype_map.get(&m.id) {
						trace!("Match at {}, type {} ({})", m.start_idx, ftype.extension.clone().unwrap_or("<no extension>".to_string()), part);
					} else {
						assert!(false);
					}
				}
			}

			let match_pairs = pair(&mut matches, id_ftype_map, true);

			info!("Searching complete: Found {} potential files ({} individual matches)", match_pairs.len(), num_matches);

			// Create output directory, erroring if it exists already
			fs::create_dir(output_dir.as_ref())?;

			let validator = DelegatingValidator::new();

			for pot_file in match_pairs {
				let validation = validator.validate(&mmap, &pot_file, cluster_size);

				// TODO: Should the type be reported differently to how it is in the TRACE logs for individual matches? It's technically different - Getting the type id instead of the file extension, and
				//       so probably should be reported differently, but how? Keeping it lowercase I think
				debug!("Potential file at {}-{} (type {}) validated as: {}, with fragments {:?}", pot_file.start_idx, pot_file.end_idx + 1, pot_file.file_type.type_id, validation.validation_type, validation.fragments);

				if validation.validation_type != FileValidationType::Unrecognised {
					let fragments = if validation.fragments.is_empty() {
						vec![ (pot_file.start_idx..(pot_file.end_idx + 1)) ]
					} else {
						validation.fragments
					};

					// Get the minimum index and maximum index of all fragments and designate them the start and end idxs
					let start_idx = fragments.iter().min_by_key(|frag| frag.start).unwrap().start; // .map_or(pot_file.start_idx, |frag| frag.start);
					let end_idx = fragments.iter().max_by_key(|frag| frag.end).unwrap().end; // .map_or(pot_file.end_idx + 1, |frag| frag.end);

					// Create validation directory if it doesn't exist
					fs::create_dir_all(format!("{}/{}", output_dir.as_ref(), validation.validation_type.to_string()))?;

					// Create the file with filename <start_idx>-<end_idx>.<extension>
					let mut file = File::create(
						format!("{}/{}/{}-{}.{}",
							output_dir.as_ref(),
							validation.validation_type,
							start_idx,
							end_idx,
							pot_file.file_type.extension.clone().unwrap_or("".to_string())
						)
					)?;

					file.write_vectored(
						&fragments.iter().map(|frag| IoSlice::new(&mmap[frag.start as usize..frag.end as usize])).collect::<Vec<IoSlice>>()
					)?;
				}
			}

			info!("Successfully validated files exported to {}", output_dir.as_ref());

			Ok(true)
		} else {
			Ok(false)
		}
	}
}