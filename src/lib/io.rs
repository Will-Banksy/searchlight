pub mod mmap;
pub mod filebuf;
#[cfg(target_os = "linux")]
pub mod io_uring;
pub mod direct;

use std::{io::{self, Seek}, fs::{File, OpenOptions}, collections::HashMap};
#[cfg(target_os = "linux")]
use std::os::{unix::prelude::OpenOptionsExt, fd::AsRawFd};

pub const DEFAULT_BLOCK_SIZE: u64 = 1 * 1024 * 1024 * 1024; // 1 GiB
pub const DEFAULT_ALIGNMENT: usize = 4096;

// TODO: What if, for example, read_next simply queued a read and and the backend may give the function to another thread to call when the read is finished

// TODO: After the changes to IoManager, benchmarking shows performance has regressed. This may be partially due to the performance impact of the hashmap
//     Investigate, and perhaps amortise the cost of calculating string hashes by A. using a faster hasher such as "ahash" or B. Calculate the hashes once,
//     and pass back to the user a "handle" (integer) that is the actual hashmap index

pub trait IoBackend {
	/// Returns information about the opened file - Currently just the length of it
	fn backend_info(&self) -> BackendInfo;
}

pub trait SeqIoBackend: IoBackend {
	/// Read the next block of file data, calling the closure with an the read block as a slice, None if reached the EOF, or returning Err if an error occurred.
	/// Implementors are required to uphold the guarantee that `f` is always called unless an error occurred, but the occurrence of an error does not
	/// guarantee that `f` was not called.
	///
	/// This function uses a closure to allow the implementor to have more control over the lifetime and usage of the slice
	fn read_next<'a>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'a>) -> Result<(), BackendError>;

	/// Write the provided data to the open file, returning an error if one occurred.
	/// This function will write from the cursor, extending the file if necessary
	fn write_next(&mut self, data: &[u8]) -> Result<(), BackendError>;
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

	/// Write the specified data to the open file at the specified index. Written data will be truncated to fit within file bounds -
	/// if the specified start position is >= file length, then no data will be written
	fn write_region(&mut self, start: u64, data: &[u8]) -> Result<(), BackendError>;// TODO: Implement in backends
}

pub trait RandSeqIoBackend: RandIoBackend + SeqIoBackend {}

impl<T> RandSeqIoBackend for T where T: RandIoBackend + SeqIoBackend {}

#[derive(Debug)]
pub enum BackendError {
	IoError(io::Error),
	RegionOutsideFileBounds,
	ZeroRangeSpecified,
	ThreadSendRecvError(String),
	UnsupportedOperation
}

impl ToString for BackendError {
	fn to_string(&self) -> String {
		match self {
			BackendError::IoError(e) => format!("I/O error: {}", e.to_string()),
			BackendError::RegionOutsideFileBounds => format!("Specified region outside of file bounds"),
			BackendError::ZeroRangeSpecified => format!("Attempting to read zero bytes is an error"),
			BackendError::ThreadSendRecvError(e_str) => format!("Failed to communicate with other thread: {}", e_str),
			BackendError::UnsupportedOperation => format!("Operation is unsupported by this backend")
		}
	}
}

#[derive(Debug)]
pub enum IoManagerError {
	BackendError(BackendError),
	InvalidOperation(String),
}

impl ToString for IoManagerError {
	fn to_string(&self) -> String {
		match self {
			IoManagerError::BackendError(e) => format!("Backend error: {}", e.to_string()),
			IoManagerError::InvalidOperation(msg) => format!("Invalid operation: {}", msg)
		}
	}
}

pub enum GenIoBackend {
	Rand(Box<dyn RandIoBackend>),
	Seq(Box<dyn SeqIoBackend>),
	RandSeq(Box<dyn RandSeqIoBackend>)
}

pub struct IoWorker {
	backend: GenIoBackend,
	read: bool,
	write: bool
}

impl IoWorker {
	pub fn backend_info(&self) -> BackendInfo {
		match &self.backend {
			GenIoBackend::Rand(b) => b.backend_info(),
			GenIoBackend::Seq(b) => b.backend_info(),
			GenIoBackend::RandSeq(b) => b.backend_info(),
		}
	}
}

pub struct BackendInfo {
	pub file_len: u64,
	pub block_size: u64,
	pub cursor: u64,
}

pub struct IoManager {
	// seq_io_backend: Option<Box<dyn SeqIoBackend>>,
	io_backends: HashMap<String, IoWorker>
	// rand_io_backend: Option<Box<dyn RandIoBackend>>
}

pub enum AccessPattern {
	Seq,
	Rand,
	RandSeq,
	Unspecified
}

impl IoManager {
	/// Create a new I/O manager to manage multiple read/write operations to different files
	pub fn new() -> Self {
		IoManager { io_backends: HashMap::new() }
	}

