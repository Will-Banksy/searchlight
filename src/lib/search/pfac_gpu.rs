mod pfac_shader {
	use vulkano_shaders::shader;

	shader! {
		ty: "compute",
		path: "shaders/pfac.comp"
	}
}

use std::{sync::Arc, ops::DerefMut, io::Write, time::Duration};

use vulkano::{VulkanLibrary, instance::{Instance, InstanceCreateInfo}, device::{DeviceExtensions, QueueFlags, physical::{PhysicalDevice, PhysicalDeviceType}, Device, DeviceCreateInfo, QueueCreateInfo, Features, Queue}, memory::{allocator::{StandardMemoryAllocator, AllocationCreateInfo, MemoryTypeFilter, MemoryAllocator, DeviceLayout}, DeviceAlignment}, buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer}, NonZeroDeviceSize, command_buffer::{allocator::{StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo}, AutoCommandBufferBuilder, CommandBufferUsage, CopyBufferInfo, PrimaryAutoCommandBuffer}, pipeline::{PipelineShaderStageCreateInfo, PipelineLayout, layout::PipelineDescriptorSetLayoutCreateInfo, ComputePipeline, compute::ComputePipelineCreateInfo, Pipeline, PipelineBindPoint}, descriptor_set::{allocator::{StandardDescriptorSetAllocator, StandardDescriptorSetAllocatorCreateInfo}, PersistentDescriptorSet, WriteDescriptorSet}, sync::{self, GpuFuture}};

use crate::lib::{error::{Error, VulkanError}, utils::chunk_iter::ToChunksExact};

use super::{pfac_common::PfacTable, Match};

const UPLOAD_BUFFER_SIZE: u64 = (1024 * 1024) + 4;
const OUTPUT_BUFFER_SIZE: u64 = 1024 * 1024;
const STATE_BUFFER_SIZE: u64 = 1024; // If changing change in shader too

pub struct PfacGpu {
	vkdev: Arc<Device>,
	vkqueue: Arc<Queue>,
	vkcmd_buf: Arc<PrimaryAutoCommandBuffer>,
	upload_buffer_host: Arc<Buffer>,
	output_buffer_host: Arc<Buffer>,
}

impl PfacGpu {
	/// Creates a new instance of PfacGpu, initialising Vulkan, and returning an Err if Vulkan was unable to be initialised
	pub fn new(table: PfacTable) -> Result<Self, Error> {
		// Initialise vulkan library and create vulkan instance
		let vklib = VulkanLibrary::new().map_err(Error::from)?;
		let vkins = Instance::new(vklib, InstanceCreateInfo::default()).map_err(Error::from)?;

		// Select a vulkan implementation and queue family index using `select_compute_device`
		let (vkphys, vkqf_idx) = Self::select_compute_device(vkins, &DeviceExtensions {
			// khr_shader_non_semantic_info: true,
			// khr_8bit_storage: true,
			..DeviceExtensions::default()
		}, QueueFlags::COMPUTE).ok_or(VulkanError::NoVulkanImplementations)?;

		println!("Using physical vulkan device: {} (type {:?})", vkphys.properties().device_name, vkphys.properties().device_type);

		// Initialise a logical vulkan device and queue objects
		let (vkdev, mut vkqueues) = Device::new(vkphys, DeviceCreateInfo {
			queue_create_infos: vec![QueueCreateInfo {
				queue_family_index: vkqf_idx, ..Default::default()
			}],
			enabled_features: Features {
				uniform_and_storage_buffer8_bit_access: true,
				shader_int64: true,
				..Default::default()
			},
			..Default::default()
		}).map_err(Error::from)?;

		// We requested one queue, and Device::new returns an interator over queues, so extract & unwrap queue
		let vkqueue = vkqueues.next().expect("No vulkan queues were found");

		let malloc = Arc::new(StandardMemoryAllocator::new_default(Arc::clone(&vkdev)));

		let upload_buffer_host = Buffer::new(
			Arc::clone(&malloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::TRANSFER_SRC,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_HOST | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
				..Default::default()
			},
			DeviceLayout::new(NonZeroDeviceSize::new(UPLOAD_BUFFER_SIZE).unwrap(), DeviceAlignment::new(8).unwrap()).unwrap()
		).map_err(Error::from)?;

		let upload_buffer_device = Buffer::new(
			Arc::clone(&malloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
				..Default::default()
			}, AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
				..Default::default()
			},
			DeviceLayout::new(NonZeroDeviceSize::new(UPLOAD_BUFFER_SIZE).unwrap(), DeviceAlignment::new(8).unwrap()).unwrap()
		).map_err(Error::from)?;

