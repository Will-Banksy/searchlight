// TODO: Go through the BUG: unwrap markings and sort out the ones that are actually a bug and those that are intentional, and try fix those that are a bug
// NOTE: Queuing read operations with io_uring might have a more substantial performance improvement for HDDs, as it may be able to reduce the amount of disk rotations - but for a single file, would it be any better? Perhaps look into this
// TODO: Introduce feature flag for vulkan so it can be continued to be tested with github actions
// TODO: Maybe change io_test.dat to be more random or to hit more edge cases or something

use searchlight::{sl_info, sl_warn, sl_error};

fn main() {
	sl_info!("main", "Hello world!");
	sl_warn!("main", "Hello world!");
	sl_error!("main", "Hello world!");
}