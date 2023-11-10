use std::{fs::File, os::fd::AsRawFd};

use memmap::{Mmap, MmapOptions};

use super::{IoBackend, file_len, BackendInfo};

pub struct IoMmap {
	file: File,
	file_len: u64,
	mmap: Mmap,
	cursor: u64,
	block_size: u64
}

impl IoMmap {
	pub fn new(file_path: &str, block_size: u64) -> Result<Self, String> {
		let mut file = File::open(file_path).map_err(|e| e.to_string())?;
		let file_len = file_len(&mut file)?;

		#[cfg(unix)]
		unsafe {
			libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_SEQUENTIAL);
		}

		let mmap = unsafe { MmapOptions::new().map(&file).map_err(|e| e.to_string())? };

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
	fn file_info(&self) -> BackendInfo {
		BackendInfo {
			file_len: self.file_len as u64,
			block_size: self.block_size
		}
	}

	fn read_next<'a>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'a>) -> Result<(), String> {
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