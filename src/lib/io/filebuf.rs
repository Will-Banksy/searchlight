use std::{fs::File, io::{self, Seek, Read}, sync::{Arc, Barrier, mpsc::{Receiver, self}}, thread::{self, JoinHandle}};

use super::IoBackend;

const NUM_BLOCKS: usize = 2;

enum PreloaderMsg {
	/// Indicates a block was read from the file, of the length contained in this message
	BlockLoaded(usize),
	/// Reached end of file. No more data was read
	Eof
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
	plt_reciever: Option<Arc<Receiver<PreloaderMsg>>>
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
			plt_reciever: None,
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

	fn next(&mut self) -> Result<&[u8], String> { // BUG: This could potentially cause errors - Rust thinks that the returned slice is 'static but it only lives as long as this struct which can't be represented
		todo!() // TODO

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

		// Make channel
		let (sender, reciever) = mpsc::channel();

		self.plt_reciever = Some(Arc::new(reciever));

		// Start the preloader thread
		self.plt_handle = Some(Box::new(thread::spawn(move || {
			loop {
				// If there are empty slots within which data can be preloaded (i.e. array indicies between preload_block_ref and curr_block_ref)
				// then read the next section of the file into them
				if (preload_block_ref + 1) % NUM_BLOCKS != curr_block_ref {
					let next_preload_block_ref = (preload_block_ref + 1) % NUM_BLOCKS;
					let bytes_read = file.read(block_refs[next_preload_block_ref]).unwrap(); // BUG: unwrap
					if bytes_read == 0 {
						sender.send(PreloaderMsg::Eof).unwrap(); // BUG: unwrap
						break;
					}

					sender.send(PreloaderMsg::BlockLoaded(bytes_read)).unwrap(); // BUG: unwrap

					preload_block_ref = next_preload_block_ref;
				}

				barrier.wait();
				curr_block_ref += 1;
			}
		})));

		Ok(())
		// Spawns a thread that repeatedly reads the next block of the file and then waits
		// self.io_thread = Box::new(Some(thread::spawn(move || {
		// 	let block_0 = unsafe {
		// 		std::slice::from_raw_parts_mut(block_0_addr as *mut u8, block_size as usize)
		// 	};
		// 	let block_1 = unsafe {
		// 		std::slice::from_raw_parts_mut(block_1_addr as *mut u8, block_size as usize)
		// 	};

		// 	// Simply a loop that reads a block into the second block, and waits at the barrier. Skips waiting and returns if error or at EOF
		// 	loop {
		// 		let num_bytes = if curr_block_buffer_iot == 0 {
		// 			file.read(block_0).unwrap() // BUG: unwrap
		// 		} else {
		// 			file.read(block_1).unwrap() // BUG: unwrap
		// 		};

		// 		curr_block_buffer_iot = (curr_block_buffer_iot + 1) % 2;

		// 		if num_bytes == 0 {
		// 			io_sender.send(IoChannelMsg::Eof).unwrap(); // BUG: unwrap
		// 			break;
		// 		}

		// 		io_sender.send(IoChannelMsg::Block(num_bytes as u64)).unwrap(); // BUG: unwrap
		// 		io_barrier_iot.wait();
		// 	}
		// })));
	}
}

impl Drop for IoFileBuf {
	fn drop(&mut self) {
		// Wait for preloader thread to finish // TODO: Ideally need a way to control the preloader thread
		self.plt_handle.take().map(|jh| jh.join().unwrap()); // NOTE: Unsafe unwrap but not sure how else to handle it - make sure plt_thread doesn't panic? But how to handle errors in plt_thread?
	}
}