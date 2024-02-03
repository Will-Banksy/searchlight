use crate::error::Error;

use super::{search_common::AcTable, SearchFuture, match_id_hash_init, match_id_hash_add, Match, Searcher};

struct AcState {
	state: u32,
	id: u64,
	start_idx: usize
}

pub struct AcCpu {
	table: AcTable,
	states: Vec<AcState>
}

impl AcCpu {
	pub fn new(table: AcTable) -> Self {
		AcCpu {
			table,
			states: Vec::new()
		}
	}
}

impl Searcher for AcCpu {
	fn search_next(&mut self, data: &[u8], data_offset: u64) -> Result<SearchFuture, Error> {
		let skip = (self.table.max_pat_len as usize).min(data.len() - 1);
		self.search(&data[skip..], data_offset + skip as u64)
	}

	fn search(&mut self, data: &[u8], data_offset: u64) -> Result<SearchFuture, Error> {
		let mut matches = Vec::new();

		let mut i = 0;
		loop {
			if i >= data.len() {
				break;
			}

			let mut j = 0;
			while j < self.states.len() {
				if let Some(elem) = self.table.lookup(self.states[j].state, data[i]) {
					self.states[j].state = elem.next_state;
					self.states[j].id = match_id_hash_add(self.states[j].id, data[i]);
				} else if self.table.table[self.states[j].state as usize].is_empty() {
					matches.push(Match {
						id: self.states[j].id,
						start_idx: self.states[j].start_idx as u64,
						end_idx: i as u64 + data_offset - 1
					});
					self.states.remove(j);
					continue;
				} else {
					self.states.remove(j);
					continue;
				}

				j += 1;
			}

			if let Some(elem) = self.table.lookup(0, data[i]) {
				self.states.push(AcState {
					state: elem.next_state,
					id: match_id_hash_add(match_id_hash_init(), data[i]),
					start_idx: i + data_offset as usize
				})
			}

			i += 1;
		}

		Ok(SearchFuture::new(|| Ok(matches)))
	}
}

#[cfg(test)]
mod test {
	use crate::search::{Match, search_common::AcTableBuilder, match_id_hash_slice, ac_cpu::AcCpu, Searcher};

	#[test]
	fn test_ac_cpu_single() {
		let buffer = [
			1, 2, 3, 8, 4,
			1, 2, 3, 1, 1,
			2, 1, 2, 3, 0,
			5, 9, 1, 2
		];

		let pattern = &[1, 2, 3];
		let pattern_id = match_id_hash_slice(pattern);

		let pfac_table = AcTableBuilder::new(true).with_pattern(pattern).build();
		let mut ac = AcCpu::new(pfac_table);
		let matches = ac.search(&buffer, 0).unwrap();

		let expected = vec![
			Match {
				id: pattern_id,
				start_idx: 0,
				end_idx: 2
			},
			Match {
				id: pattern_id,
				start_idx: 5,
				end_idx: 7
			},
			Match {
				id: pattern_id,
				start_idx: 11,
				end_idx: 13
			}
		];

		assert_eq!(matches.wait().unwrap(), expected);
	}

	#[test]
	fn test_ac_cpu_multi() {
		let buffer = [ 1, 2, 3, 4, 5, 8, 4, 1, 2, 3, 4, 5, 1, 1, 2, 1, 2, 3, 4, 5, 0, 5, 9, 1, 2 ];

		let pattern = &[ 1, 2, 3, 4, 5 ];
		let pattern_id = match_id_hash_slice(pattern);

		let pfac_table = AcTableBuilder::new(true).with_pattern(pattern).build();
		let mut ac = AcCpu::new(pfac_table);
		let mut matches = ac.search(&buffer[..8], 0).unwrap().wait().unwrap();
		matches.append(&mut ac.search_next(&buffer[3..10], 3).unwrap().wait().unwrap());
		matches.append(&mut ac.search_next(&buffer[5..], 5).unwrap().wait().unwrap());

		let expected = vec![
			Match {
				id: pattern_id,
				start_idx: 0,
				end_idx: 4
			},
			Match {
				id: pattern_id,
				start_idx: 7,
				end_idx: 11
			},
			Match {
				id: pattern_id,
				start_idx: 15,
				end_idx: 19
			}
		];

		assert_eq!(matches, expected);
	}
}