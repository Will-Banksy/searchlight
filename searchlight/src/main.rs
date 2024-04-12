mod args;

use std::{fs, io::Write, time::SystemTime};

use args::Args;
use clap::Parser;
use libsearchlight::searchlight::{CarveOperationInfo, Searchlight};
use log::{debug, error, info};

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

	debug!("Args: {:?}", args);

	let mut searchlight = Searchlight::new();

	if let Some(input) = args.input {
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

		debug!("Config: {:?}", config);

		searchlight.add_operation(CarveOperationInfo::Image {
			path: input,
			config,
			cluster_size: args.cluster_size.as_option(),
			skip_carving: args.skip_carving
		});
	}

	if let Some(log_path) = args.carve_log {
		searchlight.add_operation(CarveOperationInfo::FromLog {
			path: log_path
		})
	}

	loop {
		match searchlight.process_file(args.out_dir.clone().unwrap_or(humantime::format_rfc3339(SystemTime::now()).to_string())) {
			(Some(info), Ok(true)) => {
				info!("Finished processing file \"{}\"", info.path());
			}
			(_, Ok(false)) => {
				info!("No files left to process, exiting");
				break;
			}
			(Some(info), Err(e)) => {
				error!("Failed to process file \"{}\": {}", info.path(), e);
			}
			_ => {
				unimplemented!()
			}
		}
	}
}