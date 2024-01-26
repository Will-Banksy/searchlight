use std::{hash::{Hash, Hasher}, collections::{HashMap, hash_map::DefaultHasher}};

use crate::sl_info;

use self::ir::{NodeIR, ConnectionIR};

mod ir {
	#[derive(Debug, PartialEq)]
	pub struct NodeIR {
		pub next_paths: Vec<ConnectionIR>,
	}

	#[derive(Debug, PartialEq)]
	pub struct ConnectionIR {
		pub connecting_to_uuid: u32,
		pub value: u8,
	}
}

#[derive(Debug)]
pub struct AcTableBuilder {
	pat_ir: Vec<NodeIR>,
	start_idx: u32,
	end_idx: u32,
	do_suffix_opt: bool,
	suffix_idx_map: HashMap<u64, u32>,
	max_pat_len: u32
}

#[derive(Debug, Clone)]
pub struct AcTableElem {
	pub next_state: u32,
	pub value: u8
}

#[derive(Clone)]
pub struct AcTable {
	pub table: Vec<Vec<AcTableElem>>,
	pub max_pat_len: u32
}

impl AcTableBuilder {
	pub fn new(do_suffix_opt: bool) -> Self {
		let start = NodeIR { next_paths: Vec::new() };
		let end = NodeIR { next_paths: Vec::new(), };

		AcTableBuilder {
			pat_ir: vec![start, end],
			start_idx: 0,
			end_idx: 1,
			do_suffix_opt,
			suffix_idx_map: HashMap::new(),
			max_pat_len: 0
		}
	}

	pub fn with_pattern(mut self, pattern: &[u8]) -> Self {
		self.add_pattern(pattern);

		self
	}

	pub fn add_pattern(&mut self, pattern: &[u8]) {
		let mut node_idx = self.start_idx as usize;

		for (i, b)  in pattern.iter().enumerate() {
			if let Some(conn) = self.pat_ir[node_idx].next_paths.iter().find(|conn| conn.value == *b) {
				node_idx = conn.connecting_to_uuid as usize;
			} else {
				let suffix = &pattern[(i + 1)..];
				let next_node_idx = {
					if i == pattern.len() - 1 {
						self.end_idx
					} else if let Some(suffix_idx) = self.suffix_idx_map.get(&hash_suffix(suffix)) {
						*suffix_idx
					} else {
						let new_node_idx = self.pat_ir.len() as u32;
						self.pat_ir.push(NodeIR { next_paths: Vec::new() });
						if self.do_suffix_opt {
							self.suffix_idx_map.insert(hash_suffix(suffix), new_node_idx);
						}
						new_node_idx
					}
				};
				self.pat_ir[node_idx].next_paths.push(ConnectionIR { connecting_to_uuid: next_node_idx, value: *b });
				node_idx = next_node_idx as usize;
			}
		}

		self.max_pat_len = self.max_pat_len.max(pattern.len() as u32);
		println!("Pattern {:?} len = {}", pattern, pattern.len());
	}

	pub fn build(self) -> AcTable {
		let table: Vec<Vec<AcTableElem>> = self.pat_ir.into_iter()
			.map(|node| {
				node.next_paths.into_iter()
					.map(|conn| AcTableElem { next_state: conn.connecting_to_uuid, value: conn.value })
					.collect()
			})
			.collect();

		sl_info!("search_common", format!("AC Table: {:?}", table));

		AcTable { table, max_pat_len: self.max_pat_len }
	}
}

impl AcTable {
	pub fn lookup(&self, curr_state: u32, value: u8) -> Option<&AcTableElem> {
		self.table.get(curr_state as usize)?.iter().find(|e| e.value == value)
	}

	pub fn num_rows(&self) -> usize {
		self.table.len()
	}

	pub fn indexable_columns(&self) -> usize {
		256
	}

	/// Encodes the ac table into an array of u64 values, where each u64 contains, as the most significant u32, the state number, and the least significant u32, the value, prefixing each Vec with a u64 of the Vec's length.
	/// Each row is resized to be the same length, so the resulting Vec can be indexed as (i * row_len, j) where i is the row index, and j is the column index.
	///
	/// Returns the u64 Vec, with the first element being the length of the rows
	pub fn encode(&self) -> Vec<u64> {
		// Compute the maximum row size, +1 for each row needs to indicate it's length
		let rlen = self.table.iter().fold(0, |acc, elem| if elem.len() > acc { elem.len() } else { acc }) + 1;

		let mut accum = vec![0; self.num_rows() * rlen];

		let mut i = 0;
		for row in &self.table {
			let accum_idx = i * rlen;

			// Encode the row by shifting each element's next state to the left 32 places and or'ing it with the element's value
			let mut row_encoded: Vec<u64> = row.iter().map(|e| ((e.next_state as u64) << 32) | e.value as u64).collect();
			row_encoded.insert(0, row_encoded.len() as u64);

			// Write the encoded row to the corresponding range of positions in accum
			let mut j = 0;
			while j < (i + 1) * rlen && j < row_encoded.len() {
				accum[accum_idx + j] = row_encoded[j];

				j += 1;
			}

			i += 1;
		}

		accum.insert(0, rlen as u64);

		accum
	}

