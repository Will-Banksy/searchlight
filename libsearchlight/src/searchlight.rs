pub mod config;

use std::{arch::x86_64::{_mm_prefetch, _MM_HINT_T0}, collections::VecDeque, fs::{self, File}};

use log::{debug, info, log_enabled, trace, Level};
use memmap::MmapOptions;

use crate::{error::Error, io::file_len, search::{pairing::{self, pair}, search_common::AcTableBuilder, Search, SearchFuture, Searcher}, utils::iter::ToGappedWindows, validation::{DelegatingValidator, FileValidationType, FileValidator}};

use self::config::SearchlightConfig;

pub const DEFAULT_BLOCK_SIZE: usize = 1024 * 1024;

/// The main mediator of the library, this struct manages state
pub struct Searchlight {
	config: SearchlightConfig,
	queue: VecDeque<String>,
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
	pub fn with_file(mut self, path: impl Into<String>) -> Self {
		self.add_file(path);
		self
	}

	/// Add a file to the queue of files to be processed
	pub fn add_file(&mut self, path: impl Into<String>) {
		self.queue.push_back(path.into());
	}

	/// Processes the file at the front of the queue, returning true if one was processed, and false if there were none to be processed.
	/// Returns an error if one occurred.
	pub fn process_file(&mut self, output_dir: impl AsRef<str>) -> Result<bool, Error> {
		if let Some(path) = self.queue.pop_front() {
			let (mmap, file_len) = {
				let mut file = File::open(&path)?;

				let file_len = file_len(&mut file)?;

				info!("Opened file {} (size: {} bytes)", &path, file_len);

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

			debug!("Starting search phase, searching {} bytes in {} blocks of (at most) {} bytes each", file_len, num_blocks, block_size);

			let mut matches = Vec::new();
			let mut result_fut: Option<SearchFuture> = None;

			// TODO: Perhaps use a by-block loading method when doing the sequential search and then go back to the memory map for the random-access carving.
			//       If possible, when using the GPU search impl, write directly into the vulkan-allocated host-side buffer to avoid a memcpy
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
				let validation = validator.validate(&mmap, &pot_file);

				debug!("Potential file at {}-{} (type {:?}) validated as: {}, with len {:?}", pot_file.start_idx, pot_file.end_idx + 1, pot_file.file_type.type_id, validation.validation_type, validation.file_len);

				if validation.validation_type != FileValidationType::Unrecognised {
					let end_idx = validation.file_len.map(|len| len + pot_file.start_idx).unwrap_or(pot_file.end_idx + 1);

					// Create validation directory if it doesn't exist
					fs::create_dir_all(format!("{}/{}", output_dir.as_ref(), validation.validation_type.to_string()))?;

					// Write the file content into output directory
					fs::write(
						format!("{}/{}/{}-{}.{}",
							output_dir.as_ref(),
							validation.validation_type,
							pot_file.start_idx,
							end_idx,
							pot_file.file_type.extension.clone().unwrap_or("".to_string())
						),
						&mmap[pot_file.start_idx as usize..end_idx as usize]
					)?;
				}
			}

			Ok(true)
		} else {
			Ok(false)
		}
	}
}