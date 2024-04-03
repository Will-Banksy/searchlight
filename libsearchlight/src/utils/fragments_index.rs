use std::ops::Index;

use crate::validation::Fragment;

// NOTE: Could I, instead of having a specialised FragmentsIndex, decouple this logic into a `indexes_to_slices` (file_data + frags into
//       a vec of slices of file_data) and a `FlatSlice`/`FlatIndex` struct that indexes through a slice of slices? Heck I could actually just reuse
//       iterators probably (Iterator::flatten)

pub struct FragmentsIndex<'d, 'f> {
	file_data: &'d [u8],
	frags: &'f [Fragment],
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
			len: counter
		}
	}

	pub fn len(&self) -> usize {
		self.len
	}
}

impl<'d, 'f> Index<usize> for FragmentsIndex<'d, 'f> {
	type Output = u8;

	/// Indexes into the fragments, i.e. if frags = [4..7, 10..15] then idx 0 would be file_data[4] and idx 5 would be file_data[10]
	fn index(&self, index: usize) -> &Self::Output { // PERF: Precomputation optimisation?
		let mut counter = 0;

		for f in self.frags {
			if counter + ((f.end - f.start) as usize) > index {
				let file_idx = f.clone().nth(index - counter).unwrap() as usize;
				return &self.file_data[file_idx];
			} else {
				counter += (f.end - f.start) as usize;
			}
		}

		panic!("Index {index} out of bounds for len {counter}");
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
}