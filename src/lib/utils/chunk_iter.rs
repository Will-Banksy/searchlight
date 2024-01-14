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