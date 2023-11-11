use std::{fs::{OpenOptions, File}, alloc::{self, Layout}, slice, os::{unix::prelude::OpenOptionsExt, fd::AsRawFd}, io::{Read, Seek, SeekFrom}};

use crate::lib::io::DEFAULT_ALIGNMENT;

use super::{SeqIoBackend, file_len, BackendInfo, IoBackend, RandIoBackend, BackendError};

pub struct IoDirect<'a> {
	buf: &'a mut [u8],
	mem_layout: Layout,
	file: File,
	file_len: u64,
	cursor: u64
}

impl<'a> IoDirect<'a> {
	/// Opens the file specified by file_path, using a buffer of size the specified block size, using the O_DIRECT flag
	///
	/// Note that the actual block size used may be changed
	pub fn new(file_path: &str, block_size: u64) -> Result<Self, String> {
		let mut open_opts = OpenOptions::new();
		open_opts.read(true);

		// If on linux, use the O_DIRECT flag to avoid caching and copying since we're doing our own buffering
		#[cfg(unix)]
		{
			open_opts.custom_flags(libc::O_DIRECT);
		}

		// Open the file and get it's length
		let mut file = open_opts.open(file_path).map_err(|e| e.to_string())?;
		let file_len = file_len(&mut file)?;

		#[cfg(unix)]
		unsafe {
			libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_SEQUENTIAL);
		}

		// Need aligned memory of a size a multiple of the alignment for O_DIRECT - round upwards
		let block_size = (block_size as f64 / DEFAULT_ALIGNMENT as f64).ceil() as u64 * DEFAULT_ALIGNMENT as u64;
		assert_eq!(block_size % DEFAULT_ALIGNMENT as u64, 0);
		let mem_layout = Layout::from_size_align(block_size as usize, DEFAULT_ALIGNMENT).map_err(|e| e.to_string())?;
		let buf = unsafe {
			slice::from_raw_parts_mut(
				alloc::alloc(mem_layout),
				block_size as usize
			)
		};

		Ok(IoDirect {
			buf,
			mem_layout,
			file,
			file_len,
			cursor: 0
		})
	}
}

impl<'a> IoBackend for IoDirect<'a> {
	fn backend_info(&self) -> BackendInfo {
		BackendInfo {
			file_len: self.file_len,
			block_size: self.mem_layout.size() as u64,
			cursor: self.cursor
		}
	}
}

impl<'a> SeqIoBackend for IoDirect<'a> {
	fn read_next<'b>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'b>) -> Result<(), BackendError> {
		let bytes_read = self.file.read(self.buf).map_err(|e| BackendError::IoError(e))?;

		if bytes_read == 0 {
			f(None)
		} else {
			f(Some(&self.buf[0..bytes_read]));

			self.cursor += bytes_read as u64;
		}

		Ok(())
	}
}

impl<'a> RandIoBackend for IoDirect<'a> {
	fn read_region<'b>(&mut self, start: u64, end: u64, f: Box<dyn FnOnce(&[u8]) + 'b>) -> Result<(), BackendError> {
		if end > self.file_len || start >= end || (end - start) > self.buf.len() as u64 {
			return Err(BackendError::RegionOutsideFileBounds)
		}
		if end == start {
			return Err(BackendError::ZeroRangeSpecified)
		}

		let prev_cursor = self.cursor;
		self.file.seek(SeekFrom::Start(start)).map_err(|e| BackendError::IoError(e))?;

		let bytes_read = self.file.read(self.buf).map_err(|e| BackendError::IoError(e))?;

		f(&self.buf[0..bytes_read]);

		self.file.seek(SeekFrom::Start(prev_cursor)).map_err(|e| BackendError::IoError(e))?;

		Ok(())
	}
}

impl<'a> Drop for IoDirect<'a> {
	fn drop(&mut self) {
		// Deallocate the aligned memory
		unsafe {
			alloc::dealloc(self.buf.as_mut_ptr(), self.mem_layout);
		}
	}
}