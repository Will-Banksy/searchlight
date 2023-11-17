use std::{fs::File, os::fd::AsRawFd};

use memmap::{Mmap, MmapOptions};

use super::{SeqIoBackend, file_len, BackendInfo, IoBackend, RandIoBackend, BackendError, AccessPattern};

pub struct IoMmap {
	file: File,
	file_len: u64,
	mmap: Mmap,
	cursor: u64,
	block_size: u64
}

impl IoMmap {
	pub fn new(file_path: &str, read: bool, write: bool, access_pattern: AccessPattern, block_size: u64) -> Result<Self, BackendError> {
		let mut file = super::open_with(file_path, read, write, access_pattern, 0).map_err(|e| BackendError::IoError(e))?;
		let file_len = file_len(&mut file).map_err(|e| BackendError::IoError(e))?;

		let mmap = unsafe { MmapOptions::new().map(&file).map_err(|e| BackendError::IoError(e))? };

		#[cfg(unix)]
		unsafe {
			libc::madvise(mmap.as_ptr() as *mut libc::c_void, mmap.len(), libc::MADV_SEQUENTIAL);
		}

		Ok(IoMmap {
			file,
			file_len,
			mmap,
			cursor: 0,
			block_size
		})
	}
}

impl IoBackend for IoMmap {
	fn backend_info(&self) -> BackendInfo {
		BackendInfo {
			file_len: self.file_len as u64,
			block_size: self.block_size,
			cursor: self.cursor
		}
	}
}

impl SeqIoBackend for IoMmap {
	fn read_next<'a>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'a>) -> Result<(), BackendError> {
		let start = self.cursor;
		let end = if self.cursor + self.block_size < self.file_len {
			self.cursor + self.block_size
		} else {
			self.file_len
		};
		let ret = if start == end {
			Ok(f(None))
		} else {
			Ok(f(Some(&self.mmap[start as usize..end as usize])))
		};
		self.cursor = end;
		ret
	}

	fn write_next(&mut self, data: &[u8]) -> Result<(), BackendError> {
		// Unimplemented because cannot satisfy the requirements of this method
		unimplemented!("Cannot grow memory mapped files")
	}
}

impl RandIoBackend for IoMmap {
	fn read_region<'a>(&mut self, start: u64, end: u64, f: Box<dyn FnOnce(&[u8]) + 'a>) -> Result<(), BackendError> {
		if end > self.file_len || start >= end {
			return Err(BackendError::RegionOutsideFileBounds)
		}

		f(&self.mmap[start as usize..end as usize]);

		Ok(())
	}
}

impl Drop for IoMmap {
	fn drop(&mut self) {
		// NOTE: Left in for benchmarking - Instruct linux to discard cached file data
		#[cfg(unix)]
		unsafe {
			libc::posix_fadvise(self.file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
		}
	}
}