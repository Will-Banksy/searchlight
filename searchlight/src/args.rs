use std::str::FromStr;

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
	/// The cluster size that the filesystem that is/was present in the disk image allocated files in, i.e. all valid non-embedded file headers will be found at multiples of this value.
	/// Alternatively, you can specify "unaligned" or "unknown"
	#[arg(short = 'l', long, default_value = "unknown")]
	pub cluster_size: ClusterSizeArg,
	/// The output directory to save recovered file contents in. Defaults to a timestamped directory (startup time) in the current working directory
	#[arg(short, long)]
	pub out_dir: Option<String>,
	/// Whether to simply output a log of the discovered file locations instead of carving the file data. Defaults to false. Currently unimplemented
	#[arg(short, long)]
	pub skip_carving: bool,
	/// Path to the TOML config file. Defaults to looking for "Searchlight.toml" in the current working directory
	#[arg(short, long)]
	pub config: Option<String>
}

#[derive(Debug, Clone)]
pub enum ClusterSizeArg {
	Unknown,
	Unaligned,
	Known(u64)
}

impl FromStr for ClusterSizeArg {
	type Err = <u64 as FromStr>::Err;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.trim() {
			"unknown" => Ok(ClusterSizeArg::Unknown),
			"unaligned" => Ok(ClusterSizeArg::Unaligned),
			value => Ok(ClusterSizeArg::Known(value.parse::<u64>()?))
		}
	}
}

impl ClusterSizeArg {
	pub fn as_options(&self) -> Option<Option<u64>> {
		match self {
			ClusterSizeArg::Unknown => None,
			ClusterSizeArg::Unaligned => Some(None),
			ClusterSizeArg::Known(val) => Some(Some(*val))
		}
	}
}