	/// Open a file in read/write mode, using an automatically selected backend that depends on the specified access pattern.
	/// A block size can also be requested of the backend to use, which it will not necessarily obey, which controls the size of buffers allocated by the backend,
	/// and the amount of data that is read by read_next, and the max amount of data that is read by read_region (backend-dependent: some backends such as
	/// the memory-mapped one do not limit the amount of data that can be read at once).
	///
	/// This function will return nothing if the file was successfully opened, and an error if one occurred. An error will be returned if read and write are both false,
	/// or if an error is returned by the backend, which is backend-dependent, but often because the file couldn't be opened, or a lack of permissions.
	pub fn open(&mut self, path: &str, read: bool, write: bool, access_pattern: AccessPattern, req_block_size: Option<u64>) -> Result<(), IoManagerError> {
		if !read && !write {
			return Err(IoManagerError::InvalidOperation("Cannot open a file in neither read or write mode".to_string()));
		}

		let req_block_size = req_block_size.unwrap_or(DEFAULT_BLOCK_SIZE);

		/// Macros for creating/inserting workers to avoid repeating code
		macro_rules! ins_worker {
			($backend_cons: expr, $geniobackend_variant: ident) => {
				self.io_backends.insert(path.to_string(),
					IoWorker {
						backend: GenIoBackend::$geniobackend_variant(Box::new(
							$backend_cons
						)),
						read,
						write,
					}
				);
			};
		}
		macro_rules! ins_worker_auto {
			($backend_struct: ty, $geniobackend_variant: ident) => {
				ins_worker!(
					<$backend_struct>::new(path, read, write, access_pattern, req_block_size).map_err(|e| IoManagerError::BackendError(e))?,
					$geniobackend_variant
				)
			};
		}

		// Each backend (except for the io_uring backend) is strong at different access patterns and reading/writing
		// Use the direct backend as fallback as that is decent at everything
		match access_pattern {
			AccessPattern::Seq => {
				// For sequential, filebuf is stronger at sequential reads, but can't do writes
				if read && !write {
					ins_worker_auto!(
						filebuf::IoFileBuf,
						Seq
					);
				} else {
					ins_worker_auto!(
						direct::IoDirect,
						Seq
					);
				}
			},
			AccessPattern::Rand => {
				// The mmap backend is (theoretically) the best at random reads/writes
				ins_worker_auto!(
					mmap::IoMmap,
					Rand
				);
			},
			// If the access pattern is unspecified, it's probably sensible to assume the caller might want random and sequential reads
			AccessPattern::RandSeq | AccessPattern::Unspecified => {
				ins_worker!(
					direct::IoDirect::new(path, read, write, AccessPattern::RandSeq, req_block_size).map_err(|e| IoManagerError::BackendError(e))?,
					RandSeq
				);
			},
		}

		Ok(())
	}

	/// Doesn't actually open the file, but adds the already initialised backend to this IoManager's database of open
	/// files, using `read` and `write` to know whether this backend is capable of reading/writing
	pub fn open_with(&mut self, path: &str, read: bool, write: bool, io_backend: GenIoBackend) {
		self.io_backends.insert(path.to_string(),
			IoWorker {
				backend: io_backend,
				read,
				write,
			}
		);
	}

	/// Close the file by dropping the backend
	pub fn close(&mut self, path: &str) {
		self.io_backends.remove(path);
	}

	/// Sequentially read the next block of the open file specified by `path`, calling `f` upon the read slice, or None if the
	/// EOF has been reached/there are no more bytes to read.
	///
	/// This function will return the return value of `f` or an error, which could be because the file was not opened, the file was not
	/// opened in read mode, the backend used for this file does not support sequential access, or because the backend returned an error
	pub fn read_next<'a, F, R>(&mut self, path: &str, f: F) -> Result<R, IoManagerError> where F: FnOnce(Option<&[u8]>) -> R + 'a {
		if let Some(worker) = self.io_backends.get_mut(path) {
			if !worker.read {
				Err(IoManagerError::InvalidOperation("File not opened in read mode".to_string()))
			} else {
				match &mut worker.backend {
					GenIoBackend::Seq(seq_backend) => {
						let mut r = None;
						seq_backend.read_next(Box::new(|block_opt| {
							r = Some(f(block_opt))
						})).map_err(|e| IoManagerError::BackendError(e))?;
						// unwrap here should never panic - `f` should always be called if an error was not returned, in which case it returns early
						Ok(r.unwrap())
					},
					GenIoBackend::RandSeq(seq_backend) => {
						let mut r = None;
						seq_backend.read_next(Box::new(|block_opt| {
							r = Some(f(block_opt))
						})).map_err(|e| IoManagerError::BackendError(e))?;
						// unwrap here should never panic - `f` should always be called if an error was not returned, in which case it returns early
						Ok(r.unwrap())
					},
					GenIoBackend::Rand(_) => Err(IoManagerError::InvalidOperation("I/O backend does not support tracked sequential access".to_string()))
				}
			}
		} else {
			Err(IoManagerError::InvalidOperation("File has not been opened".to_string()))
		}
	}

