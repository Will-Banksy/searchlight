// TODO: Go through the BUG: unwrap markings and sort out the ones that are actually a bug and those that are intentional, and try fix those that are a bug
// NOTE: Queuing read operations with io_uring might have a more substantial performance improvement for HDDs, as it may be able to reduce the amount of disk rotations - but for a single file, would it be any better? Perhaps look into this
// TODO: Introduce feature flag for vulkan so it can be continued to be tested with github actions
// TODO: Maybe change io_test.dat to be more random or to hit more edge cases or something

use std::fs;

use searchlight::{sl_error, lib::searchlight::{config::SearchlightConfig, Searchlight}, sl_info};

fn main() {
	let config_string = fs::read_to_string("Searchlight.toml");
	if let Err(e) = config_string {
		sl_error!("main", format!("Could not open config file \"Searchlight.toml\": {}", e));
		return;
	}
	let config_string = config_string.unwrap();

	let config = toml::from_str(&config_string);
	if let Err(e) = config {
		sl_error!("main", format!("Error processing config file \"Searchlight.toml\": {}", e));
		return;
	}
	let config: SearchlightConfig = config.unwrap();

	if !config.quiet {
		sl_info!("main", format!("config: {:?}", config));
	}

	let searchlight = Searchlight::new(config);
	if let Err(e) = searchlight {
		sl_error!("main", format!("Failed to initialise Searchlight: {}", e));
		return;
	}
	let searchlight = searchlight.unwrap();

	// let result = searchlight.open("path/to/image");
	// if let Err(e) = result {
	// 	sl_error!("main", format!("Failed to open disk image file {}", e));
	// 	return;
	// }
}