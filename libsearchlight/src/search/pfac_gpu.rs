// Paper on optimized GPGPU Aho-Corasick: https://ieeexplore.ieee.org/document/6680959
// This is a paper from 2013 that claims it's GPU-accelerated AC strategy can get up to more than 127 Gbps throughput (on an Nvidia GeForce GTX 285 -
// my RX 6950 XT, according to user benchmark, is 2041% faster)
// Optimisations noted:
// - They used a Deterministic Finite Automata with failure states
// - They constructed the STT to have 257 columns, 1 for each byte value and 1 for whether the match was a failure or success, and each row representing a state, where STT[i][j] gets the next state
// - They stored the STT in texture memory since it's optimised for 2D data
// - They stored the input data in global device memory (using a CUDA zero-copy) and then cached it in shared memory with coalesced memory accesses (global memory accesses that fall within a 128-byte range are,
//   apparently, combined together into one memory load request which reduces latency). Each thread loads more than 1 byte at once, caching into shared memory with the coalesced memory accesses
// - Shared memory is split into separate banks, and two threads accessing the same bank at the same time introduces conflicts, and as a result, latency. To avoid this, a storing scheme
//   was devised to eliminate/reduce shared memory bank conflicts
//
// Conclusions were that zero-copy transfer of input data to global memory combined with the shared memory approach (coalescing accesses and avoiding shared memory bank conflicts) were significant in
// upping the throughput from 2-2.5 Gbps to 127 Gpbs
//
// Noted by me however is that there is very little discussion of storing data from the compute kernel - i.e., storing matches
//
// Additional performance/optimization notes:
// - https://www.reddit.com/r/vulkan/comments/aln5gt/vendor_queue_family_formalities_fast_framebuffer/
// - https://gpuopen.com/wp-content/uploads/2016/03/VulkanFastPaths.pdf
// - https://codereview.stackexchange.com/questions/259775/user-implementation-of-memcpy-where-to-optimize-further
// - https://www.embedded.com/optimizing-memcpy-improves-speed/
// - https://forums.raspberrypi.com/viewtopic.php?t=319315
//
// PERF: The CUDA zero-copy transfer seems to be rather enormous in terms of optimisation - If I could get the GPU to copy to device from a host-side array that I can use as if it were a standard
//   rust array, then that might bring similar performance improvements. It's maybe possible to do this actually - Vulkano buffers allow direct access to the underlying buffer as a slice,
//   so I could perhaps use this slice as the buffer in which to store file data (read directly from storage into that buffer) and then I'd have to make sure that access is synchronised, but
//   I could maybe use it as normal

mod pfac_shaders {
	pub mod ac {
		use vulkano_shaders::shader;

		shader! {
			ty: "compute",
			path: "shaders/pfac.comp"
		}
	}
}

use std::{sync::Arc, ops::DerefMut, time::Duration, io::Write};

use log::info;
use vulkano::{instance::{Instance, InstanceCreateInfo}, device::{DeviceExtensions, QueueFlags, physical::{PhysicalDevice, PhysicalDeviceType}, Features, Device, DeviceCreateInfo, QueueCreateInfo, Queue}, VulkanLibrary, memory::{allocator::{StandardMemoryAllocator, MemoryAllocator, AllocationCreateInfo, MemoryTypeFilter, MemoryAllocatePreference, DeviceLayout}, DeviceAlignment}, buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer}, NonZeroDeviceSize, pipeline::{PipelineShaderStageCreateInfo, PipelineLayout, layout::{PipelineDescriptorSetLayoutCreateInfo, PushConstantRange, PipelineLayoutCreateFlags}, ComputePipeline, compute::ComputePipelineCreateInfo, Pipeline, PipelineBindPoint}, descriptor_set::{allocator::{StandardDescriptorSetAllocator, StandardDescriptorSetAllocatorCreateInfo}, PersistentDescriptorSet, WriteDescriptorSet, layout::{DescriptorSetLayoutCreateInfo, DescriptorSetLayoutBinding, DescriptorType}}, image::{Image, ImageCreateInfo, ImageType, ImageUsage, view::ImageView}, format::Format, command_buffer::{allocator::{StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo}, AutoCommandBufferBuilder, CommandBufferUsage, CopyBufferToImageInfo, CopyBufferInfo}, sync::{self, GpuFuture}, shader::ShaderStage};

use crate::{error::{Error, VulkanError}, utils::iter::ToChunksExact};

use super::{search_common::AcTable, SearchFuture, Match, Searcher};

