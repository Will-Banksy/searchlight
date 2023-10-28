use std::{fs::File, io::Read, sync::{Arc, Barrier, mpsc::{Receiver, self, Sender}}, thread::{self, JoinHandle}};

use super::IoBackend;

const NUM_BLOCKS: usize = 2; // NOTE: Changing this will require changing the threading/synchronisation strategy

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
	preload_block_ref: usize,

	plt_handle: Option<Box<JoinHandle<()>>>,
	plt_barrier: Arc<Barrier>,
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
			preload_block_ref: 0,
			plt_handle: None,
			plt_barrier: Arc::new(Barrier::new(2)),
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

	/// If multithreading, then await a message from the preloader thread saying a block is loaded, and then return  t
	fn next(&mut self) -> Result<Option<&[u8]>, String> { // BUG: This could potentially cause errors - Rust thinks that the returned slice is 'static but it only lives as long as this struct which can't be represented
		let multithreading = self.plt_handle.is_some();

		if multithreading {
			if let Some(plt_reciever) = &self.plt_receiver {
				let msg = plt_reciever.recv().map_err(|e| e.to_string())?;
				match msg {
					FromPreloaderMsg::BlockLoaded(num_bytes) => {
						let curr_slice = &self.block_refs[self.curr_block_ref][0..num_bytes];

						self.curr_block_ref += 1;

						// Inform the preloader thread that a block has been read
						// BUG: By the time the slice is returned, it may be (and in fact probably will be) being overwritten
						// since we are letting the preloader thread continue before the slice has been finished with
						// TODO: To fix, see the todo in io::IoBackend to do with modifying this function to take a function
						if let Some(plt_sender) = &self.plt_sender {
							plt_sender.send(ToPreloaderMsg::ReadBlock).unwrap(); // BUG: unwrap
						}

						Ok(Some(curr_slice))
					},
					FromPreloaderMsg::Eof => {
						Ok(None)
					}
				}
			} else {
				panic!("[ERROR]: Invalid state")
			}
		} else {
			todo!()
		}

		// Most of this line is transforming each variant of the Result from file.read
		// self.file.read(block).map(|n| n as u64).map_err(|e| e.to_string())
	}

	fn start_preload_thread(&mut self) -> Result<(), String> {
		// Copy a load of stuff to be sent to the preloader thread
		let mut preload_block_ref = self.preload_block_ref;
		let mut curr_block_ref = self.curr_block_ref;
		let mut block_refs: Vec<&'static mut [u8]> = {
			self.block_refs.iter_mut().map(|r| unsafe { &mut *(*r as *mut [u8]) }).collect()
		};
		let mut file = self.file.take().ok_or("[ERROR] Preload thread already started")?;
		let barrier = Arc::clone(&self.plt_barrier);

		// Make channels
		let (frmplt_sender, frmplt_receiver) = mpsc::channel();
		let (toplt_sender, toplt_receiver) = mpsc::channel();

		self.plt_receiver = Some(Arc::new(frmplt_receiver));
		self.plt_sender = Some(Arc::new(toplt_sender));

		// Start the preloader thread
		self.plt_handle = Some(Box::new(thread::spawn(move || {
			loop {
				// If there are empty slots within which data can be preloaded (i.e. array indicies between preload_block_ref and curr_block_ref)
				// then read the next section of the file into them
				if (preload_block_ref + 1) % NUM_BLOCKS != curr_block_ref {
					let next_preload_block_ref = (preload_block_ref + 1) % NUM_BLOCKS;
					let bytes_read = file.read(block_refs[next_preload_block_ref]).unwrap(); // BUG: unwrap
					if bytes_read == 0 {
						frmplt_sender.send(FromPreloaderMsg::Eof).unwrap(); // BUG: unwrap
						break;
					}

					frmplt_sender.send(FromPreloaderMsg::BlockLoaded(bytes_read)).unwrap(); // BUG: unwrap

					preload_block_ref = next_preload_block_ref;
				}

				let msg = toplt_receiver.recv().unwrap(); // BUG: unwrap
				match msg {
					ToPreloaderMsg::ReadBlock => {
						curr_block_ref += 1;
					},
					ToPreloaderMsg::Terminate => {
						break;
					}
				}
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
	}
}