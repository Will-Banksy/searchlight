use std::{thread, num::NonZeroUsize, sync::{RwLock, Arc}};

use scoped_thread_pool::Pool;

use super::{pfac_common::PfacTable, Match, match_id_hash_init, match_id_hash_add};

struct PfacState {
	state: u32,
	id: u64,
	start_idx: usize
}

pub struct PfacCpu {
	table: PfacTable,
	thread_pool: Pool,
	running_search_states: Arc<RwLock<Vec<PfacState>>>
}

impl PfacCpu {
	pub fn new(table: PfacTable) -> Self {
		let thread_num = thread::available_parallelism().unwrap_or(NonZeroUsize::new(8).unwrap()).into();

		PfacCpu {
			table,
			thread_pool: Pool::new(thread_num),
			running_search_states: Arc::new(RwLock::new(Vec::new()))
		}
	}

	pub fn search_next(&mut self, data: &[u8], data_offset: u64) -> Vec<Match> {
		let matches: Arc<RwLock<Vec<Match>>> = Arc::new(RwLock::new(Vec::new()));
		let pfac_cpu = &*self;
		pfac_cpu.thread_pool.scoped(|scope| {
			for pfac_state in self.running_search_states.write().unwrap().drain(..) {
				let matches = Arc::clone(&matches);
				let mut state = pfac_state.state;
				let start_idx = pfac_state.start_idx;
				let mut id = pfac_state.id;
				scope.execute(move || {
					let mut i = 0;
					loop {
						if i > data.len() {
							pfac_cpu.running_search_states.write().unwrap().push(PfacState { state, id, start_idx })
						}

						if let Some(elem) = pfac_cpu.table.lookup(state, data[i]) {
							state = elem.next_state;
							i += 1;
							id = match_id_hash_add(id, elem.value);
						} else if pfac_cpu.table.table[state as usize].is_empty() {
							// Found a match
							matches.write().unwrap().push(
								Match {
									id,
									start_idx: start_idx as u64,
									end_idx: i as u64 + data_offset - 1
								}
							);
							break;
						} else {
							break;
						}
					}
				})
			}

			for i in 0..data.len() {
				let matches = Arc::clone(&matches);
				scope.execute(move || {
					let mut state = 0;
					let start_idx = i;
					let mut i = start_idx;
					let mut id = match_id_hash_init();
					loop {
						if i >= data.len() {
							pfac_cpu.running_search_states.write().unwrap().push(PfacState { state, id, start_idx });
							break;
						}

						if let Some(elem) = pfac_cpu.table.lookup(state, data[i]) {
							state = elem.next_state;
							i += 1;
							id = match_id_hash_add(id, elem.value);
						} else if pfac_cpu.table.table[state as usize].is_empty() {
							// Found a match
							matches.write().unwrap().push(
								Match {
									id,
									start_idx: start_idx as u64 + data_offset,
									end_idx: i as u64 + data_offset - 1
								}
							);
							break;
						} else {
							break;
						}
					}
				})
			}
		});

		// This looks scary because lots of unwrapping but it should never panic
		Arc::into_inner(matches).unwrap().into_inner().unwrap()
	}
}

#[cfg(test)]
mod test {
    use crate::lib::search::{Match, pfac_cpu::PfacCpu, pfac_common::PfacTableBuilder, match_id_hash_slice};

	#[test]
	fn test_pfac_cpu_single() {
		let buffer = [ 1, 2, 3, 8, 4, 1, 2, 3, 1, 1, 2, 1, 2, 3, 0, 5, 9, 1, 2 ];

		let pattern = &[1, 2, 3];
		let pattern_id = match_id_hash_slice(pattern);

		let pfac_table = PfacTableBuilder::new(true).with_pattern(pattern).build();
		let mut pfac = PfacCpu::new(pfac_table);
		let matches = pfac.search_next(&buffer, 0);

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

		assert_eq!(matches, expected);
	}

	#[test]
	fn test_pfac_cpu_multi() {
		let buffer = [ 1, 2, 3, 8, 4, 1, 2, 3, 1, 1, 2, 1, 2, 3, 0, 5, 9, 1, 2 ];

		let pattern = &[1, 2, 3];
		let pattern_id = match_id_hash_slice(pattern);

		let pfac_table = PfacTableBuilder::new(true).with_pattern(pattern).build();
		let mut pfac = PfacCpu::new(pfac_table);
		let mut matches = pfac.search_next(&buffer[..6], 0);
		matches.append(&mut pfac.search_next(&buffer[6..], 6));

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

		assert_eq!(matches, expected);
	}
}