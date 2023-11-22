use std::{fs::File, alloc::{self, Layout}, slice, io::{Read, Seek, SeekFrom, Write}, os::unix::fs::FileExt};

use crate::lib::io::DEFAULT_ALIGNMENT;

use super::{SeqIoBackend, file_len, BackendInfo, IoBackend, RandIoBackend, BackendError, AccessPattern};

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
	pub fn new(file_path: &str, read: bool, write: bool, access_pattern: AccessPattern, req_block_size: u64) -> Result<Self, BackendError> {
		let custom_flags = {
			#[cfg(target_os = "linux")]
			{ libc::O_DIRECT }
			#[cfg(not(target_os = "linux"))]
			{ 0 }
		};

		let mut file = super::open_with(file_path, read, write, access_pattern, custom_flags).map_err(|e| BackendError::IoError(e))?;
		let file_len = file_len(&mut file).map_err(|e| BackendError::IoError(e))?;

		// Need aligned memory of a size a multiple of the alignment for O_DIRECT - round upwards
		let block_size = (req_block_size as f64 / DEFAULT_ALIGNMENT as f64).ceil() as u64 * DEFAULT_ALIGNMENT as u64;
		assert_eq!(block_size % DEFAULT_ALIGNMENT as u64, 0);
		let mem_layout = Layout::from_size_align(block_size as usize, DEFAULT_ALIGNMENT).unwrap();
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

	fn write_next(&mut self, data: &[u8]) -> Result<(), BackendError> {
		self.file.write_all(data).map_err(|e| BackendError::IoError(e))
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

	fn write_region(&mut self, start: u64, data: &[u8]) -> Result<(), BackendError> {
		// BUG: Will subvert expectations by extending the file and this is also Linux exclusive
		self.file.write_at(data, start).map_err(|e| BackendError::IoError(e))?;

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