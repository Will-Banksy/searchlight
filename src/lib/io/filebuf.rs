use std::{fs::File, io::Read, sync::{Arc, mpsc::{Receiver, self, Sender}}, thread::{self, JoinHandle}, alloc::Layout, slice, alloc};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;

use crate::lib::io::DEFAULT_ALIGNMENT;

use super::{SeqIoBackend, file_len, BackendInfo, IoBackend, BackendError, AccessPattern};

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
	cursor: u64,

	plt_handle: Option<Box<JoinHandle<()>>>,
	plt_receiver: Option<Arc<Receiver<FromPreloaderMsg>>>,
	plt_sender: Option<Arc<Sender<ToPreloaderMsg>>>
}

impl<'a> IoFileBuf<'a> {
	/// Returns an instance of self, having opened the file, or returns an error if one occurred
	pub fn new(file_path: &str, read: bool, write: bool, access_pattern: AccessPattern, block_size: u64) -> Result<Self, BackendError> {
		let custom_flags = {
			#[cfg(target_os = "linux")]
			{ libc::O_DIRECT }
			#[cfg(not(target_os = "linux"))]
			{ 0 }
		};

		let mut file = super::open_with(file_path, read, write, access_pattern, custom_flags).map_err(|e| BackendError::IoError(e))?;
		let file_len = file_len(&mut file).map_err(|e| BackendError::IoError(e))?;

		// Need aligned memory of a size a multiple of the alignment for O_DIRECT - round upwards
		// Allocate 3 times the rounded block size
		let block_size = (block_size as f64 / DEFAULT_ALIGNMENT as f64).ceil() as u64 * DEFAULT_ALIGNMENT as u64;
		assert_eq!(block_size % DEFAULT_ALIGNMENT as u64, 0);
		let buf_size = (block_size as usize) * NUM_BLOCKS;
		let mem_layout = Layout::from_size_align(buf_size, DEFAULT_ALIGNMENT).unwrap(); // Could naturally occur but in the instance that it does... I think panicking is an appropriate response
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
			cursor: 0,
			plt_handle: None,
			plt_receiver: None,
			plt_sender: None
		};

		fb.start_preload_thread()?;

		Ok(fb)
	}

	fn start_preload_thread(&mut self) -> Result<(), BackendError> {
		// Copy a load of stuff to be sent to the preloader thread
		let mut block_refs: Vec<&'static mut [u8]> = {
			self.block_refs.iter_mut().map(|r| unsafe { &mut *(*r as *mut [u8]) }).collect()
		};
		let mut file = self.file.take().unwrap(); // Panic if no file cause if no file that indicates a logic error

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
	fn backend_info(&self) -> BackendInfo {
		BackendInfo {
			file_len: self.file_len,
			block_size: self.block_refs[0].len() as u64,
			cursor: self.cursor,
		}
	}
}

impl SeqIoBackend for IoFileBuf<'_> {
	/// If multithreading, then await a message from the preloader thread saying a block is loaded,
	/// and then call `f` with the loaded block (passing None if the end of the file is reached),
	/// and then inform the preloader thread it can overwrite that block.
	///
	/// If singlethreading, then read the next block of the file and call `f` with that or None.
	///
	/// An error will be returned if one occurs. Note that an error can still be returned even if
	/// `f` was called successfully with the next block or None.
	fn read_next<'b>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'b>) -> Result<(), BackendError> {
		if let Some(plt_reciever) = &self.plt_receiver {
			let msg = plt_reciever.recv().map_err(|e| e.to_string());
			match msg {
				Ok(FromPreloaderMsg::BlockLoaded(num_bytes)) => {
					// Get reference to the current slice that is being modified
					let curr_slice = &self.block_refs[self.curr_block_ref][0..num_bytes];

					self.curr_block_ref = (self.curr_block_ref + 1) % NUM_BLOCKS;

					self.cursor += num_bytes as u64;

					// Let the caller process the slice
					f(Some(curr_slice));

					// Inform the preloader thread that a block has been read
					if let Some(plt_sender) = &self.plt_sender {
						if let Err(e) = plt_sender.send(ToPreloaderMsg::ReadBlock) {
							Err(BackendError::ThreadSendRecvError(format!("Failed to send a message to the preloader thread: {}", e.to_string())))
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
					Err(BackendError::ThreadSendRecvError(format!("Failed to receive message from preloader thread: {}", e.to_string())))
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