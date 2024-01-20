// TODO: Add more to logging macros (timestamps etc.?) also should logging shit be purely the responsibility of the binary crate, or... idk. Maybe put logging behind a feature or something...
#![allow(unused)]

/// Output verbose information
#[macro_export]
macro_rules! sl_vinfo { // TODO: This should check a global settings variable or something cause this is verbose output
	($source: expr, $msg: expr) => {{
		use colored::Colorize;
		eprintln!("[{}/{}]: {}", $source, "INFO".blue(), $msg);
	}};
}

/// Output information
#[macro_export]
macro_rules! sl_info {
	($source: expr, $msg: expr) => {{
		use colored::Colorize;
		eprintln!("[{}/{}]: {}", $source, "INFO".blue(), $msg);
	}};
}

#[macro_export]
macro_rules! sl_warn {
	($source: expr, $msg: expr) => {{
		use colored::Colorize;
		eprintln!("[{}/{}]: {}", $source, "WARN".yellow(), $msg);
	}};
}

#[macro_export]
macro_rules! sl_error {
	($source: expr, $msg: expr) => {{
		use colored::Colorize;
		eprintln!("[{}/{}]: {}", $source, "ERROR".red(), $msg);
	}};
}

pub use sl_info;
pub use sl_warn;
pub use sl_error;