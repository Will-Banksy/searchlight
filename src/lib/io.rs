pub mod mmap;
pub mod filebuf;
#[cfg(unix)]
pub mod io_uring;
pub mod direct;

use std::{io::{self, Seek}, fs::File};

pub const DEFAULT_BLOCK_SIZE: u64 = 1 * 1024 * 1024 * 1024; // 1 GiB
pub const DEFAULT_ALIGNMENT: usize = 4096;

// TODO: Add writing and random access support to IoManager - Maybe also streamline it so that it doesn't support pluggable IO backends, but uses mmaps for reading large files and io_uring for reading/writing to lots of different files
// TODO: What if, for example, read_next simply queued a read and and the backend may give the function to another thread to call when the read is finished

pub trait IoBackend {
	/// Returns information about the opened file - Currently just the length of it
	fn backend_info(&self) -> BackendInfo;
}

pub trait SeqIoBackend: IoBackend {
	/// Read the next block of file data, calling the closure with an the read block as a slice, None if reached the EOF, or returning Err if an error occurred
	///
	/// This function uses a closure to allow the implementor to have more control over the lifetime and usage of the slice
	fn read_next<'a>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'a>) -> Result<(), BackendError>;
}

pub trait RandIoBackend: IoBackend {
	/// Read a region of the file, calling the closure with the read region as a slice, or returning an Err if an error occurred.
	/// Attempting to read a region partially or completely outside of the file address space is considered an error. Additionally, if the backend
	/// is unable to provide the entire region as a slice (such as the case of the region size exceeding the block size for a buffered reader),
	/// this is also considered an error. Before calling this function, be aware of the block size as reported by the backend, and on buffered backends
	/// don't try to read more than that.
	///
	/// This function uses a closure to allow the implementor to have more control over the lifetime and usage of the slice
	fn read_region<'a>(&mut self, start: u64, end: u64, f: Box<dyn FnOnce(&[u8]) + 'a>) -> Result<(), BackendError>;

	/// Read a region of the file, calling the closure with the read region as a slice, or returning an Err if an error occurred.
	/// This function is more lenient with what it considers errors - Attempting to read a region partially outside of the file
	/// address space is not considered an error: The read region is truncated to fit within the file address space where possible.
	/// Attempting to read a region completely outside the file address space however is still treated as an error
	///
	/// This function is implemented automatically in terms of backend_info and read_region.
	fn read_region_truncated<'a>(&mut self, start: u64, mut end: u64, f: Box<dyn FnOnce(&[u8]) + 'a>) -> Result<(), BackendError> {
		let info = self.backend_info();

		if end > info.file_len {
			end = info.file_len;
		}

		match self.read_region(start, end, Box::new(|block| {
			f(block)
		})) {
			Err(e) => Err(e),
			Ok(_) => Ok(())
		}
	}
}

pub enum BackendError { // TODO: Implement Error trait
	IoError(io::Error),
	RegionOutsideFileBounds,
	ZeroRangeSpecified,
	ThreadSendRecvError(String)
}

impl ToString for BackendError {
	fn to_string(&self) -> String {
		match self {
			Self::IoError(e) => format!("I/O error: {}", e.to_string()),
			BackendError::RegionOutsideFileBounds => format!("Specified region outside of file bounds"),
			BackendError::ZeroRangeSpecified => format!("Attempting to read zero bytes is an error"),
			BackendError::ThreadSendRecvError(e_str) => format!("Failed to communicate with other thread: {}", e_str)
		}
	}
}

pub struct BackendInfo {
	pub file_len: u64,
	pub block_size: u64,
	pub cursor: u64,
}

pub struct IoManager {
	req_block_size: u64,
	seq_io_backend: Option<Box<dyn SeqIoBackend>>,
	// rand_io_backend: Option<Box<dyn RandIoBackend>>
}

// pub enum IoBackendRole {
// 	SeqRead,
// 	RandRead,
// 	RandSeqRead,
// }

// impl IoBackendRole {
// 	const ROLE_ACCESS_SEQ: u8 = 0b0001;
// 	const ROLE_ACCESS_RAND: u8 = 0b0010;
// 	const ROLE_READ: u8 = 0b0100;
// 	const ROLE_WRITE: u8 = 0b1000;
// }

impl IoManager {
	/// Create a new IoManager that will initialise the backend requesting the default block size (`DEFAULT_BLOCK_SIZE`). Note
	/// that the backend may use a slightly different block size for memory layout purposes - This can be queried with
	/// `IoManager::backend_info().unwrap().block_size` once the backend is initialised
	pub fn new() -> Self {
		Self::new_with(DEFAULT_BLOCK_SIZE)
	}

	/// Create a new IoManager that will initialise the backend requesting the specified block size. Note
	/// that the backend may use a slightly different block size for memory layout purposes - This can be queried with
	/// `IoManager::backend_info().unwrap().block_size` once the backend is initialised
	pub fn new_with(req_block_size: u64) -> Self {
		IoManager { req_block_size, seq_io_backend: None }
	}

