pub mod config;

use std::{arch::x86_64::{_mm_prefetch, _MM_HINT_T0}, collections::VecDeque, fs::{self, File}};

use log::{debug, info, log_enabled, Level};
use memmap::MmapOptions;

use crate::{error::Error, io::file_len, search::{pairing::{self, pair}, search_common::AcTableBuilder, Search, SearchFuture, Searcher}, utils::iter::ToGappedWindows};

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

				debug!("Opened file {} (size: {} bytes)", &path, file_len);

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
						searcher.search_next(window, (i * block_size - max_pat_len) as u64).unwrap()
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
			let match_pairs = pair(&mut matches, id_ftype_map, true);

			info!("Searching complete: Found {} potential files ({} individual matches)", match_pairs.len(), num_matches);

			// TODO: Very basic testing with carving JPEGs showed that many JPEGs have their footer throughout their content
			//       Is there any way to combat this? Perhaps the validator can say "the footer that has been found is not a real footer, look for the next one instead"

			fs::create_dir(output_dir.as_ref()).unwrap();

			for pot_file in match_pairs {
				fs::write(format!("{}/{}-{}.{}", output_dir.as_ref(), pot_file.start_idx, pot_file.end_idx, pot_file.file_type.extension.clone().unwrap_or("".to_string())), &mmap[pot_file.start_idx as usize..pot_file.end_idx as usize]).unwrap();
			}

			Ok(true)
		} else {
			Ok(false)
		}
	}
}