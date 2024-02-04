// TODO: Go through the BUG: unwrap markings and sort out the ones that are actually a bug and those that are intentional, and try fix those that are a bug
// NOTE: Queuing read operations with io_uring might have a more substantial performance improvement for HDDs, as it may be able to reduce the amount of disk rotations - but for a single file, would it be any better? Perhaps look into this
// TODO: Maybe change io_test.dat to be more random or to hit more edge cases or something
mod args;

use std::{fs, io::Write, time::SystemTime};

use args::Args;
use clap::Parser;
use libsearchlight::searchlight::Searchlight;
use log::{error, info};

fn main() {
	let mut args = Args::parse();

	env_logger::Builder::new()
		.filter_level(args.verbose.log_level_filter())
		.format(|f, record| {
			let level_style = f.default_level_style(record.level());
			writeln!(f, "[{} {}/{}{}{}]: {}", f.timestamp(), record.target(), level_style.render(), record.level(), level_style.render_reset(), record.args())
		})
		.init();

	info!("args: {:?}", args);

	args.config = Some(args.config.unwrap_or("Searchlight.toml".to_string()));

	let config = match fs::read_to_string(args.config.as_ref().unwrap()) {
		Ok(config_string) => match toml::from_str(&config_string) {
			Ok(config) => config,
			Err(e) => {
				error!("Error processing config file \"{}\": {}", args.config.unwrap(), e);
				return;
			}
		},
		Err(e) => {
			error!("Could not open config file \"{}\": {}", args.config.unwrap(), e);
			return;
		}
	};

	info!("config: {:?}", config);

	let mut searchlight = match Searchlight::new(config) {
		Ok(searchlight) => searchlight.with_file(&args.input),
		Err(e) => {
			error!("Failed to initialise Searchlight: {}", e);
			return;
		}
	};

	searchlight.process_file(args.out_dir.unwrap_or(humantime::format_rfc3339(SystemTime::now()).to_string())).unwrap();

	// let result = searchlight.open("path/to/image");
	// if let Err(e) = result {
	// 	sl_error!("main", format!("Failed to open disk image file {}", e));
	// 	return;
	// }
}