pub const INPUT_BUFFER_SIZE: u64 = 1024 * 1024;
pub const OUTPUT_BUFFER_SIZE: u64 = 1024 * 1024;

pub struct PfacGpu {
	vkdev: Arc<Device>,
	vkqueue_comp: Arc<Queue>,
	vkcmd_buf_alloc: StandardCommandBufferAllocator,
	vkpipeline: Arc<ComputePipeline>,
	vkdescriptor_set: Arc<PersistentDescriptorSet>,
	input_buffer_host: Arc<Buffer>,
	input_buffer_device: Arc<Buffer>,
	output_buffer_host: Arc<Buffer>,
	output_buffer_device: Arc<Buffer>
}

impl PfacGpu {
	pub fn new(table: AcTable) -> Result<Self, Error> {
		let req_device_extensions = DeviceExtensions::default();
		let req_features = Features {
			uniform_and_storage_buffer8_bit_access: true,
			shader_int8: true,
			shader_int64: true,
			..Default::default()
		};

		// Initialise vulkan library and create vulkan instance
		let vklib = VulkanLibrary::new().map_err(Error::from)?;
		let vkins = Instance::new(vklib, InstanceCreateInfo::default()).map_err(Error::from)?;

		let (vkphys, vkqfidx_comp) = Self::select_device(&vkins, &req_device_extensions).ok_or(VulkanError::NoVulkanImplementations)?;

		info!("Using physical vulkan device: {} (type {:?})", vkphys.properties().device_name, vkphys.properties().device_type);

		let (vkdev, mut vkqueues) = Device::new(Arc::clone(&vkphys), DeviceCreateInfo {
			queue_create_infos: vec![
				QueueCreateInfo {
					queue_family_index: vkqfidx_comp,
					..Default::default()
				}
			],
			enabled_extensions: req_device_extensions,
			enabled_features: req_features,
			..Default::default()
		}).map_err(Error::from)?;

		let vkqueue_comp = vkqueues.next().ok_or(VulkanError::NoVulkanImplementations)?;

		let vkmalloc = Arc::new(StandardMemoryAllocator::new_default(Arc::clone(&vkdev)));

		let input_buffer_host = Buffer::new(
			Arc::clone(&vkmalloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::TRANSFER_SRC,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
				allocate_preference: MemoryAllocatePreference::AlwaysAllocate,
				..Default::default()
			},
			DeviceLayout::new(
				NonZeroDeviceSize::new(INPUT_BUFFER_SIZE).unwrap(),
				DeviceAlignment::new(64).unwrap()
			).unwrap()
		).map_err(Error::from)?;

		let input_buffer_device = Buffer::new(
			Arc::clone(&vkmalloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::TRANSFER_DST | BufferUsage::STORAGE_BUFFER,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
				allocate_preference: MemoryAllocatePreference::AlwaysAllocate,
				..Default::default()
			},
			DeviceLayout::new(
				NonZeroDeviceSize::new(INPUT_BUFFER_SIZE).unwrap(),
				DeviceAlignment::new(64).unwrap()
			).unwrap()
		).map_err(Error::from)?;

		let table_buffer_host = Buffer::new(
			Arc::clone(&vkmalloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::TRANSFER_SRC,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
				allocate_preference: MemoryAllocatePreference::AlwaysAllocate,
				..Default::default()
			},
			DeviceLayout::new(
				NonZeroDeviceSize::new((table.indexable_columns() * table.num_rows() * 4) as u64).unwrap(),
				DeviceAlignment::new(8).unwrap()
			).unwrap()
		).map_err(Error::from)?;

		let table_image_device = Image::new(
			Arc::clone(&vkmalloc) as Arc<dyn MemoryAllocator>,
			ImageCreateInfo {
				image_type: ImageType::Dim2d,
				format: Format::R32_UINT,
				extent: [table.indexable_columns() as u32, table.num_rows() as u32, 1],
				usage: ImageUsage::TRANSFER_DST | ImageUsage::STORAGE,
				..Default::default()
			},
			AllocationCreateInfo::default()
		).map_err(Error::from)?;

		let table_subbuffer_host = Subbuffer::new(Arc::clone(&table_buffer_host));
		{
			let mut table_subbuffer_host_wlock = table_subbuffer_host.write().unwrap();
			table_subbuffer_host_wlock.deref_mut().write(&table.encode_indexable().into_iter().flat_map(|uint| uint.to_ne_bytes()).collect::<Vec<u8>>()).unwrap();
		}

		let table_imageview_device = ImageView::new_default(Arc::clone(&table_image_device)).map_err(Error::from)?;

		let output_buffer_host = Buffer::new(
			Arc::clone(&vkmalloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_HOST | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
				allocate_preference: MemoryAllocatePreference::AlwaysAllocate,
				..Default::default()
			},
			DeviceLayout::new(
				NonZeroDeviceSize::new(OUTPUT_BUFFER_SIZE).unwrap(),
				DeviceAlignment::new(8).unwrap()
			).unwrap()
		).map_err(Error::from)?;

		let output_buffer_device = Buffer::new(
			Arc::clone(&vkmalloc) as Arc<dyn MemoryAllocator>,
			BufferCreateInfo {
				usage: BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST,
				..Default::default()
			},
			AllocationCreateInfo {
				memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
				allocate_preference: MemoryAllocatePreference::AlwaysAllocate,
				..Default::default()
			},
			DeviceLayout::new(
				NonZeroDeviceSize::new(OUTPUT_BUFFER_SIZE).unwrap(),
				DeviceAlignment::new(8).unwrap()
			).unwrap()
		).map_err(Error::from)?;

		let output_subbuffer_host = Subbuffer::new(Arc::clone(&output_buffer_host));
		{
			let mut output_subbuffer_host_wlock = output_subbuffer_host.write().unwrap();
			output_subbuffer_host_wlock.deref_mut().fill(0u8);
		}

		let pfac_shader = pfac_shaders::ac::load(Arc::clone(&vkdev)).map_err(Error::from)?
			.specialize(
				[(0, table.max_pat_len.into())].into_iter().collect()
			)
			.map_err(Error::from)?;
		let pfac_entry_point = pfac_shader.entry_point("main").unwrap();

		let pfac_pipeline = {
			let pfac_pipeline_stage = PipelineShaderStageCreateInfo::new(pfac_entry_point);

			let pfac_pipeline_layout = PipelineLayout::new(
				Arc::clone(&vkdev),
				PipelineDescriptorSetLayoutCreateInfo {
					set_layouts: vec![
						DescriptorSetLayoutCreateInfo {
							bindings: [
								(0, DescriptorSetLayoutBinding {
									stages:	ShaderStage::Compute.into(),
									descriptor_count: 1,
									..DescriptorSetLayoutBinding::descriptor_type(DescriptorType::StorageBuffer)
								}),
								(1, DescriptorSetLayoutBinding {
									stages:	ShaderStage::Compute.into(),
									descriptor_count: 1,
									..DescriptorSetLayoutBinding::descriptor_type(DescriptorType::StorageImage)
								}),
								(2, DescriptorSetLayoutBinding {
									stages:	ShaderStage::Compute.into(),
									descriptor_count: 1,
									..DescriptorSetLayoutBinding::descriptor_type(DescriptorType::StorageBuffer)
								}),
							].into(),
							..Default::default()
						}
					],
					push_constant_ranges: vec![
						PushConstantRange {
							stages: ShaderStage::Compute.into(),
							offset: 0,
							size: 16
						}
					],
					flags: PipelineLayoutCreateFlags::default()
				}.into_pipeline_layout_create_info(Arc::clone(&vkdev)).expect("Failed to create pipeline layout create info")
				// PipelineDescriptorSetLayoutCreateInfo::from_stages([&ac_pipeline_stage])
				// 	.into_pipeline_layout_create_info(Arc::clone(&vkdev))
				// 	.expect("Failed to create pipeline layout create info")
			).map_err(Error::from)?;

			ComputePipeline::new(
				Arc::clone(&vkdev),
				None,
				ComputePipelineCreateInfo::stage_layout(pfac_pipeline_stage, pfac_pipeline_layout)
			).map_err(Error::from)?
		};

		let descriptor_set = {
			let desc_set_alloc = StandardDescriptorSetAllocator::new(
				Arc::clone(&vkdev),
				StandardDescriptorSetAllocatorCreateInfo::default()
			);
			let desc_set_layout = Arc::clone(&pfac_pipeline.layout().set_layouts()[0]);
			PersistentDescriptorSet::new(
				&desc_set_alloc,
				desc_set_layout,
				[
					// Descriptors
					WriteDescriptorSet::buffer(0, Subbuffer::new(Arc::clone(&input_buffer_device))),
					WriteDescriptorSet::image_view(1, table_imageview_device),
					WriteDescriptorSet::buffer(2, Subbuffer::new(Arc::clone(&output_buffer_device)))
				],
				[]
			).map_err(Error::from)?
		};

		let cmd_buf_alloc = StandardCommandBufferAllocator::new(Arc::clone(&vkdev), StandardCommandBufferAllocatorCreateInfo::default());

		let setup_cmd_buf = {
			let mut builder = AutoCommandBufferBuilder::primary(&cmd_buf_alloc, vkqfidx_comp, CommandBufferUsage::OneTimeSubmit).map_err(Error::from)?;

			builder
				.copy_buffer_to_image(CopyBufferToImageInfo::buffer_image(Subbuffer::new(table_buffer_host), table_image_device))
				.map_err(Error::from)?
				.copy_buffer(CopyBufferInfo::buffers(Subbuffer::new(Arc::clone(&output_buffer_host)), Subbuffer::new(Arc::clone(&output_buffer_device))))
				.map_err(Error::from)?;

			builder.build().map_err(Error::from)?
		};

		sync::now(Arc::clone(&vkdev))
			.then_execute(Arc::clone(&vkqueue_comp), Arc::clone(&setup_cmd_buf))
			.map_err(Error::from)?
			.then_signal_fence_and_flush()
			.map_err(Error::from)?
			.wait(Some(Duration::from_secs(10)))
			.map_err(Error::from)?;

		Ok(PfacGpu {
			vkdev,
			vkqueue_comp,
			vkcmd_buf_alloc: cmd_buf_alloc,
			vkpipeline: pfac_pipeline,
			vkdescriptor_set: descriptor_set,
			input_buffer_host,
			input_buffer_device,
			output_buffer_host,
			output_buffer_device,
		})
	}

