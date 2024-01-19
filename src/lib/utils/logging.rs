// TODO: Add more to logging macros (timestamps etc.?)
#![allow(unused)]

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