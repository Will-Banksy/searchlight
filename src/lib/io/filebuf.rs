use std::{fs::{File, OpenOptions}, io::Read, sync::{Arc, mpsc::{Receiver, self, Sender}}, thread::{self, JoinHandle}, os::{fd::AsRawFd, unix::prelude::OpenOptionsExt}, alloc::Layout, slice, alloc};

use crate::lib::io::DEFAULT_ALIGNMENT;

use super::{IoBackend, file_len, BackendInfo};

const NUM_BLOCKS: usize = 3; // Controls how many blocks are loaded at once

/// Messages sent from the preloader thread
enum FromPreloaderMsg {
	/// Indicates a block was read from the file, of the length contained in this message
	BlockLoaded(usize),
	/// Reached end of file. No more data was read
	Eof
}

/// Messages sent to the preloader thread
enum ToPreloaderMsg {
	ReadBlock,
	Terminate,
}

pub struct IoFileBuf<'a> {
	file: Option<File>,
	file_len: u64,
	buf: &'a mut [u8],
	mem_layout: Layout,
	block_refs: [&'a mut [u8]; NUM_BLOCKS],
	curr_block_ref: usize,

	plt_handle: Option<Box<JoinHandle<()>>>,
	plt_receiver: Option<Arc<Receiver<FromPreloaderMsg>>>,
	plt_sender: Option<Arc<Sender<ToPreloaderMsg>>>
}

impl<'a> IoFileBuf<'a> {
	/// Returns an instance of self, having opened the file, or returns an error if one occurred
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
		// Allocate 3 times the rounded block size
		let block_size = (block_size as f64 / DEFAULT_ALIGNMENT as f64).ceil() as u64 * DEFAULT_ALIGNMENT as u64;
		assert_eq!(block_size % DEFAULT_ALIGNMENT as u64, 0);
		let buf_size = (block_size as usize) * NUM_BLOCKS;
		let mem_layout = Layout::from_size_align(buf_size, DEFAULT_ALIGNMENT).map_err(|e| e.to_string())?;
		let buf = unsafe {
			slice::from_raw_parts_mut(
				alloc::alloc(mem_layout),
				buf_size
			)
		};

		// Get mutable references to the allocated buffer's blocks/chunks and collect them into an array
		let block_refs = unsafe {
			(
				slice::from_raw_parts_mut(buf as *mut [u8] as *mut u8, buf.len())
			).chunks_exact_mut(block_size as usize).collect::<Vec<&mut [u8]>>().try_into().unwrap() // Should never error
		};

		let mut fb = IoFileBuf {
			file: Some(file),
			file_len,
			buf,
			mem_layout,
			block_refs,
			curr_block_ref: 0,
			plt_handle: None,
			plt_receiver: None,
			plt_sender: None
		};

		fb.start_preload_thread()?;

