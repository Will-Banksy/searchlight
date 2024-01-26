pub trait ToChunksExact<I> where I: Iterator {
	fn to_chunks_exact(self, chunk_size: usize) -> ChunksIterExact<I>;
}

impl<I> ToChunksExact<I> for I where I: Iterator {
    fn to_chunks_exact(self, chunk_size: usize) -> ChunksIterExact<I> {
        ChunksIterExact::new(self, chunk_size)
    }
}

pub struct ChunksIterExact<I> where I: Iterator {
	iter: I,
	chunk_size: usize,
	collector: Vec<I::Item>
}

impl<'a, I> ChunksIterExact<I> where I: Iterator {
	pub fn new(iter: I, chunk_size: usize) -> Self {
		ChunksIterExact {
			iter,
			chunk_size,
			collector: Vec::with_capacity(chunk_size)
		}
	}
}

impl<I> Iterator for ChunksIterExact<I> where I: Iterator {
    type Item = Vec<I::Item>;

    fn next(&mut self) -> Option<Self::Item> {
		for _ in 0..self.chunk_size {
			self.collector.push(self.iter.next()?);
		}

		Some(self.collector.drain(..).collect())
    }
}

pub trait ToGappedWindows<'a, T> {
	fn to_gapped_windows(&'a self, window_size: usize, window_gap: usize) -> GappedWindows<'a, T>;
}

impl<'a, T> ToGappedWindows<'a, T> for [T] {
	fn to_gapped_windows(&'a self, window_size: usize, window_gap: usize) -> GappedWindows<'a, T> {
		GappedWindows {
			inner: Some(self),
			window_size,
			window_gap
		}
	}
}

pub struct GappedWindows<'a, T> {
	inner: Option<&'a [T]>,
	window_size: usize,
	window_gap: usize,
}

impl<'a, T> Iterator for GappedWindows<'a, T> {
	type Item = &'a [T];

	fn next(&mut self) -> Option<Self::Item> {
		let ret = self.inner?.get(0..self.window_size.min(self.inner?.len()));
		self.inner = self.inner?.get(self.window_gap..);
		ret
	}
}

#[cfg(test)]
mod test {
    use super::ToGappedWindows;

	#[test]
	fn test_gapped_windows() {
		let array = [
			1, 2, 3, 4, 5,
			6, 7, 8, 9, 10,
			11, 12, 13
		];

		let result: Vec<&[i32]> = array.to_gapped_windows(7, 5).collect();

		let expected: &[&[i32]] = &[ &[1, 2, 3, 4, 5, 6, 7], &[6, 7, 8, 9, 10, 11, 12], &[11, 12, 13] ];

		assert_eq!(&result, expected);
	}
}