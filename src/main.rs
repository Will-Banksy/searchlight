// TODO: Go through the BUG: unwrap markings and sort out the ones that are actually a bug and those that are intentional, and try fix those that are a bug
// NOTE: Queuing read operations with io_uring might have a more substantial performance improvement for HDDs, as it may be able to reduce the amount of disk rotations - but for a single file, would it be any better? Perhaps look into this

use searchlight::lib::search::{pfac_gpu::PfacGpu, pfac_common::PfacTableBuilder};

fn main() {
	let pfac_gpu = PfacGpu::new(PfacTableBuilder::new(true).build(), &[ 2, 5, 8, 9, 1, 3, 7, 4 ]).unwrap();
}