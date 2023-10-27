mod mmap;
mod filebuf;

use std::{sync::{Arc, Barrier, mpsc::{self, Receiver}}, io::{self, Read, Seek}, fs::File, thread::{JoinHandle, self}};

const DEFAULT_BLOCK_SIZE: u64 = 8192; // Got from BUFSIZ in stdio.h // 1 * 1024 * 1024 * 1024; // 1 GiB

// TODO: Test adding more blocks loaded at once - although theoretically, I'm not sure I see how that'd help
// TODO: Test how long the main thread waits on the io_thread
// https://stackoverflow.com/a/39196499/11009247

trait IoBackend {
	/// Returns information about the opened file - Currently just the length of it
	fn file_info(&self) -> u64;
	/// Read the next block of file data, returning the data as a slice, or returning an error if one occurred
	fn next(&mut self) -> Result<&[u8], String>;
	/// Optionally, this method should start a thread for preloading
	fn start_preload_thread(&mut self) -> Result<(), String> {
		Ok(())
	}
}

pub struct IoManager {
	block_size: u64,
	file_len: Option<u64>,
	io_backend: Option<Box<dyn IoBackend>>
}

impl IoManager {
	pub fn new() -> Self {
		Self::new_with(DEFAULT_BLOCK_SIZE)
	}

	pub fn new_with(block_size: u64) -> Self {
		IoManager { block_size, file_len: None, io_backend: None }
	}

	pub fn open(&mut self, path: &str) -> Result<(), String> {
		let mut file = File::open(path).map_err(|e| e.to_string())?;

		// Get the length of the file, by querying metadata and as a last resort seeking to the end of the file and getting the offset
		let file_len = {
			if let Ok(metadata) = file.metadata() {
				metadata.len()
			} else {
				let size = file.seek(io::SeekFrom::End(0)).map_err(|e| e.to_string())?;
				file.seek(io::SeekFrom::Start(0)).map_err(|e| e.to_string())?;
				size
			}
		};

		// If the file size is more than 16KiB, use the memory mapped IoBackend
		// Otherwise, use the filebuf IoBackend
		// NOTE: Since it's only 16KiB... is it worth agonising over getting the filebuf one perfect?
		self.io_backend = {
			Some(if file_len > (16 * 1024) { // https://stackoverflow.com/a/39196499/11009247
				mmap::IoMmap::new(file, file_len, self.block_size).map(|io_mmap| Box::new(io_mmap))?
			} else {
				filebuf::IoFileBuf::new(file, file_len, self.block_size).map(|io_filebuf| Box::new(io_filebuf))?
			})
		};

		// Just start the preload thread immediately
		if let Some(ref mut io_backend) = self.io_backend {
			io_backend.start_preload_thread().unwrap_or(eprintln!("[WARN]: Preloading thread failed to start")); // Just ignoring errors for starting the preload thread
		}

		self.file_len = Some(file_len);

		Ok(())
	}

	/// Waits for the io_thread to finish reading the next block into a secondary buffer, and copies it to the primary buffer,
	/// allowing the io_thread to continue reading the next block
	///
	/// Returns an error if there was an error communicating with the io_thread, otherwise a bool that is true if EOF is reached,
	/// false otherwise
	///
	/// Also returns an error if this method is called before `start`
	pub fn load_next_block(&mut self) -> Result<bool, String> {
		todo!() // TODO
		// let io_reciever = (self.io_thread_chreciever.as_mut()).ok_or("IoManager not started yet")?;

		// // Await a message
		// let msg = io_reciever.recv().map_err(|e| format!("Error recieving message from io_thread: {}", e.to_string()))?;

		// match msg {
		// 	IoChannelMsg::Block(bytes_read) => {
		// 		self.curr_block_bytes_read = bytes_read;
		// 		self.curr_block_buffer = (self.curr_block_buffer + 1) % 2;

		// 		// Pattern matching on arrays allows multiple mutable references to different array indices
		// 		// let [ref mut block_0, ref mut block_1] = self.blocks;

		// 		// memcpy bytes_read bytes from block1 to block0
		// 		// (&mut (block_0)[0..bytes_read as usize]).copy_from_slice(&(*block_1)[0..bytes_read as usize]);

		// 		self.curr_block_idx += 1; // NOTE: If the functionality of this struct changes this might become incorrect

		// 		// Let the thread continue
		// 		self.io_thread_barrier.wait();

		// 		Ok(false)
		// 	},
		// 	IoChannelMsg::Eof => Ok(true)
		// }
	}

	/// Calls a function with a reference to the current block as an argument
	///
	/// If this method is called before `load_next_block`, the contents of the current block will be zeroed
	pub fn with_current_block<F>(&self, mut f: F) where F: FnMut(&[u8]) {
		todo!() // TODO
		// f(&(self.blocks[self.curr_block_buffer as usize])[0..self.curr_block_bytes_read as usize])
	}

	/// Returns the progress through the file as a number between 0.0 and 1.0.
	/// Specifically, returns the last loaded address divided by the file length
	pub fn progress(&self) -> f32 {
		todo!() // TODO
		// if let Some(file_len) = self.file_len {
		// 	(((self.curr_block_idx - 1) * self.block_size + self.curr_block_bytes_read) as f32) / (file_len as f32)
		// } else {
		// 	0.0
		// }
	}

	/// Returns the length of the opened file in bytes, or none if a file hasn't been opened
	pub fn file_len(&self) -> Option<u64> {
		self.file_len
	}
}

#[test]
#[cfg(test)]
fn test_io_manager() {
	let file_path = "Cargo.toml";

	let mut ioman = IoManager::new_with(10);

	ioman.open(file_path).expect("Failed to open Cargo.toml");

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