use clap::Parser;
use clap_verbosity_flag::InfoLevel;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
	#[command(flatten)]
	pub verbose: clap_verbosity_flag::Verbosity<InfoLevel>,
	/// Path to the input disk image file to attempt to recover data from
	#[arg(short, long)]
	pub input: String,
	/// The output directory to save recovered file contents in. Defaults to a timestamped directory (startup time) in the current working directory
	#[arg(short, long)]
	pub out_dir: Option<String>,
	/// Whether to simply output a log of the discovered file locations instead of carving the file data. Defaults to false
	#[arg(short, long)]
	pub skip_carving: bool,
	/// Path to the TOML config file. Defaults to looking for "Searchlight.toml" in the current working directory
	#[arg(short, long)]
	pub config: Option<String>
}