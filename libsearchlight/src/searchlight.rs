pub mod config;
mod carve_log;

use std::{arch::x86_64::{_mm_prefetch, _MM_HINT_T0}, collections::VecDeque, fs::{self, File}, io::{IoSlice, Write}, path::{Path, PathBuf}};

use log::{debug, info, log_enabled, trace, Level};
use memmap::MmapOptions;

use crate::{error::Error, search::{pairing::{self, pair, MatchPart}, search_common::AcTableBuilder, Search, SearchFuture, Searcher}, searchlight::carve_log::CarveLog, utils::{estimate_cluster_size, file_len, iter::ToGappedWindows}, validation::{DelegatingValidator, FileValidationType, FileValidator}};

use self::config::SearchlightConfig;

/// Default size of the blocks to load and search disk image data in
pub const DEFAULT_BLOCK_SIZE: usize = 1024 * 1024;

pub enum CarveOperationInfo {
	Image {
		path: String,
		config: SearchlightConfig,
		cluster_size: Option<u64>, // TODO: Handle a cluster size of 1 (unaligned) better in the validators
		skip_carving: bool,
	},
	FromLog {
		path: String,
	}
}

impl CarveOperationInfo {
	pub fn path(&self) -> String {
		match &self {
			CarveOperationInfo::Image { path, .. } => path,
			CarveOperationInfo::FromLog { path } => path,
		}.to_string()
	}
}

/// The main mediator of the library, this struct manages state
pub struct Searchlight {
	queue: VecDeque<CarveOperationInfo>,
}

impl Searchlight {
	pub fn new() -> Self {
		Searchlight {
			queue: VecDeque::new()
		}
	}

	/// Add an operation to the queue of operations to be processed
	pub fn with_operation(mut self, info: CarveOperationInfo) -> Self {
		self.add_operation(info);
		self
	}

	/// Add an operation to the queue of operations to be processed
	pub fn add_operation(&mut self, info: CarveOperationInfo) {
		self.queue.push_back(info);
	}

	/// Processes the file at the front of the queue, returning true if one was processed, and false if there were none to be processed.
	/// Returns an error if one occurred. Also returns the carve operation info
	pub fn process_file(&mut self, output_dir: impl AsRef<str>) -> (Option<CarveOperationInfo>, Result<bool, Error>) {
		if let Some(info) = self.queue.pop_front() {
			let result = match info {
				CarveOperationInfo::Image { ref path, ref config, cluster_size, skip_carving } => {
					self.process_image_file(output_dir, &path, &config, cluster_size, skip_carving).map(|_| true)
				}
				CarveOperationInfo::FromLog { ref path } => {
					self.process_log_file(output_dir, &path).map(|_| true)
				}
			};

			(
				Some(info),
				result
			)
		} else {
			(None, Ok(false))
		}
	}