	pub fn encode_indexable(&self) -> Vec<u32> {
		let rlen = self.indexable_columns();

		let mut accum = vec![0u32; rlen * self.num_rows()];

		for (i, row) in self.table.iter().enumerate() {
			if row.is_empty() {
				for j in 0..rlen {
					accum[i * rlen + j] = u32::MAX;
				}
			}
			for elem in row {
				accum[i * rlen + elem.value as usize] = elem.next_state;
			}
		}

		// TODO: Remove debugging code
		// {
		// 	let arr2d: Vec<&[u32]> = accum.chunks(256).collect();

		// 	for (row_idx, row) in arr2d.iter().enumerate() {
		// 		for (elem_idx, elem) in row.iter().enumerate() {
		// 			if *elem != 0 {
		// 				println!("elem at row {row_idx}, column {elem_idx} is {elem}");
		// 			}
		// 		}
		// 	}
		// }

		accum
	}
}

fn hash_suffix(suffix: &[u8]) -> u64 {
	let mut hasher = DefaultHasher::new();
	suffix.hash(&mut hasher);
	hasher.finish()
}

#[cfg(test)]
mod test {
    use crate::lib::search::search_common::ir::{NodeIR, ConnectionIR};

    use super::AcTableBuilder;

	#[test]
	fn test_encode_indexable() {
		// let patterns: [&[u8]; 2] = [
		// 	&[ 69, 69, 69, 69 ],
		// 	&[ 10, 1, 9, 2, 8, 3, 5, 4, 6 ],
		// ];

		let patterns = [&[ 1, 2, 3 ]];

		let mut tb = AcTableBuilder::new(true);

		for p in patterns {
			tb.add_pattern(p);
		}

		let encoded = tb.build().encode_indexable();

		println!("encoded len: {}", encoded.len());

		let arr2d: Vec<&[u32]> = encoded.chunks(256).collect();

		for (row_idx, row) in arr2d.iter().enumerate() {
			for (elem_idx, elem) in row.iter().enumerate() {
				if *elem != 0 {
					println!("elem at row {row_idx}, column {elem_idx} is {elem}");
				}
			}
		}
	}

	#[test]
	fn test_ir_gen() {
		let patterns: [&[u8]; 4] = [ &[ 45, 32, 23, 97 ], &[ 87, 34, 12 ], &[ 87, 45, 12 ], &[ 29, 45, 32, 23, 97 ] ];

		let mut pb = AcTableBuilder::new(true);

		for p in patterns {
			pb.add_pattern(p);
		}

		let expected_ir = [
			NodeIR {
				next_paths: vec![
					ConnectionIR {
						connecting_to_uuid: 2,
						value: 45,
					},
					ConnectionIR {
						connecting_to_uuid: 5,
						value: 87,
					},
					ConnectionIR {
						connecting_to_uuid: 7,
						value: 29,
					},
				],
			},
			NodeIR {
				next_paths: vec![],
			},
			NodeIR {
				next_paths: vec![
					ConnectionIR {
						connecting_to_uuid: 3,
						value: 32,
					},
				],
			},
			NodeIR {
				next_paths: vec![
					ConnectionIR {
						connecting_to_uuid: 4,
						value: 23,
					},
				],
			},
			NodeIR {
				next_paths: vec![
					ConnectionIR {
						connecting_to_uuid: 1,
						value: 97,
					},
				],
			},
			NodeIR {
				next_paths: vec![
					ConnectionIR {
						connecting_to_uuid: 6,
						value: 34,
					},
					ConnectionIR {
						connecting_to_uuid: 6,
						value: 45,
					},
				],
			},
			NodeIR {
				next_paths: vec![
					ConnectionIR {
						connecting_to_uuid: 1,
						value: 12,
					},
				],
			},
			NodeIR {
				next_paths: vec![
					ConnectionIR {
						connecting_to_uuid: 2,
						value: 45,
					},
				],
			},
		];

		assert_eq!(pb.pat_ir, expected_ir)
	}
}