use std::{hash::{Hash, Hasher}, collections::{HashMap, hash_map::DefaultHasher}};

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
pub struct PfacTableBuilder {
	pat_ir: Vec<NodeIR>,
	start_idx: u32,
	end_idx: u32,
	do_suffix_opt: bool,
	suffix_idx_map: HashMap<u64, u32>
}

#[derive(Debug)]
pub struct PfacTableElem {
	pub next_state: u32,
	pub value: u8
}

pub struct PfacTable {
	pub table: Vec<Vec<PfacTableElem>>
}

impl PfacTableBuilder {
	pub fn new(do_suffix_opt: bool) -> Self {
		let start = NodeIR { next_paths: Vec::new() };
		let end = NodeIR { next_paths: Vec::new(), };

		PfacTableBuilder { pat_ir: vec![start, end], start_idx: 0, end_idx: 1, do_suffix_opt, suffix_idx_map: HashMap::new() }
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
	}

	pub fn build(self) -> PfacTable {
		let table: Vec<Vec<PfacTableElem>> = self.pat_ir.into_iter()
			.map(|node| {
				node.next_paths.into_iter()
					.map(|conn| PfacTableElem { next_state: conn.connecting_to_uuid, value: conn.value })
					.collect()
			})
			.collect();

		println!("PfacTable: {:?}", table);

		PfacTable { table }
	}
}

impl PfacTable {
	pub fn lookup(&self, curr_state: u32, value: u8) -> Option<&PfacTableElem> {
		self.table.get(curr_state as usize)?.iter().find(|e| e.value == value)
	}

	/// Encodes the pfac table into an array of u64 values, where each u64 contains, as the most significant u32, the state number, and the least significant u32, the value, prefixing each Vec with a u64 of the Vec's length.
	/// Each row is resized to be the same length, so the resulting Vec can be indexed as (i * row_len, j) where i is the row index, and j is the column index.
	///
	/// Returns the u64 Vec, with the first element being the length of the rows
	pub fn encode(&self) -> Vec<u64> {
		// Compute the maximum row size, +1 for each row needs to indicate it's length
		let rlen = self.table.iter().fold(0, |acc, elem| if elem.len() > acc { elem.len() } else { acc }) + 1;

		let mut accum = vec![0; self.table.len() * rlen];

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
}

#[cfg(test)]
mod test {
    use crate::lib::search::pfac_common::ir::{NodeIR, ConnectionIR};

    use super::PfacTableBuilder;

	#[test]
	fn test_ir_gen() {
		let patterns: [&[u8]; 4] = [ &[ 45, 32, 23, 97 ], &[ 87, 34, 12 ], &[ 87, 45, 12 ], &[ 29, 45, 32, 23, 97 ] ];

		let mut pb = PfacTableBuilder::new(true);

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

fn hash_suffix(suffix: &[u8]) -> u64 {
	let mut hasher = DefaultHasher::new();
	suffix.hash(&mut hasher);
	hasher.finish()
}