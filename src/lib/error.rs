use vulkano::{VulkanError, LoadingError, ValidationError, Validated, memory::allocator::MemoryAllocatorError, buffer::AllocateBufferError, command_buffer::CommandBufferExecError};

#[derive(Debug)]
pub enum Error {
	VulkanLoadError(LoadingError),
	VulkanError(VulkanError),
	VulkanValidationError(Box<ValidationError>),
	NoVulkanImplementations,
	VulkanMallocError(MemoryAllocatorError),
	VulkanCmdExecError(CommandBufferExecError)
}

macro_rules! impl_from_for_variant {
	($variant: path, $contained_type: ty) => {
		impl From<$contained_type> for Error {
			fn from(value: $contained_type) -> Self {
				$variant(value)
			}
		}
	};
}

impl_from_for_variant!(Error::VulkanError, VulkanError);
impl_from_for_variant!(Error::VulkanLoadError, LoadingError);
impl_from_for_variant!(Error::VulkanValidationError, Box<ValidationError>);
impl_from_for_variant!(Error::VulkanMallocError, MemoryAllocatorError);
impl_from_for_variant!(Error::VulkanCmdExecError, CommandBufferExecError);

impl<T> From<Validated<T>> for Error where T: Into<Error> {
    fn from(value: Validated<T>) -> Self {
        match value { Validated::Error(e) => e.into(), Validated::ValidationError(ve) => Error::from(ve) }
    }
}

impl From<AllocateBufferError> for Error {
	fn from(value: AllocateBufferError) -> Self {
		match value {
			AllocateBufferError::AllocateMemory(mae) => Error::from(mae),
			AllocateBufferError::BindMemory(vke) => Error::from(vke),
			AllocateBufferError::CreateBuffer(vke) => Error::from(vke)
		}
	}
}