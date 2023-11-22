pub mod pfac_common;
pub mod pfac_cpu;

pub struct Match { // TODO: Need an appropriate hash algorithm to produce the ids from a sequence of numbers - SipHash?
	id: u64,
	start_idx: u64,
	end_idx: u64
}

impl Match {
	pub fn new(id: u64, start_idx: u64, end_idx: u64) -> Self {
		Match {
			id,
			start_idx,
			end_idx
		}
	}
}