		Ok(fb)
	}

	fn start_preload_thread(&mut self) -> Result<(), String> {
		// Copy a load of stuff to be sent to the preloader thread
		let mut block_refs: Vec<&'static mut [u8]> = {
			self.block_refs.iter_mut().map(|r| unsafe { &mut *(*r as *mut [u8]) }).collect()
		};
		let mut file = self.file.take().ok_or("[ERROR] Preload thread already started")?;

		// preload_block_ref is the block that will be written to by the preloader thread - We want that to be (initially) the current block
		let mut curr_block_ref = (self.curr_block_ref + NUM_BLOCKS - 1) % NUM_BLOCKS;
		let mut preload_block_ref = self.curr_block_ref;

		// Make channels
		let (frmplt_sender, frmplt_receiver) = mpsc::channel();
		let (toplt_sender, toplt_receiver) = mpsc::channel();

		self.plt_receiver = Some(Arc::new(frmplt_receiver));
		self.plt_sender = Some(Arc::new(toplt_sender));

		// Start the preloader thread
		self.plt_handle = Some(Box::new(thread::spawn(move || {
			let mut eof = false;

			loop {
				// If there are empty slots within which data can be preloaded (i.e. array indicies between preload_block_ref and curr_block_ref)
				// then read the next section of the file into them
				if preload_block_ref != curr_block_ref && !eof {
					let bytes_read = file.read(block_refs[preload_block_ref]).unwrap(); // BUG: unwrap

					if bytes_read == 0 {
						frmplt_sender.send(FromPreloaderMsg::Eof).unwrap(); // BUG: unwrap

						eof = true;
					} else {
						frmplt_sender.send(FromPreloaderMsg::BlockLoaded(bytes_read)).unwrap(); // BUG: unwrap
					}

					preload_block_ref = (preload_block_ref + 1) % NUM_BLOCKS;
				}

				let msg = toplt_receiver.recv().unwrap(); // BUG: unwrap
				match msg {
					ToPreloaderMsg::ReadBlock => {
						curr_block_ref = (curr_block_ref + 1) % NUM_BLOCKS;
					},
					ToPreloaderMsg::Terminate => {
						break;
					}
				}
			}

			// NOTE: Left in for benchmarking
			#[cfg(unix)]
			unsafe {
				libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
			}
		})));

		Ok(())
	}
}

impl IoBackend for IoFileBuf<'_> {
	fn file_info(&self) -> BackendInfo {
		BackendInfo {
			file_len: self.file_len,
			block_size: self.block_refs[0].len() as u64
		}
	}

	/// If multithreading, then await a message from the preloader thread saying a block is loaded,
	/// and then call `f` with the loaded block (passing None if the end of the file is reached),
	/// and then inform the preloader thread it can overwrite that block.
	///
	/// If singlethreading, then read the next block of the file and call `f` with that or None.
	///
	/// An error will be returned if one occurs. Note that an error can still be returned even if
	/// `f` was called successfully with the next block or None.
	fn next<'b>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'b>) -> Result<(), String> {
		if let Some(plt_reciever) = &self.plt_receiver {
			let msg = plt_reciever.recv().map_err(|e| e.to_string());
			match msg {
				Ok(FromPreloaderMsg::BlockLoaded(num_bytes)) => {
					// Get reference to the current slice that is being modified
					let curr_slice = &self.block_refs[self.curr_block_ref][0..num_bytes];

					self.curr_block_ref = (self.curr_block_ref + 1) % NUM_BLOCKS;

					// Let the caller process the slice
					f(Some(curr_slice));

					// Inform the preloader thread that a block has been read
					if let Some(plt_sender) = &self.plt_sender {
						if let Err(e) = plt_sender.send(ToPreloaderMsg::ReadBlock) {
							Err(format!("[ERROR]: Failed to send a message to the preloader thread: {}", e.to_string()))
						} else {
							Ok(())
						}
					} else {
						panic!("[ERROR]: Invalid state")
					}
				},
				Ok(FromPreloaderMsg::Eof) => {
					Ok(f(None))
				},
				Err(e) => {
					Err(format!("[ERROR]: Failed to receive message from preloader thread: {}", e.to_string()))
				}
			}
		} else {
			panic!("[ERROR]: Invalid state")
		}
	}
}

impl Drop for IoFileBuf<'_> {
	fn drop(&mut self) {
		// Ask the preloader thread to terminate
		if let Some(plt_sender) = &self.plt_sender {
			plt_sender.send(ToPreloaderMsg::Terminate).unwrap() // BUG: unwrap
		}

		// Wait for preloader thread to finish
		self.plt_handle.take().map(|jh| jh.join().unwrap()); // BUG: unwrap

		// Deallocate the aligned memory
		unsafe {
			alloc::dealloc(self.buf.as_mut_ptr(), self.mem_layout);
		}

		// NOTE: Left in for benchmarking
		#[cfg(unix)]
		unsafe {
			if let Some(file) = &self.file {
				libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
			}
		}
	}
}