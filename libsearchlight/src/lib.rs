pub mod search;
pub mod error;
pub mod utils;
pub mod searchlight;
pub mod validation;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("Target architecture is not 64-bit - This software is only supported on 64-bit platforms");