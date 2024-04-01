mod args;

use std::{fs, io::Write, time::SystemTime};

use args::Args;
use clap::Parser;
use libsearchlight::searchlight::{DiskImageInfo, Searchlight};
use log::{debug, error};

#[cfg(not(target_pointer_width = "64"))]
compile_error!("Target architecture is not 64-bit - This software is only supported on 64-bit platforms");

fn main() {
	let mut args = Args::parse();

	env_logger::Builder::new()
		.filter_level(args.verbose.log_level_filter())
		.format(|f, record| {
			let level_style = f.default_level_style(record.level());
			writeln!(f, "[{} {}/{}{}{}]: {}", f.timestamp(), record.target(), level_style.render(), record.level(), level_style.render_reset(), record.args())
		})
		.init();

	debug!("args: {:?}", args);

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

	debug!("config: {:?}", config);

	let mut searchlight = match Searchlight::new(config) {
		Ok(searchlight) => searchlight.with_file(DiskImageInfo { path: args.input.clone(), cluster_size: args.cluster_size.as_option() }),
		Err(e) => {
			error!("Failed to initialise Searchlight: {}", e);
			return;
		}
	};

	if let Err(e) = searchlight.process_file(args.out_dir.unwrap_or(humantime::format_rfc3339(SystemTime::now()).to_string())) {
		error!("Failed to process file \"{}\": {}", args.input, e);
	}
}