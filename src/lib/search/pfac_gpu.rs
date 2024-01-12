mod pfac_shader {
    use vulkano_shaders::shader;

	shader! {
		ty: "compute",
		path: "shaders/pfac.comp"
	}
}

use std::sync::Arc;

use vulkano::{VulkanLibrary, instance::{Instance, InstanceCreateInfo}, device::{DeviceExtensions, QueueFlags, physical::{PhysicalDevice, PhysicalDeviceType}, Device, DeviceCreateInfo, QueueCreateInfo}, memory::{allocator::{StandardMemoryAllocator, AllocationCreateInfo, MemoryTypeFilter, MemoryAllocator, DeviceLayout}, DeviceAlignment}, buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer}, NonZeroDeviceSize, command_buffer::{allocator::{StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo}, AutoCommandBufferBuilder, CommandBufferUsage}, pipeline::{PipelineShaderStageCreateInfo, PipelineLayout, layout::PipelineDescriptorSetLayoutCreateInfo, ComputePipeline, compute::ComputePipelineCreateInfo, Pipeline, PipelineBindPoint}, descriptor_set::{allocator::{StandardDescriptorSetAllocator, StandardDescriptorSetAllocatorCreateInfo}, self, PersistentDescriptorSet, WriteDescriptorSet}};

use crate::lib::error::Error;

use super::{pfac_common::PfacTable, Match};

const UPLOAD_BUFFER_SIZE: u64 = 1024 * 1024 * 1024;
const OUTPUT_BUFFER_SIZE: u64 = 1024 * 1024;

struct PfacGpu {
	vkdev: Arc<Device>,
	vkqf_idx: u32,
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

		let malloc = Arc::new(StandardMemoryAllocator::new_default(Arc::clone(&vkdev)));

		let upload_buffer = Subbuffer::new(Buffer::new(
			Arc::clone(&malloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::STORAGE_BUFFER,
				..Default::default()
			}, AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_DEVICE | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
				..Default::default()
			},
			DeviceLayout::new(NonZeroDeviceSize::new(UPLOAD_BUFFER_SIZE).unwrap(), DeviceAlignment::new(64).unwrap()).expect("Unable to create device memory layout for upload buffer")
		).map_err(Error::from)?);

		let table_data = table.encode();

		let table_buffer = Buffer::from_iter(
			Arc::clone(&malloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::STORAGE_BUFFER,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_DEVICE | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
				..Default::default()
			},
			table_data
		).map_err(Error::from)?;

		let output_buffer = Subbuffer::new(Buffer::new(
			Arc::clone(&malloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::STORAGE_BUFFER,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_DEVICE | MemoryTypeFilter::HOST_RANDOM_ACCESS,
				..Default::default()
			},
			DeviceLayout::new(NonZeroDeviceSize::new(OUTPUT_BUFFER_SIZE).unwrap(), DeviceAlignment::new(64).unwrap()).expect("Unable to create device memory layout for output buffer")
		).map_err(Error::from)?);

		let shader = pfac_shader::load(Arc::clone(&vkdev)).unwrap();
		let entry_point = shader.entry_point("main").unwrap();

		let pipeline = {
			let pipeline_stage = PipelineShaderStageCreateInfo::new(entry_point);
			let pipeline_layout = PipelineLayout::new(
				Arc::clone(&vkdev),
				PipelineDescriptorSetLayoutCreateInfo::from_stages([&pipeline_stage])
					.into_pipeline_layout_create_info(Arc::clone(&vkdev))
					.expect("Failed to create pipeline layout create info")
			).expect("Failed to create pipeline layout");

			ComputePipeline::new(Arc::clone(&vkdev), None, ComputePipelineCreateInfo::stage_layout(pipeline_stage, pipeline_layout)).map_err(Error::from)?
		};

		let descriptor_set = {
			let desc_set_alloc = StandardDescriptorSetAllocator::new(Arc::clone(&vkdev), StandardDescriptorSetAllocatorCreateInfo::default());
			let descriptor_set_layout = Arc::clone(&pipeline.layout().set_layouts()[0]);
			PersistentDescriptorSet::new(
				&desc_set_alloc,
				descriptor_set_layout,
				[
					WriteDescriptorSet::buffer(0, upload_buffer),
					WriteDescriptorSet::buffer(1, table_buffer),
					WriteDescriptorSet::buffer(2, output_buffer)
				],
				[]
			).map_err(Error::from)?
		};

		let cmd_buffer = {
			let cmd_buf_alloc = StandardCommandBufferAllocator::new(Arc::clone(&vkdev), StandardCommandBufferAllocatorCreateInfo::default());

			let mut builder = AutoCommandBufferBuilder::primary(&cmd_buf_alloc, vkqf_idx, CommandBufferUsage::MultipleSubmit).map_err(Error::from)?;

			let work_group_counts = [(UPLOAD_BUFFER_SIZE / 32) as u32, 1, 1];

			builder
				.bind_pipeline_compute(Arc::clone(&pipeline))
				.map_err(Error::from)?
				.bind_descriptor_sets(
					PipelineBindPoint::Compute,
					Arc::clone(&pipeline.layout()),
					0,
					descriptor_set
				)
				.map_err(Error::from)?
				.dispatch(work_group_counts)
				.map_err(Error::from)?;

			builder.build().map_err(Error::from)?
		};

		// TODO: write data to input, execute command buffer, read output, check it works. Edit shader to test different things

		Ok(PfacGpu {
			vkdev,
			vkqf_idx
		})
	}

	pub fn search_next(&mut self, data: &[u8], data_offset: u64) -> Result<Vec<Match>, Error> {
		todo!()
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
					.map(|i| (Arc::clone(&p), i as u32))
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