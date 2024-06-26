use std::str::FromStr;

use clap::Parser;
use clap_verbosity_flag::InfoLevel;

// TODO: Add a "quick search" option to only look for headers at the start of clusters... but still need to find footers...
// TODO: Add in-place carving with FUSE/WinFsp
#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
	#[command(flatten)]
	pub verbose: clap_verbosity_flag::Verbosity<InfoLevel>,
	/// If specified, will read the target disk image file and attempt to carve files from it, using the default or specified configuration file and the default or specified cluster size
	#[arg(short, long)]
	pub image: Option<String>,
	/// The cluster size that the filesystem that is/was present in the disk image allocated files in, i.e. all valid non-embedded file headers will be found at multiples of this value.
	/// Alternatively, you can specify "unaligned" or "unknown"
	#[arg(short, long, default_value = "unknown")]
	pub cluster_size: ClusterSizeArg,
	/// The output directory to save recovered file contents in. Defaults to a timestamped directory (processing start time) in the current working directory. Has no effect when processing
	/// a log
	#[arg(short, long)]
	pub out_dir: Option<String>,
	/// Whether to simply output a log of the discovered file locations instead of carving the file data. Defaults to false. Has no effect when processing a log
	#[arg(short, long)]
	pub skip_carving: bool,
	/// Path to the TOML config file. Defaults to looking for "Searchlight.toml" in the current working directory. If only processing a log, searchlight makes no attempt to open a config file
	#[arg(short = 'f', long)]
	pub config: Option<String>,
	/// If specified, will read the target log file and carve the files indicated in it. Doesn't require a config. If specified alongside input, will perform both carving operations separately
	#[arg(short = 'l', long)]
	pub carve_log: Option<String>,
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
	pub fn as_option(&self) -> Option<u64> {
		match self {
			ClusterSizeArg::Unknown => None,
			ClusterSizeArg::Unaligned => Some(1),
			ClusterSizeArg::Known(val) => Some(*val)
		}
	}
}