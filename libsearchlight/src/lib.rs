// TODO: Either use u64 or usize, don't use them interchangably. We probably have to stick to usize as memory maps would require that. Some fs operations require u64/i64 though (seeking)

pub mod search;
pub mod error;
pub mod utils;
pub mod searchlight;
pub mod validation;
pub mod classifiers;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("Target architecture is not 64-bit - This software is only supported on 64-bit platforms");