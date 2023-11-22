use std::{thread::available_parallelism, num::NonZeroUsize, sync::{RwLock, Arc}};

use scoped_thread_pool::Pool;

use super::{pfac_common::PfacTable, Match};

struct PfacCpu {
	table: PfacTable,
	thread_pool: Pool
}

impl PfacCpu {
	pub fn new(table: PfacTable) -> Self {
		let thread_num = available_parallelism().unwrap_or(NonZeroUsize::new(8).unwrap()).into();

		PfacCpu {
			table,
			thread_pool: Pool::new(thread_num)
		}
	}

	pub fn search_next(&mut self, data: &[u8]) { // TODO: Save state to avoid the boundary problem
		let matches: Arc<RwLock<Vec<Match>>> = Arc::new(RwLock::new(Vec::new()));
		let pfac_cpu = &*self;
		pfac_cpu.thread_pool.scoped(|scope| {
			for i in 0..data.len() {
				let matches = Arc::clone(&matches);
				scope.execute(move || {
					let mut state = 0;
					let start_idx = i;
					let mut i = start_idx;
					loop {
						if let Some(elem) = pfac_cpu.table.lookup(state, data[i]) {
							state = elem.next_state;
							i += 1;
						} else if pfac_cpu.table.table[state as usize].is_empty() {
							// Found a match
							matches.write().unwrap().push(
								Match {
									id: 0, // TODO: Once decided on hash function, this can be changed to the final hash value
									start_idx: start_idx as u64,
									end_idx: i as u64
								}
							);
							break;
						}
					}
				})
			}
		})
	}
}

#[cfg(test)]
mod test {
	#[test]
	fn test_pfac_cpu() {
		todo!() // TODO: Testing of PfacCpu
	}
}