	/// Open a file with an automatically selected backend based on the file size: For sizes below 16KiB, it
	/// will use the `IoFileBuf` backend, for bigger sizes it'll use the `IoMmap` backend. This function will initialise the backend
	pub fn open_seq(&mut self, path: &str) -> Result<(), String> {
		// If the file size is more than 16KiB, use the memory mapped IoBackend
		// Otherwise, use the filebuf IoBackend
		let io_backend_cons = |file_path, req_block_size| {
			// Unfortunately need to open the file to determine it's size... Drop it immediately though
			let file_len = {
				let mut file = File::open(file_path).map_err(|e| e.to_string())?;
				file_len(&mut file)?
			};

			// Mmap improves file read speed compared to sequential read when over 16KiB of file size: https://stackoverflow.com/a/39196499/11009247
			// The filebuf backend seems to be *slightly* faster than the io_uring or direct backends at the moment too, while it's difficult to benchmark the mmap backend
			Ok(if file_len > (16 * 1024) {
				println!("[INFO]: Using I/O backend: IoMmap");
				mmap::IoMmap::new(file_path, req_block_size).map(|io_mmap| Box::new(io_mmap))? as Box<dyn SeqIoBackend>
			} else {
				println!("[INFO]: Using I/O backend: IoFileBuf");
				filebuf::IoFileBuf::new(file_path, req_block_size).map(|io_filebuf| Box::new(io_filebuf))?
			})
		};

		self.open_with_seq(path, io_backend_cons)
	}

	/// Open a file with a specific io backend, constructed using the passed-in closure with arguments: open file, file length, block size.
	/// This function will initialise the backend
	pub fn open_with_seq<'a, F>(&mut self, path: &'a str, backend_cons: F) -> Result<(), String> where F: FnOnce(&'a str, u64) -> Result<Box<dyn SeqIoBackend>, String> {
		// Get the io backend by calling the provided closure
		self.seq_io_backend = Some(backend_cons(path, self.req_block_size)?);

		Ok(())
	}

	/// Waits for the next block to be loaded by the backend, then calls the provided closure with the block slice, or None if
	/// the EOF is reached
	///
	/// Returns an Err if an error occurred in the backend or the backend hasn't been initialised (i.e. a file hasn't been opened),
	/// otherwise returns the return value of `f`
	pub fn with_next_block<'a, F, R>(&mut self, f: F) -> Result<R, String> where F: FnOnce(Option<&[u8]>) -> R + 'a {
		if let Some(ref mut io_backend) = self.seq_io_backend {
			let mut r: Option<R> = None;

			// Call the backend's next function, letting the caller of this function handle it, and extract the return value of the provided function
			io_backend.read_next(Box::new(|next| {
				r = Some(f(next))
			})).map_err(|e| format!("[ERROR] IoManager::with_next_block: Backend error on next: {}", e.to_string()))?;

			// Invalid state should not occur, since `f` should only not be called when there is an error,
			// and execution won't reach here if there is an error
			Ok(r.ok_or_else(|| panic!("[ERROR]: Invalid state")).unwrap())
		} else {
			Err("[ERROR]: Backend uninitialised (is a file open?)".to_string())
		}
	}

	/// Returns the progress through the file as a number between 0.0 and 1.0.
	/// Specifically, returns the last loaded address divided by the file length
	pub fn progress(&self) -> Option<f32> {
		let info = self.backend_info()?;
		Some(info.cursor as f32 / info.file_len as f32)
	}

	/// Returns an instance of `BackendInfo` if the backend is initialised, or None otherwise. BackendInfo contains information
	/// common to most backends that may be useful
	pub fn backend_info(&self) -> Option<BackendInfo> {
		if let Some(backend) = &self.seq_io_backend {
			Some(backend.backend_info())
		} else {
			None
		}
	}
}

/// Get the length of the file, by querying metadata and as a last resort seeking to the end of the file and getting the offset
pub fn file_len(file: &mut File) -> Result<u64, String> {
	if let Ok(metadata) = file.metadata() {
		Ok(metadata.len())
	} else {
		let size = file.seek(io::SeekFrom::End(0)).map_err(|e| e.to_string())?;
		file.seek(io::SeekFrom::Start(0)).map_err(|e| e.to_string())?;
		Ok(size)
	}
}

#[cfg(test)]
mod test {
    use super::{IoManager, filebuf, mmap, io_uring, direct};

	#[test]
	fn test_io_manager_filebuf() {
		let mut ioman = IoManager::new_with(10);

		ioman.open_with_seq("test_data/io_test.dat", |file_path, block_size| {
			Ok(filebuf::IoFileBuf::new(file_path, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open test_data/io_test.dat");

		test_io_manager(ioman, include_str!("../../test_data/io_test.dat"))
	}

	#[test]
	fn test_io_manager_mmap() {
		let mut ioman = IoManager::new_with(10);

		ioman.open_with_seq("test_data/io_test.dat", |file_path, block_size| {
			Ok(mmap::IoMmap::new(file_path, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open test_data/io_test.dat");

		test_io_manager(ioman, include_str!("../../test_data/io_test.dat"))
	}

	#[test]
	fn test_io_manager_io_uring() {
		let mut ioman = IoManager::new_with(10);

		ioman.open_with_seq("test_data/io_test.dat", |file_path, block_size| {
			Ok(io_uring::IoUring::new(file_path, block_size, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open test_data/io_test.dat");

		test_io_manager(ioman, include_str!("../../test_data/io_test.dat"))
	}

	#[test]
	fn test_io_manager_direct() {
		let mut ioman = IoManager::new_with(10);

		ioman.open_with_seq("test_data/io_test.dat", |file_path, block_size| {
			Ok(direct::IoDirect::new(file_path, block_size).map(|io_filebuf| Box::new(io_filebuf))?)
		}).expect("Failed to open test_data/io_test.dat");

		test_io_manager(ioman, include_str!("../../test_data/io_test.dat"))
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

