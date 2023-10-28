use std::fs::File;

use memmap::{Mmap, MmapOptions};

use super::IoBackend;

pub struct IoMmap {
	file_len: usize,
	mmap: Mmap,
	cursor: usize,
	block_size: usize
}

impl IoMmap {
	pub fn new(file: File, file_len: u64, block_size: u64) -> Result<Self, String> {
		let mmap = unsafe { MmapOptions::new().map(&file).map_err(|e| e.to_string())? };

		Ok(IoMmap {
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

	fn next(&mut self) -> Result<Option<&[u8]>, String> {
		let start = self.cursor;
		let end = if self.cursor + self.block_size < self.file_len {
			self.cursor + self.block_size
		} else {
			self.file_len
		};
		if start == end {
			Ok(None)
		} else {
			Ok(Some(&self.mmap[start..end]))
		}
	}
}