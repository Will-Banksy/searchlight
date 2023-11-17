use std::{fs::File, alloc::{self, Layout}, slice, collections::VecDeque};

use rio::{Rio, Completion};

use crate::lib::io::DEFAULT_ALIGNMENT;

use super::{SeqIoBackend, file_len, BackendInfo, BackendError, IoBackend, AccessPattern};

pub const DEFAULT_URING_READ_SIZE: usize = DEFAULT_ALIGNMENT * 16; // DEFAULT_BLOCK_SIZE as usize;// DEFAULT_ALIGNMENT * 320;

// TODO: Test using a read queing strategy more similar to OpenForensics
//     How I *think* file reading works in OpenForensics is that instead of queing a read for an entire chunk
//     at a time, it queues reads for multiple sub-chunks of that chunk

pub struct IoUring<'a, 'c> {
	buf: &'a mut [u8],
	write_buffer: Vec<u8>,
	mem_layout: Layout,
	ring: Rio,
	file: File,
	file_len: u64,
	read_completions: VecDeque<Completion<'c, usize>>, // Option<Completion<'c, usize>>,
	write_completions: VecDeque<Completion<'c, usize>>,
	cursor: u64,
	uring_read_size: u64
}

impl<'a, 'c> IoUring<'a, 'c> {
	/// Opens the file specified by file_path, using a buffer of size the specified block size, using the O_DIRECT flag
	///
	/// Note that the actual block size used may be changed
	pub fn new(file_path: &str, read: bool, write: bool, access_pattern: AccessPattern, block_size: u64, read_size: u64) -> Result<Self, BackendError> {
		let mut file = super::open_with(file_path, read, write, access_pattern, libc::O_DIRECT).map_err(|e| BackendError::IoError(e))?;
		let file_len = file_len(&mut file).map_err(|e| BackendError::IoError(e))?;

		// Need aligned memory of a size a multiple of the alignment for O_DIRECT - round upwards
		let block_size = (block_size as f64 / DEFAULT_ALIGNMENT as f64).ceil() as u64 * DEFAULT_ALIGNMENT as u64;
		// Also need to read in sizes of a multiple of the alignment
		let read_size = (read_size as f64 / DEFAULT_ALIGNMENT as f64).ceil() as u64 * DEFAULT_ALIGNMENT as u64;
		assert_eq!(block_size % DEFAULT_ALIGNMENT as u64, 0);
		let mem_layout = Layout::from_size_align(block_size as usize, DEFAULT_ALIGNMENT).unwrap();
		let buf = unsafe {
			slice::from_raw_parts_mut(
				alloc::alloc(mem_layout),
				block_size as usize
			)
		};

		let ring = rio::new().map_err(|e| BackendError::IoError(e))?;

		let cursor = 0;

		let mut io_uring = IoUring {
			buf,
			write_buffer: Vec::new(),
			mem_layout,
			ring,
			file,
			file_len,
			read_completions: VecDeque::new(),
			write_completions: VecDeque::new(),
			cursor,
			uring_read_size: read_size
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
pub fn req_next<'a, 'c>(uring: &'a mut IoUring<'a, 'c>) where 'a: 'c {
	if uring.cursor >= uring.file_len {
		return;
	}

	// Temporary cursor
	let mut tcursor = uring.cursor;

	// Split the block to be read into chunks of size `uring.uring_read_size` and submit read commands for each chunk
	for c in uring.buf.chunks_mut(uring.uring_read_size as usize) {
		if tcursor >= uring.file_len {
			break;
		} else {
			uring.read_completions.push_back(uring.ring.read_at(&uring.file, unsafe { std::mem::transmute::<&&mut [u8], &&'c mut [u8]>(&c) }, tcursor));
			tcursor += c.len() as u64;
			if tcursor > uring.file_len {
				tcursor = uring.file_len;
			}
		}
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
		let mut bytes_read_total = 0;

		while self.read_completions.len() > 0 {
			let bytes_read = self.read_completions.pop_front().unwrap().wait().map_err(|e| BackendError::IoError(e))?;
			bytes_read_total += bytes_read;
		}

		if bytes_read_total == 0 {
			f(None);
		} else {
			f(Some(&self.buf[0..bytes_read_total]));
			self.cursor += bytes_read_total as u64;
		}

		// Need unsafe transmute cause rust doesn't allow self-referential structs
		req_next(unsafe {
			std::mem::transmute(self)
		});

		Ok(())
	}

	fn write_next(&mut self, data: &[u8]) -> Result<(), BackendError> {
		// TODO: Await all the write completions, then memcpy the data into the write buffer (overwriting contents) and submit write command

		todo!();

		self.write_completions.push_back(self.ring.write_at(&self.file, &data, self.cursor));

		Ok(())
	}
}

impl<'a, 'c> Drop for IoUring<'a, 'c> {
	fn drop(&mut self) {
		// Await the current io operations, lest they use the buffer after it's freed or fail to write after the file is closed
		while self.read_completions.len() != 0 {
			self.read_completions.pop_front().unwrap().wait().map_err(|e| BackendError::IoError(e)).unwrap_or_default();
		}
		while self.write_completions.len() != 0 {
			self.write_completions.pop_front().unwrap().wait().map_err(|e| BackendError::IoError(e)).unwrap_or_default();
		}

		// Deallocate the aligned memory
		unsafe {
			alloc::dealloc(self.buf.as_mut_ptr(), self.mem_layout);
		}
	}
}