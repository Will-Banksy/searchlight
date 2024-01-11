use vulkano::{VulkanError, LoadingError, ValidationError, Validated};

pub mod lib {
	pub mod io;
	pub mod search;
}

pub enum Error {
	VulkanLoadError(LoadingError),
	VulkanError(VulkanError),
	VulkanValidationError(Box<ValidationError>),
	NoVulkanImplementations
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

impl From<Validated<VulkanError>> for Error {
    fn from(value: Validated<VulkanError>) -> Self {
        match value { Validated::Error(e) => Error::from(e), Validated::ValidationError(ve) => { Error::from(ve) } }
    }
}