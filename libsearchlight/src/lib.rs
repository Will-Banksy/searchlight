// TODO: Either use u64 or usize, don't use them interchangably. We probably have to stick to usize as memory maps would require that. Some fs operations require u64/i64 though (seeking)
//       Okay so we're using usize pretty much all the time in the validators, but we do need to go over everything and make sure we're only using u64 when necessary, and stick to usize
//       for everything else. We can cast safely (panicking if the value doesn't fit) with .try_into().unwrap() (maybe add wrapper .assert_into() since we use .try_into().unwrap() so much
//       lol)

// TODO: Run cargo clippy and go through and sort out the issues that picks up

pub mod search;
pub mod error;
pub mod utils;
pub mod searchlight;
pub mod validation;
pub mod classifiers;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("Target architecture is not 64-bit - This software is only supported on 64-bit platforms");