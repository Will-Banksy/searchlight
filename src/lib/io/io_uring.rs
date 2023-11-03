use std::{fs::{OpenOptions, File}, alloc::{self, Layout}, slice, os::unix::prelude::OpenOptionsExt};

use rio::{Rio, Completion};

use super::{IoBackend, file_len};

const DEFAULT_ALIGNMENT: usize = 4096;

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
	pub fn new(file_path: &str, block_size: u64) -> Result<Self, String> {
		let mut file = OpenOptions::new().custom_flags(libc::O_DIRECT).read(true).open(file_path).map_err(|e| e.to_string())?;
		let file_len = file_len(&mut file)?;

		// Need aligned memory for io_uring
		let mem_layout = Layout::from_size_align(block_size as usize, DEFAULT_ALIGNMENT).map_err(|e| e.to_string())?;
		let buf = unsafe {
			slice::from_raw_parts_mut(
				alloc::alloc(mem_layout),
				block_size as usize
			)
		};

		let ring = rio::new().map_err(|e| e.to_string())?;

		let cursor = 0;

		Ok(IoUring {
			buf,
			mem_layout,
			ring,
			file,
			file_len,
			prev_completion: None,
			cursor
		})
	}
}

pub fn req_next<'a, 'c>(uring: &'a mut IoUring<'a, 'c>) where 'a: 'c {
	uring.prev_completion = Some(uring.ring.read_at(&uring.file, &uring.buf, uring.cursor));
}

impl<'a, 'c> IoBackend for IoUring<'a, 'c> where 'a: 'c {
	fn file_info(&self) -> u64 {
		self.file_len
	}

	fn next<'b>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'b>) -> Result<(), String> {
		// FIXME: Figure out lifetime shit for storing completions
		// req_next::<'a, 'c>(self);

		todo!()
	}
}

impl<'a, 'c> Drop for IoUring<'a, 'c> {
	fn drop(&mut self) {
		unsafe {
			alloc::dealloc(self.buf.as_mut_ptr(), self.mem_layout);
		}
	}
}