		let table_data: Vec<u32> = table.encode().into_iter().flat_map(|elem| [ (elem & 0xff) as u32, ((elem >> 32) & 0xff) as u32 ]).collect();
		let table_data_len = table_data.len() as u64;

		println!("[pfac_]table_data: {:?}", table_data);

		let table_buffer_host = Buffer::from_iter(
			Arc::clone(&malloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::TRANSFER_SRC,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_HOST | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
				..Default::default()
			},
			table_data
		).map_err(Error::from)?;

		let table_buffer_device = Buffer::new(
			Arc::clone(&malloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
				..Default::default()
			}, AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
				..Default::default()
			},
			DeviceLayout::new(NonZeroDeviceSize::new(table_data_len * 8).unwrap(), DeviceAlignment::new(1).unwrap()).unwrap()
		).map_err(Error::from)?;

		let output_buffer_host = Buffer::new(
			Arc::clone(&malloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_HOST | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
				..Default::default()
			},
			DeviceLayout::new(NonZeroDeviceSize::new(OUTPUT_BUFFER_SIZE).unwrap(), DeviceAlignment::new(8).unwrap()).unwrap()
		).map_err(Error::from)?;

		let output_buffer_device = Buffer::new(
			Arc::clone(&malloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
				..Default::default()
			},
			DeviceLayout::new(NonZeroDeviceSize::new(OUTPUT_BUFFER_SIZE).unwrap(), DeviceAlignment::new(8).unwrap()).unwrap()
		).map_err(Error::from)?;

		let output_subbuffer_host = Subbuffer::new(Arc::clone(&output_buffer_host));
		{
			let mut output_subbuffer_host_wlock = output_subbuffer_host.write().unwrap();
			output_subbuffer_host_wlock.deref_mut().fill(0u8);
		}

		let state_buffer_host = Buffer::new(
			Arc::clone(&malloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::TRANSFER_SRC,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_HOST | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
				..Default::default()
			},
			DeviceLayout::new(NonZeroDeviceSize::new(STATE_BUFFER_SIZE).unwrap(), DeviceAlignment::new(8).unwrap()).unwrap()
		).map_err(Error::from)?;

		let state_buffer_device = Buffer::new(
			Arc::clone(&malloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::UNIFORM_BUFFER | BufferUsage::TRANSFER_DST,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
				..Default::default()
			},
			DeviceLayout::new(NonZeroDeviceSize::new(STATE_BUFFER_SIZE).unwrap(), DeviceAlignment::new(8).unwrap()).unwrap()
		).map_err(Error::from)?;

		let state_subbuffer_host = Subbuffer::new(Arc::clone(&state_buffer_host));
		{
			let mut state_subbuffer_host_wlock = state_subbuffer_host.write().unwrap();
			state_subbuffer_host_wlock.deref_mut().fill(0u8);
		}

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
					WriteDescriptorSet::buffer(0, Subbuffer::new(Arc::clone(&upload_buffer_device))),
					WriteDescriptorSet::buffer(1, Subbuffer::new(Arc::clone(&table_buffer_device))),
					WriteDescriptorSet::buffer(2, Subbuffer::new(Arc::clone(&output_buffer_device))),
					// WriteDescriptorSet::buffer(3, Subbuffer::new(Arc::clone(&state_buffer_device)))
				],
				[]
			).map_err(Error::from)?
		};

		let cmd_buf_alloc = StandardCommandBufferAllocator::new(Arc::clone(&vkdev), StandardCommandBufferAllocatorCreateInfo::default());

		let one_time_cmd_buf = {
			let mut builder = AutoCommandBufferBuilder::primary(&cmd_buf_alloc, vkqf_idx, CommandBufferUsage::OneTimeSubmit).map_err(Error::from)?;

			builder
				.copy_buffer(CopyBufferInfo::buffers(Subbuffer::new(Arc::clone(&upload_buffer_host)), Subbuffer::new(Arc::clone(&upload_buffer_device))))
				.map_err(Error::from)?
				.copy_buffer(CopyBufferInfo::buffers(table_buffer_host, Subbuffer::new(Arc::clone(&table_buffer_device))))
				.map_err(Error::from)?
				.copy_buffer(CopyBufferInfo::buffers(Subbuffer::new(Arc::clone(&output_buffer_host)), Subbuffer::new(Arc::clone(&output_buffer_device))))
				.map_err(Error::from)?
				.copy_buffer(CopyBufferInfo::buffers(Subbuffer::new(Arc::clone(&state_buffer_host)), Subbuffer::new(Arc::clone(&state_buffer_device))))
				.map_err(Error::from)?;

			builder.build().map_err(Error::from)?
		};

		sync::now(Arc::clone(&vkdev))
			.then_execute(Arc::clone(&vkqueue), Arc::clone(&one_time_cmd_buf))
			.map_err(Error::from)?
			.then_signal_fence_and_flush()
			.map_err(Error::from)?
			.wait(Some(Duration::from_secs(10)))
			.map_err(Error::from)?;

		let cmd_buffer = {
			let mut builder = AutoCommandBufferBuilder::primary(&cmd_buf_alloc, vkqf_idx, CommandBufferUsage::MultipleSubmit).map_err(Error::from)?;

			let work_group_counts = [(UPLOAD_BUFFER_SIZE / 32) as u32, 1, 1]; // TODO: Use a 2D work group count and change the compute shader accordingly, to allow for a larger size of work groups

			builder
				.copy_buffer(CopyBufferInfo::buffers(Subbuffer::new(Arc::clone(&upload_buffer_host)), Subbuffer::new(Arc::clone(&upload_buffer_device))))
				.map_err(Error::from)?
				.copy_buffer(CopyBufferInfo::buffers(Subbuffer::new(Arc::clone(&output_buffer_host)), Subbuffer::new(Arc::clone(&output_buffer_device))))
				.map_err(Error::from)?
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
				.map_err(Error::from)?
				.copy_buffer(CopyBufferInfo::buffers(Subbuffer::new(Arc::clone(&output_buffer_device)), Subbuffer::new(Arc::clone(&output_buffer_host))))
				.map_err(Error::from)?;

			builder.build().map_err(Error::from)?
		};

		Ok(PfacGpu {
			vkdev,
			vkqueue,
			vkcmd_buf: cmd_buffer,
			upload_buffer_host,
			output_buffer_host,
		})
	}

	pub fn search_next(&mut self, data: &[u8], data_offset: u64) -> Result<Vec<Match>, Error> {
		let upload_subbuffer_host = Subbuffer::new(Arc::clone(&self.upload_buffer_host));
		{
			let mut upload_subbuffer_host_wlock = upload_subbuffer_host.write().unwrap();
			let data_len_bytes = (data.len() as u32).to_ne_bytes(); // Vulkan mandates that endianness is the same between host and device

			let to_write: Vec<u8> = data_len_bytes.into_iter().chain(data.iter().copied()).collect();

			upload_subbuffer_host_wlock.deref_mut().write(&to_write).unwrap();
		}

		let output_subbuffer_host = Subbuffer::new(Arc::clone(&self.output_buffer_host));
		{
			let mut output_subbuffer_host_wlock = output_subbuffer_host.write().unwrap();
			output_subbuffer_host_wlock.deref_mut().fill(0u8);
		}

		sync::now(Arc::clone(&self.vkdev))
			.then_execute(Arc::clone(&self.vkqueue), Arc::clone(&self.vkcmd_buf))
			.map_err(Error::from)?
			.then_signal_fence_and_flush()
			.map_err(Error::from)?
			.wait(Some(Duration::from_secs(10)))
			.map_err(Error::from)?;

		let output_subbuffer_host = Subbuffer::new(Arc::clone(&self.output_buffer_host));
		//let value = &output_subbuffer_host.read().unwrap()[0..((data.len() + 4) * 2)];
		let output_subbuffer_host_rlock = output_subbuffer_host.read().unwrap();
		let results_len = u32::from_ne_bytes(output_subbuffer_host_rlock[0..4].try_into().unwrap());
		// println!("Results len: {}", results_len);
		let results: Vec<Match> = output_subbuffer_host_rlock[4..((results_len as usize * 4 * 4) + 4)]
			.chunks_exact(4)
			.map(|chunk| u32::from_ne_bytes(chunk.try_into().unwrap()))
			.to_chunks_exact(4)
			.map(|chunk| Match::new(
				((chunk[1] as u64) << 32) | chunk[0] as u64,
				chunk[2] as u64 + data_offset,
				chunk[3] as u64 + data_offset
			))
			.collect();

		return Ok(results)
	}

	pub fn discard_progress(&mut self) -> Result<(), Error> {
		todo!() // TODO: Progress tracking for GPGPU impl of PFAC
	}

	// Attempts to find the best Vulkan implementation and QueueFamily (returned as an index)
	fn select_compute_device(instance: Arc<Instance>, device_extensions: &DeviceExtensions, queue_flags: QueueFlags) -> Option<(Arc<PhysicalDevice>, u32)> {
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
	use crate::lib::search::{Match, pfac_common::PfacTableBuilder, match_id_hash_slice, pfac_gpu::PfacGpu};

	#[test]
	fn test_pfac_gpu_single() {
		let buffer = [ 1, 2, 3, 8, 4, 1, 2, 3, 1, 1, 2, 1, 2, 3, 0, 5, 9, 1, 2 ];

		let pattern = &[1, 2, 3];
		let pattern_id = match_id_hash_slice(pattern);

		let pfac_table = PfacTableBuilder::new(true).with_pattern(pattern).build();
		let mut pfac = PfacGpu::new(pfac_table).unwrap();
		let matches = pfac.search_next(&buffer, 0).unwrap();

		let expected = vec![
			Match {
				id: pattern_id,
				start_idx: 0,
				end_idx: 2
			},
			Match {
				id: pattern_id,
				start_idx: 5,
				end_idx: 7
			},
			Match {
				id: pattern_id,
				start_idx: 11,
				end_idx: 13
			}
		];

		assert_eq!(matches, expected);
	}

	#[test]
	fn test_pfac_gpu_multi() {
		let buffer = [ 1, 2, 3, 4, 5, 8, 4, 1, 2, 3, 4, 5, 1, 1, 2, 1, 2, 3, 4, 5, 0, 5, 9, 1, 2, 0x7f, 0x45, 0x4c, 0x46, 0 ];

		let pattern = &[ 1, 2, 3, 4, 5 ];
		let pattern_id = match_id_hash_slice(pattern);

		let pfac_table = PfacTableBuilder::new(true).with_pattern(pattern).build();
		let mut pfac = PfacGpu::new(pfac_table).unwrap();
		let mut matches = pfac.search_next(&buffer[..8], 0).unwrap();
		matches.append(&mut pfac.search_next(&buffer[8..10], 8).unwrap());
		matches.append(&mut pfac.search_next(&buffer[10..], 10).unwrap());

		let expected = vec![
			Match {
				id: pattern_id,
				start_idx: 0,
				end_idx: 4
			},
			Match {
				id: pattern_id,
				start_idx: 7,
				end_idx: 11
			},
			Match {
				id: pattern_id,
				start_idx: 15,
				end_idx: 19
			}
		];

		assert_eq!(matches, expected);
	}

	#[test]
	fn test_pfac_gpu_multi_2() {
		let buffer = [ 1, 2, 3, 4, 5, 8, 4, 1, 2, 3, 4, 5, 1, 1, 2, 1, 2, 3, 4, 5, 0, 5, 9, 1, 2, 0x7f, 0x45, 0x4c, 0x46, 0 ];

		let patterns: &[&[u8]] = &[ &[ 1, 2, 3, 4, 5 ], &[ 0x7f, 0x45, 0x4c, 0x46 ] ];
		let pattern_id_0 = match_id_hash_slice(patterns[0]);
		let pattern_id_1 = match_id_hash_slice(patterns[1]);

		let pfac_table = PfacTableBuilder::new(true).with_pattern(patterns[0]).with_pattern(patterns[1]).build();
		let mut pfac = PfacGpu::new(pfac_table).unwrap();
		let mut matches = pfac.search_next(&buffer[..8], 0).unwrap();
		matches.append(&mut pfac.search_next(&buffer[8..10], 8).unwrap());
		matches.append(&mut pfac.search_next(&buffer[10..], 10).unwrap());

		let expected = vec![
			Match {
				id: pattern_id_0,
				start_idx: 0,
				end_idx: 4
			},
			Match {
				id: pattern_id_0,
				start_idx: 7,
				end_idx: 11
			},
			Match {
				id: pattern_id_0,
				start_idx: 15,
				end_idx: 19
			},
			Match {
				id: pattern_id_1,
				start_idx: 25,
				end_idx: 28
			}
		];

		matches.sort_unstable_by_key(|m| m.start_idx);

		assert_eq!(matches, expected);
	}
}