	/// Read the specified region of the open file specified by `path`, calling `f` upon the read slice, truncating the region if it
	/// partially lies outside of the file bounds.
	///
	/// This function will return the return value of `f` or an error, which could be because the file was not opened, the file was not
	/// opened in read mode, the backend used for this file does not support sequential access, the region specified was entirely outside
	/// of the file bounds, or because the backend returned an error
	pub fn read_region<'a, F, R>(&mut self, path: &str, start: u64, end: u64, f: F) -> Result<R, IoManagerError> where F: FnOnce(&[u8]) -> R + 'a {
		if let Some(worker) = self.io_backends.get_mut(path) {
			if !worker.read {
				Err(IoManagerError::InvalidOperation("File not opened in read mode".to_string()))
			} else {
				match &mut worker.backend {
					GenIoBackend::Seq(_) => Err(IoManagerError::InvalidOperation("I/O backend does not support random access".to_string())),
					GenIoBackend::RandSeq(rand_backend) => {
						let mut r = None;
						rand_backend.read_region_truncated(start, end, Box::new(|block_opt| {
							r = Some(f(block_opt))
						})).map_err(|e| IoManagerError::BackendError(e))?;
						// unwrap here should never panic - `f` should always be called if an error was not returned, in which case it returns early
						Ok(r.unwrap())
					},
					GenIoBackend::Rand(rand_backend) => {
						let mut r = None;
						rand_backend.read_region_truncated(start, end, Box::new(|block_opt| {
							r = Some(f(block_opt))
						})).map_err(|e| IoManagerError::BackendError(e))?;
						// unwrap here should never panic - `f` should always be called if an error was not returned, in which case it returns early
						Ok(r.unwrap())
					}
				}
			}
		} else {
			Err(IoManagerError::InvalidOperation("File has not been opened".to_string()))
		}
	}

	/// Write data to the open file from the current cursor position, extending the file where necessary. It will returnan  error if one occurred,
	/// which could be because the file was not opened, the file was not opened in write mode, the backend used for this file does not support
	/// sequential access, or because the backend returned an error
	pub fn write_next(&mut self, path: &str, data: &[u8]) -> Result<(), IoManagerError> {
		if let Some(worker) = self.io_backends.get_mut(path) {
			if !worker.write {
				Err(IoManagerError::InvalidOperation("File not opened in write mode".to_string()))
			} else {
				match &mut worker.backend {
					GenIoBackend::Seq(seq_backend) => {
						seq_backend.write_next(data).map_err(|e| IoManagerError::BackendError(e))?;
						Ok(())
					},
					GenIoBackend::RandSeq(seq_backend) => {
						seq_backend.write_next(data).map_err(|e| IoManagerError::BackendError(e))?;
						Ok(())
					},
					GenIoBackend::Rand(_) => Err(IoManagerError::InvalidOperation("I/O backend does not support tracked sequential access".to_string()))
				}
			}
		} else {
			Err(IoManagerError::InvalidOperation("File has not been opened".to_string()))
		}
	}

	/// Write data to the open file at the specified position
	pub fn write_region(&mut self, path: &str, start: u64, data: &[u8]) -> Result<(), IoManagerError> {
		if let Some(worker) = self.io_backends.get_mut(path) {
			if !worker.read {
				Err(IoManagerError::InvalidOperation("File not opened in read mode".to_string()))
			} else {
				match &mut worker.backend {
					GenIoBackend::Seq(_) => Err(IoManagerError::InvalidOperation("I/O backend does not support random access".to_string())),
					GenIoBackend::RandSeq(rand_backend) => {
						rand_backend.write_region(start, data).map_err(|e| IoManagerError::BackendError(e))?;
						Ok(())
					},
					GenIoBackend::Rand(rand_backend) => {
						rand_backend.write_region(start, data).map_err(|e| IoManagerError::BackendError(e))?;
						Ok(())
					}
				}
			}
		} else {
			Err(IoManagerError::InvalidOperation("File has not been opened".to_string()))
		}
	}

	/// Returns the progress through the specified open file as a number between 0.0 and 1.0.
	/// Specifically, returns the last loaded address divided by the file length
	pub fn progress(&self, path: &str) -> Option<f32> {
		let info = self.backend_info(path)?;
		Some(info.cursor as f32 / info.file_len as f32)
	}

