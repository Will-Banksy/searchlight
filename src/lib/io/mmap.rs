use std::{fs::File, os::fd::AsRawFd};

use memmap::{Mmap, MmapOptions};

use super::IoBackend;

pub struct IoMmap {
	file: File,
	file_len: usize,
	mmap: Mmap,
	cursor: usize,
	block_size: usize
}

impl IoMmap {
	pub fn new(file: File, file_len: u64, block_size: u64) -> Result<Self, String> {
		let mmap = unsafe { MmapOptions::new().map(&file).map_err(|e| e.to_string())? };

		#[cfg(unix)]
		unsafe {
			libc::madvise(mmap.as_ptr() as *mut libc::c_void, mmap.len(), libc::MADV_SEQUENTIAL);
		}

		Ok(IoMmap {
			file,
			file_len: file_len as usize,
			mmap,
			cursor: 0,
			block_size: block_size as usize
		})
	}
}

impl IoBackend for IoMmap {
	fn file_info(&self) -> u64 {
		self.file_len as u64
	}

	fn next<'a>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'a>) -> Result<(), String> {
		let start = self.cursor;
		let end = if self.cursor + self.block_size < self.file_len {
			self.cursor + self.block_size
		} else {
			self.file_len
		};
		let ret = if start == end {
			Ok(f(None))
		} else {
			Ok(f(Some(&self.mmap[start..end])))
		};
		self.cursor = end;
		ret
	}
}

impl Drop for IoMmap {
	fn drop(&mut self) {
		// NOTE: Left in for benchmarking
		#[cfg(unix)]
		unsafe {
			libc::posix_fadvise(self.file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
		}
	}
}