use std::{fs::{OpenOptions, File}, alloc::{self, Layout}, slice, os::{unix::prelude::OpenOptionsExt, fd::AsRawFd}};

use rio::{Rio, Completion};

use crate::lib::io::DEFAULT_ALIGNMENT;

use super::{SeqIoBackend, file_len, BackendInfo, BackendError, IoBackend};

// TODO: Test using a read queing strategy more similar to OpenForensics
//     How I *think* file reading works in OpenForensics is that instead of queing a read for an entire chunk
//     at a time, it queues reads for multiple sub-chunks of that chunk

pub struct IoUring<'a, 'c> {
	buf: &'a mut [u8],
	mem_layout: Layout,
	ring: Rio,
	file: File,
	file_len: u64,
	prev_completion: Option<Completion<'c, usize>>,
	cursor: u64
}

impl<'a, 'c> IoUring<'a, 'c> {
	/// Opens the file specified by file_path, using a buffer of size the specified block size, using the O_DIRECT flag
	///
	/// Note that the actual block size used may be changed
	pub fn new(file_path: &str, block_size: u64) -> Result<Self, String> {
		// Open file with O_DIRECT and query length of file
		let mut file = OpenOptions::new().custom_flags(libc::O_DIRECT).read(true).open(file_path).map_err(|e| e.to_string())?;
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

		let ring = rio::new().map_err(|e| e.to_string())?;

		let cursor = 0;

		let mut io_uring = IoUring {
			buf,
			mem_layout,
			ring,
			file,
			file_len,
			prev_completion: None,
			cursor
		};

		// Need unsafe transmute cause rust doesn't allow self-referential structs
		req_next(unsafe {
			std::mem::transmute(&mut io_uring)
		});

		Ok(io_uring)
	}
}

/// Queues a read into IoUring's buf using io_uring through rio
///
/// Returns a bool indicating whether an operation was queued or not... which is currently unused
pub fn req_next<'a, 'c>(uring: &'a mut IoUring<'a, 'c>) -> bool where 'a: 'c {
	if uring.cursor >= uring.file_len {
		false
	} else {
		uring.prev_completion = Some(uring.ring.read_at(&uring.file, &uring.buf, uring.cursor));
		// uring.ring.submit_all();
		true
	}
}

impl<'a, 'c> IoBackend for IoUring<'a, 'c> where 'a: 'c {
	fn backend_info(&self) -> BackendInfo {
		BackendInfo {
			file_len: self.file_len,
			block_size: self.mem_layout.size() as u64,
			cursor: self.cursor
		}
	}
}

impl<'a, 'c> SeqIoBackend for IoUring<'a, 'c> where 'a: 'c {
	fn read_next<'b>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'b>) -> Result<(), BackendError> {
		// If there is a queued operation, await that
		if let Some(completion) = self.prev_completion.take() {
			let bytes_read = completion.wait().map_err(|e| BackendError::IoError(e))?;

			// Call f with the appropriate argument
			if bytes_read == 0 {
				f(None)
			} else {
				f(Some(&self.buf[0..bytes_read]))
			}

			// And increment the cursor
			self.cursor += bytes_read as u64;
		} else {
			// Else if there was no queued operation, just call f with none
			f(None)
		}

		// Need unsafe transmute cause rust doesn't allow self-referential structs
		req_next(unsafe {
			std::mem::transmute(self)
		});

		Ok(())
	}
}

impl<'a, 'c> Drop for IoUring<'a, 'c> {
	fn drop(&mut self) {
		// Await the current io operation, lest it use the buffer after it's freed
		if let Some(completion) = self.prev_completion.take() {
			completion.wait().unwrap_or_default();
		}

		// Deallocate the aligned memory
		unsafe {
			alloc::dealloc(self.buf.as_mut_ptr(), self.mem_layout);
		}
	}
}