	pub fn process_image_file(&mut self, output_dir: impl AsRef<str>, path: &str, config: &SearchlightConfig, cluster_size: Option<u64>, skip_carving: bool) -> Result<(), Error> {
		let (mmap, file_len) = {
			let mut file = File::open(&path)?;

			let file_len = file_len(&mut file)?;

			info!("Opened image file {} (size: {} bytes)", &path, file_len);

			(
				unsafe { MmapOptions::new().map(&file)? },
				file_len
			)
		};

		assert_eq!(file_len, mmap.len() as u64);

		let (mut searcher, max_pat_len) = {
			let ac_table = AcTableBuilder::from_config(&config).build();

			(
				Search::new(ac_table.clone(), false),
				ac_table.max_pat_len as usize
			)
		};

		let block_size = searcher.max_search_size().unwrap_or(DEFAULT_BLOCK_SIZE);

		assert!(max_pat_len < block_size);

		let num_blocks = {
			let num_blocks = (file_len as usize - max_pat_len) / (block_size - max_pat_len);
			if file_len % block_size as u64 != 0 {
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
				// BUG: This is not really correct, as in, we want the progress report to go where the logs are going, without spamming lines, which is why
				//      we're using \r to repeatedly overwrite the line, but we can only do that to stdout or stderr. By default searchlight (the included
				//      binary crate) *does* write logs to stderr, but ideally we want libsearchlight to not depend on that behaviour to behave in a sensible
				//      way
				eprint!("\rProgress: {:.2}%", (i as f32 / num_blocks as f32) * 100.0);
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

		let id_ftype_map = &pairing::preprocess_config(&config);

		// Get the user-supplied cluster size or estimate it based off of headers
		// A None for cluster size here will indicate that the headers appear to be mostly not allocated on any usual cluster boundaries, or that
		// has been passed in as the case
		let cluster_size = cluster_size.unwrap_or_else(|| {
			let est = estimate_cluster_size(matches.iter().filter(|m| {
				if let Some((_, _, part)) = id_ftype_map.get(&m.id) {
					*part == MatchPart::Header
				} else {
					assert!(false);
					panic!() // assert!(false) is not detected as a control flow terminator/does not return ! but is more semantically correct
				}
			})).unwrap_or(1); // A cluster size of 1 is effectively the same as not being clustered

			info!("Calculated cluster size estimate: {est}");

			est
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

		let mut consumable_matches = matches.clone();
		let match_pairs = pair(&mut consumable_matches, id_ftype_map, true);

		info!("Searching complete: Found {} potential files ({} individual matches)", match_pairs.len(), num_matches);

		// Create output directory, erroring if it exists already
		fs::create_dir(output_dir.as_ref())?;

		let validator = DelegatingValidator::new();

		let mut num_carved_files = 0;

		let mut log = CarveLog::new(path);

		for pot_file in &match_pairs {
			let validation = validator.validate(&mmap, &pot_file, &matches, cluster_size as usize, &config);

			debug!("Potential file at {}-{} (type id {}) validated as: {}, with fragments {:?}", pot_file.start_idx, pot_file.end_idx + 1, pot_file.file_type.type_id, validation.validation_type, validation.fragments);

			if validation.validation_type != FileValidationType::Unrecognised {
				let fragments = if validation.fragments.is_empty() {
					vec![ (pot_file.start_idx..(pot_file.end_idx + 1)) ]
				} else {
					validation.fragments
				};

				// Get the minimum index and maximum index of all fragments and designate them the start and end idxs
				let start_idx = fragments.iter().min_by_key(|frag| frag.start).unwrap().start; // .map_or(pot_file.start_idx, |frag| frag.start);
				let end_idx = fragments.iter().max_by_key(|frag| frag.end).unwrap().end; // .map_or(pot_file.end_idx + 1, |frag| frag.end);

				// Filename format <start_idx>-<end_idx>.<extension>
				let filename = format!("{start_idx}-{end_idx}.{}",
					pot_file.file_type.extension.clone().unwrap_or("dat".to_string())
				);

				// Only write out the file content if the skip carving flag is false/not present
				if !skip_carving {
					// File to be placed at output_dir/validation_type/filename
					let filepath: PathBuf = [
						output_dir.as_ref(),
						&validation.validation_type.to_string(),
						&filename
					].iter().collect();

					// Create validation directory if it doesn't exist
					fs::create_dir_all(Path::new(&filepath).parent().unwrap())?;

					let mut file = File::create(filepath)?;

					file.write_vectored(
						&fragments.iter().map(|frag| IoSlice::new(&mmap[frag.start..frag.end])).collect::<Vec<IoSlice>>()
					)?;
				}

				// Add entry to log
				log.add_entry(pot_file.file_type.type_id, filename, validation.validation_type, fragments);

				num_carved_files += 1;

				// BUG: If some text is written to stderr or stdout between writes of the progress, then there will be no
				//      line break between the progress report and the output text. Put a space after the progress % to
				//      make that look less bad but I'm not sure if this is fixable, in a compelling way anyway
				if log_enabled!(Level::Info) {
					eprint!("\rProgress: {:.2}% ", (num_carved_files as f32 / match_pairs.len() as f32) * 100.0);
				}
			}
		}

		if !skip_carving {
			if log_enabled!(Level::Info) {
				eprint!("\n");
			}
			info!("{} successfully validated files exported to {}", num_carved_files, output_dir.as_ref());
		}

		log.write(output_dir.as_ref())?;

		info!("Carve log written to {}/log.txt", output_dir.as_ref());

		Ok(())
	}

	pub fn process_log_file(&mut self, output_dir: impl AsRef<str>, path: &str) -> Result<(), Error> {
		let log_file_str = fs::read_to_string(path)?;

		let log: CarveLog = serde_json::from_str(&log_file_str).map_err(|e| Error::LogReadError(e.to_string()))?;

		info!("Processing log \"{}\" - carving {} files from image at \"{}\"", path, log.files.len(), log.image_path);

		let mmap = {
			let mut file = File::open(&log.image_path)?;

			let file_len = file_len(&mut file)?;

			info!("Opened image file {} (size: {} bytes)", &log.image_path, file_len);

			unsafe { MmapOptions::new().map(&file)? }
		};

		for entry in &log.files {
			// File to be placed at output_dir/validation_type/filename
			let filepath: PathBuf = [
				output_dir.as_ref(),
				&entry.validation.to_string(),
				&entry.filename
			].iter().collect();

			// Create validation directory if it doesn't exist
			fs::create_dir_all(Path::new(&filepath).parent().unwrap())?;

			let mut file = File::create(filepath).unwrap();

			file.write_vectored(
				&entry.fragments.iter().map(|frag| IoSlice::new(&mmap[frag.start..frag.end])).collect::<Vec<IoSlice>>()
			)?;
		}

		info!("{} files exported to {}", log.files.len(), output_dir.as_ref());

		Ok(())
	}
}