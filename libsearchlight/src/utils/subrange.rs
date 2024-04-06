use std::ops::{Deref, Range};

pub trait IntoSubrangesExact<T>: Sized {
	/// Much like chunk_exact, but yields ranges
	fn into_subranges_exact(self, subrange_size: usize) -> SubrangesExact<T>;
}

pub struct SubrangesExact<T> {
	subranges: Vec<Range<T>>,
	remainder: Option<Range<T>>
}

impl<T> SubrangesExact<T> {
	pub fn remainder(&self) -> &Option<Range<T>> {
		&self.remainder
	}
}

impl<T> Deref for SubrangesExact<T> {
	type Target = Vec<Range<T>>;

	fn deref(&self) -> &Self::Target {
		&self.subranges
	}
}

impl<T> IntoSubrangesExact<T> for Range<T> where Range<T>: Iterator<Item = T> + ExactSizeIterator, T: PartialOrd + Copy {
	fn into_subranges_exact(mut self, subrange_size: usize) -> SubrangesExact<T> {
		let range_len = self.len();
		let mut stored: Option<T> = None;
		let num_chunks = range_len / subrange_size;
		let mut res = Vec::with_capacity(num_chunks);

		let mut i = 0;
		let remainder = loop {
			if let Some(curr) = self.nth(0) {
				let start = stored.replace(curr);

				if let Some(start) = start {
					res.push(start..curr);
				}

				i += subrange_size;
				if i <= range_len {
					self.nth(subrange_size - 2); // Skip forward to the next important value
				} else {
					break if let Some(start) = stored.take() {
						Some(start..self.end)
					} else {
						Some(self.start..self.end)
					};
				}
			} else {
				if let Some(start) = stored.take() {
					res.push(start..self.end);
				}
				break None;
			}
		};

		SubrangesExact {
			subranges: res,
			remainder
		}
	}
}

#[cfg(test)]
mod test {
    use super::IntoSubrangesExact;

	#[test]
	fn test_into_subranges_exact() {
		let test_range = 0..20;

		let expected = vec![0..5, 5..10, 10..15, 15..20];

		let res = test_range.into_subranges_exact(5);

		assert_eq!(*res, expected);
	}

	#[test]
	fn test_ord_subranges_exact() {
		let test_range = 0..20;

		let expected = vec![0..5, 5..10, 10..15, 15..20];

		let res = {
			let chunk_size = 5;
			let mut stored = None;
			let num_chunks = (test_range.end - test_range.start) / chunk_size;
			let mut res = Vec::with_capacity(num_chunks);

			let mut i = test_range.start;
			while i <= test_range.end {
				let start = stored.replace(i);

				if let Some(start) = start {
					res.push(start..i);
				}

				i += chunk_size;
			}

			res
		};

		assert_eq!(expected, res);
	}
}