use std::ops::Index;

use crate::validation::Fragment;

// NOTE: Could I, instead of having a specialised FragmentsIndex, decouple this logic into a `indexes_to_slices` (file_data + frags into
//       a vec of slices of file_data) and a `FlatSlice`/`FlatIndex` struct that indexes through a slice of slices? Heck I could actually just reuse
//       iterators probably (Iterator::flatten)

pub struct FragmentsIndex<'d, 'f> {
	file_data: &'d [u8],
	frags: &'f [Fragment],
	start: usize,
	len: usize
}

impl<'d, 'f> FragmentsIndex<'d, 'f> {
	pub fn new(file_data: &'d [u8], frags: &'f [Fragment]) -> Self {
		let mut counter = 0;

		for f in frags {
			counter += (f.end - f.start) as usize;
		}

		FragmentsIndex {
			file_data,
			frags,
			start: 0,
			len: counter
		}
	}

	// NOTE: We could implement Index<Range<usize>> instead
	pub fn new_sliced(file_data: &'d [u8], frags: &'f [Fragment], start_offset: usize, end_offset: usize) -> Self {
		let mut len = 0;

		for f in frags {
			len += (f.end - f.start) as usize;
		}

		if len.saturating_sub(end_offset) <= start_offset {
			panic!("Error: Offset of {end_offset} from end (len {len}) is before offset from start (index 0) of {start_offset}");
		}

		FragmentsIndex {
			file_data,
			frags,
			start: start_offset,
			len: (len - end_offset).saturating_sub(start_offset)
		}
	}

	pub fn len(&self) -> usize {
		self.len
	}
}

impl<'d, 'f> Index<usize> for FragmentsIndex<'d, 'f> {
	type Output = u8;

	/// Indexes into the fragments, i.e. if frags = [4..7, 10..15] then idx 0 would be file_data[4] and idx 5 would be file_data[10]
	fn index(&self, mut index: usize) -> &Self::Output { // PERF: Precomputation optimisation?
		let mut counter = 0;

		if index >= self.len {
			panic!("Error: Index {index} out of bounds for len {}", self.len);
		}

		index += self.start;

		for f in self.frags {
			if counter + ((f.end - f.start) as usize) > index {
				let file_idx = f.clone().nth(index - counter).unwrap() as usize;
				return &self.file_data[file_idx];
			} else {
				counter += (f.end - f.start) as usize;
			}
		}

		unimplemented!()
	}
}

#[cfg(test)]
mod test {
	use super::FragmentsIndex;

	#[test]
	fn test_fragments_index() {
		let file_data: Vec<u8> = (20..40).collect();

		let frags = [ 4..7, 10..15 ];

		let expected = [ 24, 25, 26, 30, 31, 32, 33, 34 ];

		let frags_index = FragmentsIndex::new(&file_data, &frags);

		assert_eq!(frags_index.len(), expected.len());

		let mut collector = Vec::with_capacity(frags_index.len());

		let mut i = 0;
		while i < frags_index.len() {
			collector.push(frags_index[i]);

			i += 1;
		}

		assert_eq!(collector, expected);
	}

	#[test]
	fn test_fragments_sliced_index() {
		let file_data: Vec<u8> = (20..40).collect();

		let frags = [ 4..7, 10..15 ];

		let expected = [ 25, 26, 30, 31, 32 ];

		let frags_index = FragmentsIndex::new_sliced(&file_data, &frags, 1, 2);

		assert_eq!(frags_index.len(), expected.len());

		let mut collector = Vec::with_capacity(frags_index.len());

		let mut i = 0;
		while i < frags_index.len() {
			collector.push(frags_index[i]);

			i += 1;
		}

		assert_eq!(collector, expected);
	}

	#[test]
	#[should_panic]
	fn test_fragments_sliced_index_panics() {
		let file_data: Vec<u8> = (20..40).collect();

		let frags = [ 4..7, 10..15 ];

		let expected = [ ];

		let frags_index = FragmentsIndex::new_sliced(&file_data, &frags, 4, 5);

		assert_eq!(frags_index.len(), expected.len());

		let mut collector = Vec::with_capacity(frags_index.len());

		let mut i = 0;
		while i < frags_index.len() {
			collector.push(frags_index[i]);

			i += 1;
		}

		assert_eq!(collector, expected);
	}
}