	/// Returns an instance of `BackendInfo` from the backend for the specified open file, or None if the file is not open.
	/// `BackendInfo` contains information common to most backends that may be useful
	pub fn backend_info(&self, path: &str) -> Option<BackendInfo> {
		if let Some(backend) = &self.io_backends.get(path) {
			Some(backend.backend_info())
		} else {
			None
		}
	}
}

/// Opens a file for reading/writing (as specified), with specified unix custom flags (see man page for open(2) - mainly of interest
/// is the O_DIRECT flag) and informing the OS to optimise file reading for the specified access pattern (last two are currently only
/// implemented on Linux)
pub fn open_with(path: &str, read: bool, write: bool, access_pattern: AccessPattern, custom_flags: i32) -> Result<File, io::Error> {
	let mut open_opts = OpenOptions::new();
	open_opts.read(read);
	open_opts.write(write);
	open_opts.create(write);
	#[cfg(target_os = "linux")]
	{
		open_opts.custom_flags(custom_flags);
	}

	let file = open_opts.open(path);

	#[cfg(target_os = "linux")]
	{
		if let Ok(file) = &file {
			let advice = match access_pattern {
				AccessPattern::Seq => libc::POSIX_FADV_SEQUENTIAL,
				AccessPattern::Rand => libc::POSIX_FADV_RANDOM,
				AccessPattern::RandSeq => libc::POSIX_FADV_SEQUENTIAL, // Can't really optimise for random without deoptimising for sequential so optimise for sequential
				AccessPattern::Unspecified => libc::POSIX_FADV_NORMAL,
			};

			unsafe {
				libc::posix_fadvise(file.as_raw_fd(), 0, 0, advice);
			}
		}
	}

	file
}

/// Get the length of the file, by querying metadata and as a last resort seeking to the end of the file and getting the offset
pub fn file_len(file: &mut File) -> Result<u64, io::Error> {
	if let Ok(metadata) = file.metadata() {
		Ok(metadata.len())
	} else {
		let size = file.seek(io::SeekFrom::End(0))?;
		file.seek(io::SeekFrom::Start(0))?;
		Ok(size)
	}
}

// TODO: Test sequential and random writing, and test random reading. Also test automatically selected backends?
#[cfg(test)]
mod test {
    use super::{IoManager, filebuf, mmap, direct, AccessPattern};

	#[test]
	fn test_io_manager_filebuf() {
		let mut ioman = IoManager::new();

		let path = "test_data/io_test.dat";
		let block_size = 10;

		ioman.open_with(path, true, false, {
			super::GenIoBackend::Seq(
				filebuf::IoFileBuf::new(path, true, false, AccessPattern::Seq, block_size).map(|io_filebuf| Box::new(io_filebuf)).expect("Failed to open test_data/io_test.dat")
			)
		});

		test_io_manager(ioman, path, include_str!("../../test_data/io_test.dat"))
	}

	#[test]
	fn test_io_manager_mmap() {
		let mut ioman = IoManager::new();

		let path = "test_data/io_test.dat";
		let block_size = 10;

		ioman.open_with(path, true, false, {
			super::GenIoBackend::Seq(
				mmap::IoMmap::new(path, true, false, AccessPattern::Seq, block_size).map(|io_filebuf| Box::new(io_filebuf)).expect("Failed to open test_data/io_test.dat")
			)
		});

		test_io_manager(ioman, path, include_str!("../../test_data/io_test.dat"))
	}

	#[test]
	#[cfg(target_os = "linux")]
	fn test_io_manager_io_uring() {
    use super::io_uring;

		let mut ioman = IoManager::new();

		let path = "test_data/io_test.dat";
		let block_size = 10;

		ioman.open_with(path, true, false, {
			super::GenIoBackend::Seq(
				io_uring::IoUring::new(path, true, false, AccessPattern::Seq, block_size, block_size).map(|io_filebuf| Box::new(io_filebuf)).expect("Failed to open test_data/io_test.dat")
			)
		});

		test_io_manager(ioman, path, include_str!("../../test_data/io_test.dat"))
	}

	#[test]
	fn test_io_manager_direct() {
		let mut ioman = IoManager::new();

		let path = "test_data/io_test.dat";
		let block_size = 10;

		ioman.open_with(path, true, false, {
			super::GenIoBackend::Seq(
				direct::IoDirect::new(path, true, false, AccessPattern::Seq, block_size).map(|io_filebuf| Box::new(io_filebuf)).expect("Failed to open test_data/io_test.dat")
			)
		});

		test_io_manager(ioman, path, include_str!("../../test_data/io_test.dat"))
	}

	#[cfg(test)]
	fn test_io_manager(mut ioman: IoManager, path: &str, test_str: &str) {
		let mut sb = String::new();

		loop {
			let eof = ioman.read_next(path, |next| {
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

