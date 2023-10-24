use std::{sync::{Arc, Barrier, mpsc::{self, Receiver}}, io::{self, Read, Seek}, fs::File, thread::{JoinHandle, self}};

const DEFAULT_BLOCK_SIZE: u64 = 1 * 1024 * 1024 * 1024; // 1 GiB

// TODO: Test adding more blocks loaded at once - although theoretically, I'm not sure I see how that'd help
// TODO: Test how long the main thread waits on the io_thread

/// The messages sent by the io_thread in the IoManager
enum IoChannelMsg {
	/// Indicates a block was read from the file, of the length contained in this message
	Block(u64),
	/// Reached end of file. No more data was read
	Eof
}

pub struct IoManager {
	block_size: u64,
	_buffer: Arc<Vec<u8>>, // Owns the block data, keeps it alive until IoManager is dropped
	blocks: [&'static mut [u8]; 2],
	/// The index of the current block within the file
	curr_block_idx: u64,
	/// Either 0 or 1, the index into self.blocks that is the current block (basically imagine self.blocks as a ring buffer then this is the index of the first element)
	curr_block_buffer: u8,
	curr_block_bytes_read: u64,
	file: Option<File>,
	file_len: Option<u64>,
	io_thread: Box<Option<JoinHandle<()>>>,
	io_thread_barrier: Arc<Barrier>,
	io_thread_chreciever: Option<Receiver<IoChannelMsg>>,
}

impl IoManager {
	pub fn new(block_size: Option<u64>) -> Self {
		// Allocate buffer 3 times the block size, then split it up into 3 chunks of size block size (requires unsafe code for dereferencing the raw ptr)
		let block_size = block_size.unwrap_or(DEFAULT_BLOCK_SIZE);
		let buffer = Arc::new(vec![0; (block_size * 2) as usize]);
		let buffer_raw = Arc::as_ptr(&buffer) as *mut Vec<u8>;
		let blocks = unsafe {
			(&mut *buffer_raw).chunks_exact_mut(block_size as usize).collect::<Vec<&mut [u8]>>().try_into().unwrap() // Should never error
		};

		IoManager {
			block_size,
			_buffer: buffer,
			blocks,
			curr_block_idx: 0,
			curr_block_buffer: 0,
			curr_block_bytes_read: 0,
			file: None,
			file_len: None,
			io_thread: Box::new(None),
			io_thread_barrier: Arc::new(Barrier::new(2)),
			io_thread_chreciever: None,
		}
	}

	pub fn open(&mut self, path: &str) -> io::Result<()> {
		let mut file = File::open(path)?;

		// Get the length of the file, by querying metadata and as a last resort seeking to the end of the file and getting the offset
		let file_len = {
			if let Ok(metadata) = file.metadata() {
				metadata.len()
			} else {
				let size = file.seek(io::SeekFrom::End(0))?;
				file.seek(io::SeekFrom::Start(0))?;
				size
			}
		};

		self.file = Some(file);
		self.file_len = Some(file_len);

		Ok(())
	}

	/// Starts a thread `io_thread` that repeatedly reads the next chunk of the opened file
	pub fn start(&mut self) -> Result<(), String> {
		// If file is open, claim it (take it out of the option)
		if let Some(mut file) = self.file.take() {
			// Cast the reference to a "thin pointer" to a usize
			let block_0_addr = self.blocks[0] as *mut [u8] as *mut () as usize;
			let block_1_addr = self.blocks[1] as *mut [u8] as *mut () as usize;

			let block_size = self.block_size;

			// We want to read into the unoccupied buffer (that curr_block_buffer is not currently pointing at)
			let mut curr_block_buffer_iot = (self.curr_block_buffer + 1) % 2;

			// Synchronisation stuff
			let io_barrier_iot = Arc::clone(&self.io_thread_barrier);
			let (io_sender, io_reciever) = mpsc::channel::<IoChannelMsg>();
			self.io_thread_chreciever = Some(io_reciever);

			// Spawns a thread that repeatedly reads the next block of the file and then waits
			self.io_thread = Box::new(Some(thread::spawn(move || {
				let block_0 = unsafe {
					std::slice::from_raw_parts_mut(block_0_addr as *mut u8, block_size as usize)
				};
				let block_1 = unsafe {
					std::slice::from_raw_parts_mut(block_1_addr as *mut u8, block_size as usize)
				};

				// Simply a loop that reads a block into the second block, and waits at the barrier. Skips waiting and returns if error or at EOF
				loop {
					let num_bytes = if curr_block_buffer_iot == 0 {
						file.read(block_0).unwrap() // BUG: unwrap
					} else {
						file.read(block_1).unwrap() // BUG: unwrap
					};

					curr_block_buffer_iot = (curr_block_buffer_iot + 1) % 2;

					if num_bytes == 0 {
						io_sender.send(IoChannelMsg::Eof).unwrap(); // BUG: unwrap
						break;
					}

					io_sender.send(IoChannelMsg::Block(num_bytes as u64)).unwrap(); // BUG: unwrap
					io_barrier_iot.wait();
				}
			})));

			Ok(())
		} else {
			Err("File not open".into())
		}
	}