	// Attempts to find the best Vulkan implementation and queue family indices for compute and transfer operations, returned in that order
	fn select_device(instance: &Arc<Instance>, device_extensions: &DeviceExtensions) -> Option<(Arc<PhysicalDevice>, u32)> {
		instance.enumerate_physical_devices().expect("Cannot enumerate physical devices")
			.filter(|p| p.supported_extensions().contains(&device_extensions))
			.filter_map(|p| {
				// The Vulkan specs guarantee that a compliant implementation must provide at least one queue that supports compute operations
				p.queue_family_properties().iter().enumerate()
					.position(|(_, q)| {
						q.queue_flags.contains(QueueFlags::COMPUTE | QueueFlags::TRANSFER)
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

impl Searcher for PfacGpu {
	fn search_next(&mut self, data: &[u8], data_offset: u64) -> Result<SearchFuture, Error> {
		self.search(data, data_offset)
	}

	fn search(&mut self, data: &[u8], data_offset: u64) -> Result<SearchFuture, Error> {
		let input_subbuffer_host = Subbuffer::new(Arc::clone(&self.input_buffer_host));
		let input_bytes_written = {
			let mut input_subbuffer_host_wlock = input_subbuffer_host.write().unwrap();

			// let write_len = (INPUT_BUFFER_SIZE as usize).min(data.len());
			// input_subbuffer_host_wlock.deref_mut()[..write_len].copy_from_slice(&data[..write_len]);
			// write_len

			input_subbuffer_host_wlock.deref_mut().write(data).unwrap()
		};

		let shader_pc = pfac_shaders::ac::ExtraInfo {
			offset: data_offset,
			input_len: input_bytes_written as u32 // This should never overflow since we're using the number of bytes *written* which we have control over
		};

		let dispatch_cmd_buf = {
			let mut builder = AutoCommandBufferBuilder::primary(&self.vkcmd_buf_alloc, self.vkqueue_comp.queue_family_index(), CommandBufferUsage::OneTimeSubmit).map_err(Error::from)?;

			builder
				.copy_buffer(CopyBufferInfo::buffers(Subbuffer::new(Arc::clone(&self.input_buffer_host)), Subbuffer::new(Arc::clone(&self.input_buffer_device))))
				.map_err(Error::from)?
				.fill_buffer(Subbuffer::new(Arc::clone(&self.output_buffer_device)).reinterpret::<[u32]>(), 0)
				.map_err(Error::from)?
				.bind_pipeline_compute(Arc::clone(&self.vkpipeline))
				.map_err(Error::from)?
				.bind_descriptor_sets(
					PipelineBindPoint::Compute,
					Arc::clone(&self.vkpipeline.layout()),
					0,
					Arc::clone(&self.vkdescriptor_set)
				)
				.map_err(Error::from)?
				.push_constants(
					Arc::clone(&self.vkpipeline.layout()),
					0,
					shader_pc
				)
				.map_err(Error::from)?
				.dispatch([(INPUT_BUFFER_SIZE / 64) as u32, 1, 1])
				.map_err(Error::from)?
				.copy_buffer(CopyBufferInfo::buffers(Subbuffer::new(Arc::clone(&self.output_buffer_device)), Subbuffer::new(Arc::clone(&self.output_buffer_host))))
				.map_err(Error::from)?;

			builder.build().map_err(Error::from)?
		};

		let fence_fut = sync::now(Arc::clone(&self.vkdev))
			.then_execute(Arc::clone(&self.vkqueue_comp), dispatch_cmd_buf)
			.map_err(Error::from)?
			.then_signal_fence_and_flush()
			.map_err(Error::from)?;

		let output_buffer_host = Arc::clone(&self.output_buffer_host);

		Ok(SearchFuture::new(move || {
			fence_fut
				.wait(Some(Duration::from_secs(30)))
				.map_err(Error::from)?;

			let output_subbuffer_host = Subbuffer::new(output_buffer_host);
			//let value = &output_subbuffer_host.read().unwrap()[0..((data.len() + 4) * 2)];
			let output_subbuffer_host_rlock = output_subbuffer_host.read().unwrap();
			let results_len = u32::from_ne_bytes(output_subbuffer_host_rlock[0..4].try_into().unwrap());
			// println!("Results len: {}", results_len);
			let results: Vec<Match> = output_subbuffer_host_rlock[4..((results_len as usize * 4 * 6) + 4)]
				.chunks_exact(4)
				.map(|chunk| u32::from_ne_bytes(chunk.try_into().unwrap()))
				.to_chunks_exact(6)
				.map(|chunk| Match::new(
					((chunk[1] as u64) << 32) | chunk[0] as u64,
					((chunk[3] as u64) << 32) | chunk[2] as u64,
					((chunk[5] as u64) << 32) | chunk[4] as u64
				))
				.collect();

			Ok(results)
		}))
	}
}

#[cfg(test)]
mod test {
	use crate::{search::{match_id_hash_slice_u16, pfac_gpu::PfacGpu, search_common::AcTableBuilder, Match, Searcher}, searchlight::config::MatchString};

	#[test]
	fn test_pfac_gpu_single() {
		let buffer = [
			1, 2, 3, 8, 4,
			1, 2, 3, 1, 1,
			2, 1, 2, 3, 0,
			5, 9, 1, 2, 3
		];

		let pattern = &[1u16, 2, 3];
		let pattern_id = match_id_hash_slice_u16(pattern);

		let pfac_table = AcTableBuilder::new(true).with_pattern(pattern).build();
		let mut ac = PfacGpu::new(pfac_table).unwrap();
		let matches = ac.search(&buffer, 0).unwrap();

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
			},
			Match {
				id: pattern_id,
				start_idx: 17,
				end_idx: 19
			}
		];

		assert_eq!(matches.wait().unwrap(), expected);
	}

	#[test]
	fn test_pfac_gpu_single_match() {
		let buffer = [
			1, 2, 3, 8, 4,
			1, 2, 3, 1, 1,
			2, 1, 2, 3, 0,
			5, 9, 1, 2
		];

		let pattern = &MatchString::from("\\x01\\x02\\x03.");
		let pattern_id = match_id_hash_slice_u16(pattern);

		let pfac_table = AcTableBuilder::new(true).with_pattern(pattern).build();
		let mut ac = PfacGpu::new(pfac_table).unwrap();
		let matches = ac.search(&buffer, 0).unwrap();

		let expected = vec![
			Match {
				id: pattern_id,
				start_idx: 0,
				end_idx: 3
			},
			Match {
				id: pattern_id,
				start_idx: 5,
				end_idx: 8
			},
			Match {
				id: pattern_id,
				start_idx: 11,
				end_idx: 14
			}
		];

		assert_eq!(matches.wait().unwrap(), expected);
	}

	#[test]
	fn test_pfac_gpu_multi() {
		let buffer = [ 1, 2, 3, 4, 5, 8, 4, 1, 2, 3, 4, 5, 1, 1, 2, 1, 2, 3, 4, 5, 0, 5, 9, 1, 2 ];

		let pattern = &[ 1u16, 2, 3, 4, 5 ];
		let pattern_id = match_id_hash_slice_u16(pattern);

		let pfac_table = AcTableBuilder::new(true).with_pattern(pattern).build();
		let mut ac = PfacGpu::new(pfac_table).unwrap();
		let mut matches = ac.search(&buffer[..8], 0).unwrap().wait().unwrap();
		matches.append(&mut ac.search_next(&buffer[3..10], 3).unwrap().wait().unwrap());
		matches.append(&mut ac.search_next(&buffer[5..], 5).unwrap().wait().unwrap());

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
}