#[cfg(feature = "gpu")]
mod vulkan_error {
	use vulkano::{self, LoadingError, ValidationError, Validated, memory::allocator::MemoryAllocatorError, buffer::AllocateBufferError, command_buffer::CommandBufferExecError};

	#[derive(Debug)]
	pub enum VulkanError { // TODO: Probably use a name for this enum other than VulkanError (since it conflicts with the vulkano::VulkanError)?
		VulkanLoadError(LoadingError),
		VulkanError(vulkano::VulkanError),
		VulkanValidationError(Box<ValidationError>),
		NoVulkanImplementations,
		VulkanMallocError(MemoryAllocatorError),
		VulkanCmdExecError(CommandBufferExecError)
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

#[derive(Debug)]
pub enum Error {
	#[cfg(feature = "gpu")]
	VulkanError(VulkanError)
}

#[cfg(feature = "gpu")]
impl<T> From<T> for Error where T: Into<VulkanError> {
	fn from(value: T) -> Self {
		Error::VulkanError(value.into())
	}
}