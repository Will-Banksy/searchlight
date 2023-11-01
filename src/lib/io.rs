pub mod mmap;
pub mod filebuf;

use std::{io::{self, Seek}, fs::File};

#[cfg(unix)]
use std::os::fd::AsRawFd;

const DEFAULT_BLOCK_SIZE: u64 = 8192; // Got from BUFSIZ in stdio.h // 1 * 1024 * 1024 * 1024; // 1 GiB

// TODO: Test how long the main thread waits on the io_thread
// https://stackoverflow.com/a/39196499/11009247

pub trait IoBackend {
	/// Returns information about the opened file - Currently just the length of it
	fn file_info(&self) -> u64;
	/// Read the next block of file data, calling the closure with an the read block as a slice, None if reached the EOF, or Err if an error occurred
	///
	/// This function uses a closure to allow the implementor to have more control over the lifetime and usage of the slice
	fn next<'a>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'a>) -> Result<(), String>; // Needs to take a boxed function to make it object safe
	/// Optionally, this method should start a thread for preloading
	fn start_preload_thread(&mut self) -> Result<(), String> {
		Ok(())
	}
}

pub struct IoManager {
	block_size: u64,
	file_len: Option<u64>,
	io_backend: Option<Box<dyn IoBackend>>
}

impl IoManager {
	pub fn new() -> Self {
		Self::new_with(DEFAULT_BLOCK_SIZE)
	}

	pub fn new_with(block_size: u64) -> Self {
		IoManager { block_size, file_len: None, io_backend: None }
	}

	/// Open a file with an automatically selected backend based on the file size: For sizes below 16KiB, it
	/// will use the `IoFileBuf` backend, for bigger sizes it'll use the `IoMmap` backend
	pub fn open(&mut self, path: &str) -> Result<(), String> {
		// If the file size is more than 16KiB, use the memory mapped IoBackend
		// Otherwise, use the filebuf IoBackend
		// NOTE: Since it's only 16KiB... is it worth agonising over getting the filebuf one perfect?
		let io_backend_cons = |file, file_len, block_size| {
			Ok(if file_len > (16 * 1024) { // https://stackoverflow.com/a/39196499/11009247
				println!("[INFO]: Using I/O backend: IoMmap");
				mmap::IoMmap::new(file, file_len, block_size).map(|io_mmap| Box::new(io_mmap))? as Box<dyn IoBackend>
			} else {
				println!("[INFO]: Using I/O backend: IoFileBuf");
				filebuf::IoFileBuf::new(file, file_len, block_size).map(|io_filebuf| Box::new(io_filebuf))?
			})
		};

		self.open_with(path, io_backend_cons)
	}

	/// Open a file with a specific io backend, constructed using the passed-in closure with arguments: open file, file length, block size
	pub fn open_with<F>(&mut self, path: &str, backend_cons: F) -> Result<(), String> where F: FnOnce(File, u64, u64) -> Result<Box<dyn IoBackend>, String> {
		let mut file = File::open(path).map_err(|e| e.to_string())?;

		#[cfg(unix)]
		unsafe {
			libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_SEQUENTIAL);
		}

		// Get the length of the file, by querying metadata and as a last resort seeking to the end of the file and getting the offset
		let file_len = {
			if let Ok(metadata) = file.metadata() {
				metadata.len()
			} else {
				let size = file.seek(io::SeekFrom::End(0)).map_err(|e| e.to_string())?;
				file.seek(io::SeekFrom::Start(0)).map_err(|e| e.to_string())?;
				size
			}
		};

		// Get the io backend by calling the provided closure
		self.io_backend = Some(backend_cons(file, file_len, self.block_size)?);

		// Just start the preload thread immediately
		if let Some(ref mut io_backend) = self.io_backend {
			io_backend.start_preload_thread().unwrap_or_else(|_| eprintln!("[WARN]: Preloading thread failed to start")); // Just ignoring errors for starting the preload thread
		}

		self.file_len = Some(file_len);

		Ok(())
	}

	/// Waits for the next block to be loaded by the backend, then calls the provided closure with the block slice, or None if
	/// the EOF is reached
	///
	/// Returns an Err if an error occurred in the backend or the backend hasn't been initialised (i.e. a file hasn't been opened),
	/// otherwise returns the return value of `f`
	pub fn with_next_block<'a, F, R>(&mut self, f: F) -> Result<R, String> where F: FnOnce(Option<&[u8]>) -> R + 'a {
		if let Some(ref mut io_backend) = self.io_backend {
			let mut r: Option<R> = None;

			// Call the backend's next function, letting the caller of this function handle it, and extract the return value of the provided function
			io_backend.next(Box::new(|next| {
				r = Some(f(next))
			}))?;

			// Invalid state should not occur, since `f` should only not be called when there is an error,
			// and execution won't reach here if there is an error
			Ok(r.ok_or_else(|| panic!("[ERROR]: Invalid state")).unwrap())
		} else {
			Err("[ERROR]: Backend uninitialised (is a file open?)".to_string())
		}
	}

	/// Returns the progress through the file as a number between 0.0 and 1.0.
	/// Specifically, returns the last loaded address divided by the file length
	pub fn progress(&self) -> f32 {
		todo!() // TODO: This will require either logic in IoManager or an impl in IoBackend (tradeoffs?)
		// if let Some(file_len) = self.file_len {
		// 	(((self.curr_block_idx - 1) * self.block_size + self.curr_block_bytes_read) as f32) / (file_len as f32)
		// } else {
		// 	0.0
		// }
	}

	/// Returns the length of the opened file in bytes, or none if a file hasn't been opened
	pub fn file_len(&self) -> Option<u64> {
		self.file_len
	}

	/// Returns the block size
	pub fn block_size(&self) -> u64 {
		self.block_size
	}
}

#[cfg(test)]
mod test {
    use super::{IoManager, filebuf, mmap};

	#[test]
	fn test_io_manager_filebuf() {
		let mut ioman = IoManager::new_with(10);

		ioman.open_with("Cargo.toml", |file, file_len, block_size| {
			Ok(filebuf::IoFileBuf::new(file, file_len, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open Cargo.toml");

		test_io_manager(ioman, include_str!("../../Cargo.toml"))
	}

	#[test]
	fn test_io_manager_mmap() {
		let mut ioman = IoManager::new_with(10);

		ioman.open_with("Cargo.toml", |file, file_len, block_size| {
			Ok(mmap::IoMmap::new(file, file_len, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open Cargo.toml");

		test_io_manager(ioman, include_str!("../../Cargo.toml"))
	}

	#[cfg(test)]
	fn test_io_manager(mut ioman: IoManager, test_str: &str) {
		let mut sb = String::new();

		loop {
			let eof = ioman.with_next_block(|next| {
				match next {
					Some(block) => {
						sb.push_str(std::str::from_utf8(block).unwrap());
						false
					},
					None => {
						true
					}
				}
			}).unwrap();

			if eof {
				break;
			}
		}

		assert_eq!(sb, test_str)
	}
}

