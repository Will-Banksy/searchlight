use std::sync::Arc;

use vulkano::{VulkanLibrary, instance::{Instance, InstanceCreateInfo}, Validated, device::{DeviceExtensions, QueueFlags, physical::{PhysicalDevice, PhysicalDeviceType}, Device, DeviceCreateInfo, QueueCreateInfo}};

use crate::Error;

use super::pfac_common::PfacTable;

struct PfacGpu {
}

impl PfacGpu {
	pub fn new(table: PfacTable) -> Result<Self, Error> {
		// Initialise vulkan library and create vulkan instance
		let vklib = VulkanLibrary::new().map_err(Error::from)?;
		let vkins = Instance::new(vklib, InstanceCreateInfo::default()).map_err(Error::from)?;

		// Select a vulkan implementation and queue family index using `select_compute_device`
		let (vkphys, vkqf_idx) = Self::select_compute_device(vkins, &DeviceExtensions::default(), QueueFlags::COMPUTE).ok_or(Error::NoVulkanImplementations)?;

		println!("Using physical vulkan device: {} (type {:?})", vkphys.properties().device_name, vkphys.properties().device_type);

		// Initialise a logical vulkan device and queue objects
		let (vkdev, mut vkqueues) = Device::new(vkphys, DeviceCreateInfo {
			queue_create_infos: vec![QueueCreateInfo {
				queue_family_index: vkqf_idx, ..Default::default()
			}],
			..Default::default()
		}).map_err(Error::from)?;

		// We requested one queue, and Device::new returns an interator over queues, so extract & unwrap queue
		let vkqueues = vkqueues.next().expect("No vulkan queues were found");

		Ok(PfacGpu {})
	}

	// Attempts to find the best Vulkan implementation and QueueFamily (returned as an index)
	pub fn select_compute_device(instance: Arc<Instance>, device_extensions: &DeviceExtensions, queue_flags: QueueFlags) -> Option<(Arc<PhysicalDevice>, u32)> {
		instance.enumerate_physical_devices().expect("Cannot enumerate physical devices")
			.filter(|p| p.supported_extensions().contains(&device_extensions))
			.filter_map(|p| {
				// The Vulkan specs guarantee that a compliant implementation must provide at least one queue that supports compute operations
				p.queue_family_properties().iter().enumerate()
					.position(|(_, q)| {
						q.queue_flags.contains(queue_flags)
					})
					.map(|i| (p.clone(), i as u32))
			})
			.min_by_key(|(p, _)| match p.properties().device_type { // Order by device type. Preferably we want to use a discrete gpu
				PhysicalDeviceType::DiscreteGpu => 0,
				PhysicalDeviceType::IntegratedGpu => 1,
				PhysicalDeviceType::VirtualGpu => 2,
				PhysicalDeviceType::Cpu => 3,
				PhysicalDeviceType::Other => 4,
				_ => 5
			})
	}
}

#[cfg(test)]
mod test {
    use crate::lib::search::pfac_common::PfacTableBuilder;

    use super::PfacGpu;

	#[test]
	fn test_vk_instantiate() {
		let pfac_gpu = PfacGpu::new(PfacTableBuilder::new(true).build());
	}
}