// NOTE: This will be removed in the future, and test images will be generated with Woodblock instead (https://github.com/fkie-cad/woodblock)

use std::{env::args, fs::{self, OpenOptions}, io::{Seek, SeekFrom, Write}};

use tinyrand::{Rand, StdRand};

fn main() {
	let input_dir = match args().nth(1) {
		Some(arg) => arg,
		None => panic!("Path to input directory should be supplied as first argument")
	};

	let output_path = match args().nth(2) {
		Some(arg) => arg,
		None => panic!("Path to output file should be supplied as second argument")
	};

	let mut output_file = OpenOptions::new().write(true).create(true).open(&output_path).unwrap();

	let mut rand_data: Vec<u8> = vec![0; 1024];

	let mut rng = StdRand::default();

	for dir_entry in fs::read_dir(input_dir).unwrap() {
		// Fill an amount of the output file with random data
		rand_data.chunks_exact_mut(4).for_each(|b| unsafe { *(b.as_mut_ptr() as *mut u32) = rng.next_u32() });
		let amt_rand = rng.next_lim_u32(1023);
		let written = output_file.write(&rand_data[0..amt_rand as usize]).unwrap();
		output_file.seek(SeekFrom::Current(written as i64)).unwrap();

		// Read the current directory entry (if it is a file) and appends that data to the output file
		// TODO: Introduce fragmentation, partially erase files, etc. for some more variation
		//       Also make sure the entire file is erased before writing to it - currently we're just
		//       overwriting the previous data, but if the previous data was longer than the current data,
		//       it will remain at the end of the file
		let dir_entry = dir_entry.unwrap();
		if dir_entry.metadata().unwrap().is_file() {
			let curr_fidx = output_file.seek(SeekFrom::Current(0)).unwrap();
			let file_data = fs::read(dir_entry.path()).unwrap();
			println!("Copying {} ({} bytes) into output file at idx {}", dir_entry.path().display(), file_data.len(), curr_fidx);
			let written = output_file.write(&file_data).unwrap();
			output_file.seek(SeekFrom::Current(written as i64)).unwrap();
		}
	}
}