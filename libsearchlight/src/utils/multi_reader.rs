use std::io::{self, IoSlice, Read, Write};

pub struct MultiReader<'a> {
	data: &'a [&'a [u8]],
	slice_idx: usize,
	local_idx: usize
}

impl<'a> MultiReader<'a> {
	pub fn new(data: &'a [&'a [u8]]) -> Self {
		MultiReader {
			data,
			slice_idx: 0,
			local_idx: 0
		}
	}
}

impl<'a> Read for MultiReader<'a> {
	fn read(&mut self, mut buf: &mut [u8]) -> io::Result<usize> {
		if self.slice_idx == self.data.len() {
			return Ok(0); // EOF
		}

		let mut sources = Vec::new();

		// Ugly code but seems to work
		let mut bytes_to_consume = buf.len();
		while bytes_to_consume != 0 {
			let this_slice_len = self.data[self.slice_idx].len();
			let end = (self.local_idx + bytes_to_consume).min(this_slice_len);
			sources.push(IoSlice::new(&self.data[self.slice_idx][self.local_idx..end]));
			let consumed = end - self.local_idx;
			bytes_to_consume -= consumed;

			self.local_idx += consumed;
			if end == this_slice_len {
				self.slice_idx += 1;
				self.local_idx = 0;
				if self.slice_idx == self.data.len() {
					break;
				}
			}
		}

		buf.write_vectored(&sources)
	}
}

#[cfg(test)]
mod test {
    use std::io::Read;

    use crate::utils::multi_reader::MultiReader;

	#[test]
	fn test_multi_reader() {
		let test_data: &[&[u8]] = &[
			&[ 0, 1, 2, 3, 4, 5 ],
			&[ 6, 7 ],
			&[],
			&[ 8, 9, 10, 11, 12 ],
			&[ 13 ]
		];

		let test_reads = &[
			2,
			5,
			4
		];

		let expected = &[
			0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10
		];

		let mut buf = vec![0; expected.len()];

		let mut curr_len = 0;

		let mut reader = MultiReader::new(test_data);

		for read in test_reads {
			let read_into_slice = &mut buf[curr_len..(curr_len + read)];
			curr_len += read;
			reader.read(read_into_slice).unwrap();
		}

		assert_eq!(expected.as_ref(), &buf);
	}
}