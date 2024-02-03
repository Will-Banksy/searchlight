pub mod config;

use crate::error::Error;

use self::config::SearchlightConfig;

/// The main mediator of the library, this struct manages state
pub struct Searchlight {
	config: SearchlightConfig,
}

impl Searchlight {
	/// Creates a new `Searchlight` instance with the specified config, validating it and returning an error if it
	/// did not successfully validate
	pub fn new(config: SearchlightConfig) -> Result<Self, Error> { //
		match config.validate() {
			Ok(_) => Ok(Searchlight {
				config
			}),
			Err(e) => Err(e)
		}
	}
}