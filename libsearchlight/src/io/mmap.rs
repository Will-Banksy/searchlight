use std::fs::File;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;

use memmap::{MmapOptions, MmapMut, Mmap};

use super::{SeqIoBackend, file_len, BackendInfo, IoBackend, RandIoBackend, BackendError, AccessPattern};

pub enum MmapType {
	Mut(MmapMut),
	Const(Mmap)
}

pub struct IoMmap {
	file: File,
	file_len: u64,
	mmap: MmapType,
	cursor: u64,
	block_size: u64
}

impl IoMmap {
	pub fn new(file_path: &str, read: bool, write: bool, access_pattern: AccessPattern, block_size: u64) -> Result<Self, BackendError> {
		let mut file = super::open_with(file_path, read, write, access_pattern, None).map_err(|e| BackendError::IoError(e))?;
		let file_len = file_len(&mut file).map_err(|e| BackendError::IoError(e))?;

		let mmap = {
			if write {
				MmapType::Mut(unsafe { MmapOptions::new().map_mut(&file).map_err(|e| BackendError::IoError(e))? })
			} else {
				MmapType::Const(unsafe { MmapOptions::new().map(&file).map_err(|e| BackendError::IoError(e))? })
			}
		};

		#[cfg(target_os = "linux")]
		unsafe {
			match &mmap {
				MmapType::Mut(mmap) => { libc::madvise(mmap.as_ptr() as *mut libc::c_void, mmap.len(), libc::MADV_SEQUENTIAL); },
				MmapType::Const(mmap) => { libc::madvise(mmap.as_ptr() as *mut libc::c_void, mmap.len(), libc::MADV_SEQUENTIAL); }
			}
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
		// Simply read from the mmap as if it were a simple slice, from the cursor to the cursor position + the block size

		// Initially do some calculations to make sure we are not overstepping
		let start = self.cursor;
		let end = if self.cursor + self.block_size < self.file_len {
			self.cursor + self.block_size
		} else {
			self.file_len
		};

		// Call f with None if there are no bytes to read or with Some with the slice from the mmap
		let ret = if start == end {
			Ok(f(None))
		} else {
			match &self.mmap {
				MmapType::Mut(mmap) => { Ok(f(Some(&mmap[start as usize..end as usize]))) },
				MmapType::Const(mmap) => { Ok(f(Some(&mmap[start as usize..end as usize]))) },
			}
		};
		self.cursor = end;
		ret
	}

	fn write_next(&mut self, _: &[u8]) -> Result<(), BackendError> {
		// Unimplemented/unsupported because cannot satisfy the requirements of this method
		// unimplemented!("Cannot grow memory mapped files")
		Err(BackendError::UnsupportedOperation)
	}
}

impl RandIoBackend for IoMmap {
	fn read_region<'a>(&mut self, start: u64, end: u64, f: Box<dyn FnOnce(&[u8]) + 'a>) -> Result<(), BackendError> {
		// Calculate whether the requested read region is completely outside the file bounds, returning an Err if so
		// Otherwise, calculate whether the requested region is partially outside the file bounds or not and do the appropriate calculations
		// to read bytes without going past the file length
		if start >= end || start > self.file_len {
			return Err(BackendError::RegionOutsideFileBounds)
		} else if end > self.file_len {
			let start = start as usize;
			let len = self.file_len as usize - start;
			let end = start as usize + len;
			// Call f with the truncated mmapped slice
			match &self.mmap {
				MmapType::Mut(mmap) => f(&mmap[start..end]),
				MmapType::Const(mmap) => f(&mmap[start..end])
			}
			Ok(())
		} else {
			// Call f with the requested mmapped slice
			match &self.mmap {
				MmapType::Mut(mmap) => f(&mmap[start as usize..end as usize]),
				MmapType::Const(mmap) => f(&mmap[start as usize..end as usize])
			}
			Ok(())
		}
	}

	fn write_region(&mut self, start: u64, data: &[u8]) -> Result<u64, BackendError> {
		// Calculate whether the requested write region is completely outside the file bounds, returning an Err if so
		// Otherwise, calculate whether the requested region is partially outside of the file bounds or not and do the appropriate calculations
		// to write bytes without going past the file length
		if start >= self.file_len as u64 {
			Err(BackendError::RegionOutsideFileBounds)
		} else if start + data.len() as u64 > self.file_len as u64 {
			let start = start as usize;
			let len = data.len() - start as usize;
			let end = start as usize + len;
			// Write the truncated number of bytes to the mmapped slice
			if let MmapType::Mut(ref mut mmap) = self.mmap {
				(&mut mmap[start..end]).copy_from_slice(&data[start..(start + len)]);
				Ok(len as u64)
			} else {
				Err(BackendError::InvalidOperation)
			}
		} else {
			// Write the requested number of bytes to the mmapped slice
			if let MmapType::Mut(ref mut mmap) = self.mmap {
				mmap.copy_from_slice(data);
				Ok(data.len() as u64)
			} else {
				Err(BackendError::InvalidOperation)
			}
		}
	}
}

impl Drop for IoMmap {
	fn drop(&mut self) {
		// NOTE: Left in for benchmarking - Instruct linux to discard cached file data
		#[cfg(target_os = "linux")]
		unsafe {
			libc::posix_fadvise(self.file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
		}
	}
}