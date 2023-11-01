use std::{fs::File, io::Read, sync::{Arc, mpsc::{Receiver, self, Sender}}, thread::{self, JoinHandle}, os::fd::AsRawFd};

use super::IoBackend;

const NUM_BLOCKS: usize = 3; // Controls how many blocks are loaded at once

enum FromPreloaderMsg {
	/// Indicates a block was read from the file, of the length contained in this message
	BlockLoaded(usize),
	/// Reached end of file. No more data was read
	Eof
}

enum ToPreloaderMsg {
	ReadBlock,
	Terminate,
}

pub struct IoFileBuf {
	file: Option<File>,
	file_len: u64,
	_buffer: Arc<Vec<u8>>,
	block_refs: [&'static mut [u8]; NUM_BLOCKS],
	curr_block_ref: usize,

	plt_handle: Option<Box<JoinHandle<()>>>,
	plt_receiver: Option<Arc<Receiver<FromPreloaderMsg>>>,
	plt_sender: Option<Arc<Sender<ToPreloaderMsg>>>
}

impl IoFileBuf {
	/// Returns an instance of self, having opened the file, or returns an error if one occurred
	pub fn new(file: File, file_len: u64, block_size: u64) -> Result<Self, String> {
		// Allocate the backing buffer
		let buffer = Arc::new(vec![0; block_size as usize * NUM_BLOCKS]);

		// Cast a reference to the buffer to a pointer to it, get mutable references to it's chunks and collect them
		let buffer_raw = Arc::as_ptr(&buffer) as *mut Vec<u8>;
		let block_refs = unsafe {
			(&mut *buffer_raw).chunks_exact_mut(block_size as usize).collect::<Vec<&mut [u8]>>().try_into().unwrap() // Should never error
		};

		Ok(IoFileBuf {
			file: Some(file),
			file_len,
			_buffer: buffer,
			block_refs,
			curr_block_ref: 0,
			plt_handle: None,
			plt_receiver: None,
			plt_sender: None
		})
	}
}

impl IoBackend for IoFileBuf {
	fn file_info(&self) -> u64 {
		self.file_len
	}

	// fn preload_next(&mut self) -> Result<(), String> {
	// 	// if (self.preload_block_ref + 1) % NUM_BLOCKS == self.curr_block_ref {
	// 	// 	Ok(())
	// 	// } else {
	// 	// 	let preload_block_ref = (self.preload_block_ref + 1) % NUM_BLOCKS;
	// 	// 	let bytes_read = self.file.read(self.block_refs[preload_block_ref]).map_err(|e| e.to_string())?;
	// 	// 	Ok(())
	// 	// }
	// 	todo!()
	// }

	/// If multithreading, then await a message from the preloader thread saying a block is loaded,
	/// and then call `f` with the loaded block (passing None if the end of the file is reached),
	/// and then inform the preloader thread it can overwrite that block.
	///
	/// If singlethreading, then read the next block of the file and call `f` with that or None.
	///
	/// An error will be returned if one occurs. Note that an error can still be returned even if
	/// `f` was called successfully with the next block or None.
	fn next<'a>(&mut self, f: Box<dyn FnOnce(Option<&[u8]>) + 'a>) -> Result<(), String> {
		let multithreading = self.plt_handle.is_some();

		if multithreading {
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
		} else {
			todo!() // TODO: Single threading implementation... Necessary?
		}
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

impl Drop for IoFileBuf {
	fn drop(&mut self) {
		// Ask the preloader thread to terminate
		if let Some(plt_sender) = &self.plt_sender {
			plt_sender.send(ToPreloaderMsg::Terminate).unwrap() // BUG: unwrap
		}

		// Wait for preloader thread to finish
		self.plt_handle.take().map(|jh| jh.join().unwrap()); // BUG: unwrap

		// NOTE: Left in for benchmarking
		#[cfg(unix)]
		unsafe {
			if let Some(file) = &self.file {
				libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
			}
		}
	}
}