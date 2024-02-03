use clap::Parser;
use clap_verbosity_flag::InfoLevel;

#[derive(Debug, Parser)]
pub struct Args {
	#[command(flatten)]
	pub verbose: clap_verbosity_flag::Verbosity<InfoLevel>,
}