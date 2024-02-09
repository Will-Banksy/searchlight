use std::{hash::{Hash, Hasher}, collections::{HashMap, hash_map::DefaultHasher}};

use log::debug;

use crate::searchlight::config::SearchlightConfig;

use self::ir::{NodeIR, ConnectionIR};

mod ir {
	#[derive(Debug, PartialEq)]
	pub struct NodeIR {
		pub next_paths: Vec<ConnectionIR>,
	}

	#[derive(Debug, PartialEq)]
	pub struct ConnectionIR {
		pub connecting_to_uuid: u32,
		pub value: u16,
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
	pub value: u16
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

	pub fn from_config(config: &SearchlightConfig) -> Self {
		let mut builder = AcTableBuilder::new(true);

		for ft in &config.file_types {
			for head in &ft.headers {
				builder.add_pattern(head);
			}
			for foot in &ft.footers {
				builder.add_pattern(foot);
			}
		}

		builder
	}

	pub fn with_pattern(mut self, pattern: &[u16]) -> Self {
		self.add_pattern(pattern);

		self
	}

	pub fn add_pattern(&mut self, pattern: &[u16]) {
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
	}

	pub fn build(self) -> AcTable {
		let table: Vec<Vec<AcTableElem>> = self.pat_ir.into_iter()
			.map(|node| {
				node.next_paths.into_iter()
					.map(|conn| AcTableElem { next_state: conn.connecting_to_uuid, value: conn.value })
					.collect()
			})
			.collect();

		debug!("AC Table: {:?}", table);

		AcTable { table, max_pat_len: self.max_pat_len }
	}
}

impl AcTable {
	pub fn lookup(&self, curr_state: u32, value: u8) -> Option<&AcTableElem> {
		self.table.get(curr_state as usize)?.iter().find(|e| e.value == value as u16 || e.value == 0x8000)
	}

	pub fn num_rows(&self) -> usize {
		self.table.len()
	}

	pub fn indexable_columns(&self) -> usize {
		257
	}

	/// Returns a 1D vector representation of a 2D array, with 256 columns (width) and a number of rows (height) equal to the number
	/// of unique states, that can be obtained from calling `num_rows`. To get the next state from the table, where y is the current state
	/// and x is the current value, lookup column x and row y.
	///
	/// In the case of '.'s, or match alls, the last column in a row will contain the next state.
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
				if elem.value == 0x8000 {
					accum[i * rlen + rlen - 1] = elem.next_state;
				} else {
					accum[i * rlen + elem.value as usize] = elem.next_state;
				}
			}
		}

		accum
	}
}

fn hash_suffix(suffix: &[u16]) -> u64 {
	let mut hasher = DefaultHasher::new();
	suffix.hash(&mut hasher);
	hasher.finish()
}

#[cfg(test)]
mod test {
    use crate::search::search_common::ir::{NodeIR, ConnectionIR};

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

		let arr2d: Vec<&[u32]> = encoded.chunks(257).collect();

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
		let patterns: [&[u16]; 4] = [ &[ 45, 32, 23, 97 ], &[ 87, 34, 12 ], &[ 87, 45, 12 ], &[ 29, 45, 32, 23, 97 ] ];

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