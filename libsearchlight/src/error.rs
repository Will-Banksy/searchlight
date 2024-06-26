#[cfg(feature = "gpu")]
mod vulkan_error {
	use std::fmt::Display;

	use vulkano::{self, LoadingError, ValidationError, Validated, memory::allocator::MemoryAllocatorError, buffer::AllocateBufferError, command_buffer::CommandBufferExecError, image::AllocateImageError};

	#[derive(Debug)]
	pub enum VulkanError { // TODO: Probably use a name for this enum other than VulkanError (since it conflicts with the vulkano::VulkanError)?
		VulkanLoadError(LoadingError),
		VulkanError(vulkano::VulkanError),
		VulkanValidationError(Box<ValidationError>),
		NoVulkanImplementations,
		VulkanMallocError(MemoryAllocatorError),
		VulkanCmdExecError(CommandBufferExecError),
		VulkanAllocImageError(AllocateImageError)
	}

	impl Display for VulkanError {
		fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
			write!(f, "{}", match self {
				VulkanError::VulkanLoadError(e) => e.to_string(),
				VulkanError::VulkanError(e) => e.to_string(),
				VulkanError::VulkanValidationError(e) => e.to_string(),
				VulkanError::NoVulkanImplementations => "No appropriate vulkan implementations found on the system".to_string(),
				VulkanError::VulkanMallocError(e) => e.to_string(),
				VulkanError::VulkanCmdExecError(e) => e.to_string(),
				VulkanError::VulkanAllocImageError(e) => e.to_string(),
			})
		}
	}

	macro_rules! impl_from_for_variant {
		($variant: path, $contained_type: ty) => {
			impl From<$contained_type> for VulkanError {
				fn from(value: $contained_type) -> Self {
					$variant(value)
				}
			}
		};
	}

	impl_from_for_variant!(VulkanError::VulkanError, vulkano::VulkanError);
	impl_from_for_variant!(VulkanError::VulkanLoadError, LoadingError);
	impl_from_for_variant!(VulkanError::VulkanValidationError, Box<ValidationError>);
	impl_from_for_variant!(VulkanError::VulkanMallocError, MemoryAllocatorError);
	impl_from_for_variant!(VulkanError::VulkanCmdExecError, CommandBufferExecError);
	impl_from_for_variant!(VulkanError::VulkanAllocImageError, AllocateImageError);

	impl<T> From<Validated<T>> for VulkanError where T: Into<VulkanError> {
		fn from(value: Validated<T>) -> Self {
			match value { Validated::Error(e) => e.into(), Validated::ValidationError(ve) => VulkanError::from(ve) }
		}
	}

	impl From<AllocateBufferError> for VulkanError {
		fn from(value: AllocateBufferError) -> Self {
			match value {
				AllocateBufferError::AllocateMemory(mae) => VulkanError::from(mae),
				AllocateBufferError::BindMemory(vke) => VulkanError::from(vke),
				AllocateBufferError::CreateBuffer(vke) => VulkanError::from(vke)
			}
		}
	}
}

use std::{fmt::Display, io};

#[cfg(feature = "gpu")]
pub use self::vulkan_error::VulkanError;

macro_rules! impl_from_for_variant {
	($variant: path, $contained_type: ty) => {
		impl From<$contained_type> for Error {
			fn from(value: $contained_type) -> Self {
				$variant(value)
			}
		}
	};
}

// TODO: Error design: See https://nrc.github.io/error-docs/error-design/error-type-design.html,
//       https://blog.burntsushi.net/rust-error-handling/
//       Error handling in general needs a bit of an overhaul, as I'm not really sure what I'm doing with it, and error messages are
//       a bit of a mess. I'm not sure in many places whether I should be returning an error with the necessary information, or
//       simply logging an error message and returning a more generic error
#[derive(Debug)]
pub enum Error {
	#[cfg(feature = "gpu")]
	VulkanError(VulkanError),
	ConfigValidationError,
	IoError(io::Error),
	LogReadError(String)
}

impl Display for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", match self {
			#[cfg(feature = "gpu")]
			Error::VulkanError(e) => e.to_string(),
			Error::ConfigValidationError => "Config validation error".to_string(),
			Error::IoError(e) => e.to_string(),
			Error::LogReadError(msg) => format!("Log file format error: {msg}")
		})
	}
}

impl_from_for_variant!(Error::IoError, io::Error);

#[cfg(feature = "gpu")]
impl<T> From<T> for Error where T: Into<VulkanError> {
	fn from(value: T) -> Self {
		Error::VulkanError(value.into())
	}
}