	/// Waits for the io_thread to finish reading the next block into a secondary buffer, and copies it to the primary buffer,
	/// allowing the io_thread to continue reading the next block
	///
	/// Returns an error if there was an error communicating with the io_thread, otherwise a bool that is true if EOF is reached,
	/// false otherwise
	///
	/// Also returns an error if this method is called before `start`
	pub fn load_next_block(&mut self) -> Result<bool, String> {
		let io_reciever = (self.io_thread_chreciever.as_mut()).ok_or("IoManager not started yet")?;

		// Await a message
		let msg = io_reciever.recv().map_err(|e| format!("Error recieving message from io_thread: {}", e.to_string()))?;

		match msg {
			IoChannelMsg::Block(bytes_read) => {
				self.curr_block_bytes_read = bytes_read;
				self.curr_block_buffer = (self.curr_block_buffer + 1) % 2;

				// Pattern matching on arrays allows multiple mutable references to different array indices
				// let [ref mut block_0, ref mut block_1] = self.blocks;

				// memcpy bytes_read bytes from block1 to block0
				// (&mut (block_0)[0..bytes_read as usize]).copy_from_slice(&(*block_1)[0..bytes_read as usize]);

				self.curr_block_idx += 1; // NOTE: If the functionality of this struct changes this might become incorrect

				// Let the thread continue
				self.io_thread_barrier.wait();

				Ok(false)
			},
			IoChannelMsg::Eof => Ok(true)
		}
	}

	/// Calls a function with a reference to the current block as an argument
	///
	/// If this method is called before `load_next_block`, the contents of the current block will be zeroed
	pub fn with_current_block<F>(&self, mut f: F) where F: FnMut(&[u8]) {
		f(&(self.blocks[self.curr_block_buffer as usize])[0..self.curr_block_bytes_read as usize])
	}

	/// Returns the progress through the file as a number between 0.0 and 1.0.
	/// Specifically, returns the last loaded address divided by the file length
	pub fn progress(&self) -> f32 {
		if let Some(file_len) = self.file_len {
			(((self.curr_block_idx - 1) * self.block_size + self.curr_block_bytes_read) as f32) / (file_len as f32)
		} else {
			0.0
		}
	}

	/// Returns the length of the opened file in bytes, or none if a file hasn't been opened
	pub fn file_len(&self) -> Option<u64> {
		self.file_len
	}
}

impl Drop for IoManager {
	fn drop(&mut self) {
		// Wait for io thread to finish
		self.io_thread.take().map(|jh| jh.join().unwrap()); // NOTE: Unsafe unwrap but not sure how else to handle it - make sure io_thread doesn't panic? But how to handle errors in io_thread?
	}
}

#[test]
#[cfg(test)]
fn test_io_manager() {
	let file_path = "Cargo.toml";

	let mut ioman = IoManager::new(Some(10));

	ioman.open(file_path).expect("Failed to open Cargo.toml");

	ioman.start().expect("Failed to start IoManager");

	let mut sb = String::new();

	loop {
		if let Ok(eof) = ioman.load_next_block() {
			if eof {
				break;
			}
		}

		ioman.with_current_block(|block| {
			sb.push_str(std::str::from_utf8(block).unwrap());
		});
	}

	assert_eq!(sb, include_str!("../../Cargo.toml"))
}