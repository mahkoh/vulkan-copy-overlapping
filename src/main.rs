use jay_ash::Entry;
use jay_ash::vk::{
    API_VERSION_1_3, AccessFlags, ApplicationInfo, BufferCreateInfo, BufferImageCopy,
    BufferMemoryBarrier, BufferUsageFlags, ClearColorValue, CommandBufferAllocateInfo,
    CommandBufferBeginInfo, CommandBufferLevel, CommandPoolCreateFlags, CommandPoolCreateInfo,
    DependencyFlags, DeviceCreateInfo, DeviceQueueCreateInfo, DeviceSize, Extent3D, Fence, Format,
    ImageAspectFlags, ImageCopy, ImageCreateInfo, ImageLayout, ImageMemoryBarrier,
    ImageSubresourceLayers, ImageSubresourceRange, ImageTiling, ImageType, ImageUsageFlags,
    InstanceCreateInfo, MemoryAllocateInfo, MemoryMapFlags, MemoryPropertyFlags, Offset3D,
    PipelineStageFlags, QUEUE_FAMILY_IGNORED, SampleCountFlags, SharingMode, SubmitInfo,
};
use std::slice;

fn main() {
    unsafe {
        const OFFSET: usize = 1;
        let entry = Entry::load().unwrap();
        let instance = {
            let app_info = ApplicationInfo::default().api_version(API_VERSION_1_3);
            let create_info = InstanceCreateInfo::default().application_info(&app_info);
            entry.create_instance(&create_info, None).unwrap()
        };
        let phy_dev = instance.enumerate_physical_devices().unwrap()[0];
        let mut queue_families = vec![];
        {
            let queues = instance.get_physical_device_queue_family_properties(phy_dev);
            for (idx, queue) in queues.iter().enumerate() {
                let granularity = queue.min_image_transfer_granularity;
                if (granularity.width, granularity.height, granularity.depth) == (1, 1, 1) {
                    queue_families.push(idx as u32);
                }
            }
        }
        let queue_families = [queue_families[0], queue_families[1]];
        let dev = {
            let queue_create_info = [
                DeviceQueueCreateInfo::default()
                    .queue_family_index(queue_families[0])
                    .queue_priorities(&[1.0]),
                DeviceQueueCreateInfo::default()
                    .queue_family_index(queue_families[1])
                    .queue_priorities(&[1.0]),
            ];
            let create_info = DeviceCreateInfo::default().queue_create_infos(&queue_create_info);
            instance.create_device(phy_dev, &create_info, None).unwrap()
        };
        let qs = [
            dev.get_device_queue(queue_families[0], 0),
            dev.get_device_queue(queue_families[1], 0),
        ];
        let buf = {
            let create_info = BufferCreateInfo::default()
                .size(4)
                .usage(BufferUsageFlags::TRANSFER_SRC | BufferUsageFlags::TRANSFER_DST)
                .sharing_mode(SharingMode::CONCURRENT)
                .queue_family_indices(&queue_families);
            dev.create_buffer(&create_info, None).unwrap()
        };
        {
            let req = dev.get_buffer_memory_requirements(buf);
            let allocate_info = MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(req.memory_type_bits.trailing_zeros());
            let mem = dev.allocate_memory(&allocate_info, None).unwrap();
            dev.bind_buffer_memory(buf, mem, 0).unwrap();
        }
        let width = 1024;
        let height = 1024;
        let img = {
            let create_info = ImageCreateInfo::default()
                .image_type(ImageType::TYPE_2D)
                .format(Format::R8G8B8A8_UNORM)
                .extent(Extent3D {
                    width,
                    height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(SampleCountFlags::TYPE_1)
                .tiling(ImageTiling::OPTIMAL)
                .usage(
                    ImageUsageFlags::COLOR_ATTACHMENT
                        | ImageUsageFlags::TRANSFER_DST
                        | ImageUsageFlags::TRANSFER_SRC,
                )
                .sharing_mode(SharingMode::CONCURRENT)
                .queue_family_indices(&queue_families)
                .initial_layout(ImageLayout::UNDEFINED);
            dev.create_image(&create_info, None).unwrap()
        };
        {
            let req = dev.get_image_memory_requirements(img);
            let allocate_info = MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(req.memory_type_bits.trailing_zeros());
            let mem = dev.allocate_memory(&allocate_info, None).unwrap();
            dev.bind_image_memory(img, mem, 0).unwrap();
        }
        let cmd = queue_families.map(|queue_family| {
            let create_info = CommandPoolCreateInfo::default()
                .queue_family_index(queue_family)
                .flags(CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
            let pool = dev.create_command_pool(&create_info, None).unwrap();
            let allocate_info = CommandBufferAllocateInfo::default()
                .level(CommandBufferLevel::PRIMARY)
                .command_buffer_count(1)
                .command_pool(pool);
            let cmd = dev.allocate_command_buffers(&allocate_info).unwrap();
            cmd[0]
        });
        let size = (width * height * 4) as DeviceSize;
        let out = {
            let create_info = BufferCreateInfo::default()
                .size(size)
                .usage(BufferUsageFlags::TRANSFER_DST)
                .sharing_mode(SharingMode::CONCURRENT)
                .queue_family_indices(&queue_families);
            dev.create_buffer(&create_info, None).unwrap()
        };
        let mem = {
            let props = instance.get_physical_device_memory_properties(phy_dev);
            let req = dev.get_buffer_memory_requirements(out);
            let ty = 'ty: {
                for (idx, ty) in props.memory_types.iter().enumerate() {
                    if req.memory_type_bits & (1 << idx) != 0
                        && ty.property_flags.contains(
                            MemoryPropertyFlags::HOST_VISIBLE
                                | MemoryPropertyFlags::HOST_CACHED
                                | MemoryPropertyFlags::HOST_COHERENT,
                        )
                    {
                        break 'ty idx as u32;
                    }
                }
                panic!("no type")
            };
            let allocate_info = MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(ty);
            dev.allocate_memory(&allocate_info, None).unwrap()
        };
        dev.bind_buffer_memory(out, mem, 0).unwrap();
        let ptr = dev
            .map_memory(mem, 0, size, MemoryMapFlags::empty())
            .unwrap();
        let isr = ImageSubresourceRange {
            aspect_mask: ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };
        let isl = ImageSubresourceLayers {
            aspect_mask: ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        };
        for trial in 0.. {
            {
                dev.begin_command_buffer(cmd[0], &CommandBufferBeginInfo::default())
                    .unwrap();
                let img_barrier = ImageMemoryBarrier::default()
                    .dst_access_mask(AccessFlags::MEMORY_WRITE)
                    .old_layout(ImageLayout::UNDEFINED)
                    .new_layout(ImageLayout::GENERAL)
                    .src_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .image(img)
                    .subresource_range(isr);
                dev.cmd_pipeline_barrier(
                    cmd[0],
                    PipelineStageFlags::ALL_COMMANDS,
                    PipelineStageFlags::ALL_COMMANDS,
                    DependencyFlags::empty(),
                    &[],
                    &[],
                    slice::from_ref(&img_barrier),
                );
                // Clear image to transparent black
                dev.cmd_clear_color_image(
                    cmd[0],
                    img,
                    ImageLayout::GENERAL,
                    &ClearColorValue { float32: [0.0; 4] },
                    slice::from_ref(&isr),
                );
                // Clear buffer to opaque white
                dev.cmd_fill_buffer(cmd[0], buf, 0, 4, !0);
                let img_barrier = ImageMemoryBarrier::default()
                    .src_access_mask(AccessFlags::MEMORY_WRITE)
                    .dst_access_mask(AccessFlags::MEMORY_WRITE)
                    .old_layout(ImageLayout::GENERAL)
                    .new_layout(ImageLayout::GENERAL)
                    .src_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .image(img)
                    .subresource_range(isr);
                let buf_barrier = BufferMemoryBarrier::default()
                    .src_access_mask(AccessFlags::MEMORY_WRITE)
                    .dst_access_mask(AccessFlags::MEMORY_WRITE)
                    .src_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .buffer(buf)
                    .size(4);
                dev.cmd_pipeline_barrier(
                    cmd[0],
                    PipelineStageFlags::ALL_COMMANDS,
                    PipelineStageFlags::ALL_COMMANDS,
                    DependencyFlags::empty(),
                    &[],
                    slice::from_ref(&buf_barrier),
                    slice::from_ref(&img_barrier),
                );
                let region = BufferImageCopy {
                    buffer_offset: 0,
                    buffer_row_length: 1,
                    buffer_image_height: 1,
                    image_subresource: isl,
                    image_offset: Default::default(),
                    image_extent: Extent3D {
                        width: 1,
                        height: 1,
                        depth: 1,
                    },
                };
                // Copy buffer to image at 0x0
                dev.cmd_copy_buffer_to_image(
                    cmd[0],
                    buf,
                    img,
                    ImageLayout::GENERAL,
                    slice::from_ref(&region),
                );
                let img_barrier = ImageMemoryBarrier::default()
                    .src_access_mask(AccessFlags::MEMORY_WRITE)
                    .dst_access_mask(AccessFlags::MEMORY_WRITE | AccessFlags::MEMORY_READ)
                    .old_layout(ImageLayout::GENERAL)
                    .new_layout(ImageLayout::GENERAL)
                    .src_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .image(img)
                    .subresource_range(isr);
                dev.cmd_pipeline_barrier(
                    cmd[0],
                    PipelineStageFlags::ALL_COMMANDS,
                    PipelineStageFlags::ALL_COMMANDS,
                    DependencyFlags::empty(),
                    &[],
                    &[],
                    slice::from_ref(&img_barrier),
                );
                dev.end_command_buffer(cmd[0]).unwrap();
                let submit_info = SubmitInfo::default().command_buffers(slice::from_ref(&cmd[0]));
                dev.queue_submit(qs[0], slice::from_ref(&submit_info), Fence::null())
                    .unwrap();
            }
            dev.device_wait_idle().unwrap();
            for i in 0..2 {
                let begin_info = CommandBufferBeginInfo::default();
                dev.begin_command_buffer(cmd[i], &begin_info).unwrap();
                let region = ImageCopy {
                    src_subresource: isl,
                    src_offset: Offset3D { x: 0, y: 0, z: 0 },
                    dst_subresource: isl,
                    dst_offset: Offset3D {
                        x: OFFSET as i32 * (i as i32 + 1),
                        y: 0,
                        z: 0,
                    },
                    extent: Extent3D {
                        width: 1,
                        height: 1,
                        depth: 1,
                    },
                };
                // Copy image 0x0 to 1x0 on queue 1 and to 2x0 on queue 2
                dev.cmd_copy_image(
                    cmd[i],
                    img,
                    ImageLayout::GENERAL,
                    img,
                    ImageLayout::GENERAL,
                    slice::from_ref(&region),
                );
                dev.end_command_buffer(cmd[i]).unwrap();
            }
            for i in 0..2 {
                let submit_info = SubmitInfo::default().command_buffers(slice::from_ref(&cmd[i]));
                dev.queue_submit(qs[i], slice::from_ref(&submit_info), Fence::null())
                    .unwrap();
            }
            dev.device_wait_idle().unwrap();
            {
                dev.begin_command_buffer(cmd[0], &CommandBufferBeginInfo::default())
                    .unwrap();
                let region = BufferImageCopy {
                    buffer_offset: 0,
                    buffer_row_length: width,
                    buffer_image_height: height,
                    image_subresource: isl,
                    image_offset: Default::default(),
                    image_extent: Extent3D {
                        width,
                        height,
                        depth: 1,
                    },
                };
                let img_barrier = ImageMemoryBarrier::default()
                    .src_access_mask(AccessFlags::MEMORY_WRITE)
                    .dst_access_mask(AccessFlags::MEMORY_READ)
                    .old_layout(ImageLayout::GENERAL)
                    .new_layout(ImageLayout::GENERAL)
                    .src_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .image(img)
                    .subresource_range(isr);
                dev.cmd_pipeline_barrier(
                    cmd[0],
                    PipelineStageFlags::ALL_COMMANDS,
                    PipelineStageFlags::ALL_COMMANDS,
                    DependencyFlags::empty(),
                    &[],
                    &[],
                    slice::from_ref(&img_barrier),
                );
                // Copy image to host buffer
                dev.cmd_copy_image_to_buffer(
                    cmd[0],
                    img,
                    ImageLayout::GENERAL,
                    out,
                    slice::from_ref(&region),
                );
                let buf_barrier = BufferMemoryBarrier::default()
                    .src_access_mask(AccessFlags::MEMORY_WRITE)
                    .dst_access_mask(AccessFlags::HOST_READ)
                    .src_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(QUEUE_FAMILY_IGNORED)
                    .buffer(buf)
                    .size(4);
                dev.cmd_pipeline_barrier(
                    cmd[0],
                    PipelineStageFlags::ALL_COMMANDS,
                    PipelineStageFlags::HOST,
                    DependencyFlags::empty(),
                    &[],
                    slice::from_ref(&buf_barrier),
                    &[],
                );
                dev.end_command_buffer(cmd[0]).unwrap();
                let submit_info = SubmitInfo::default().command_buffers(&cmd[..1]);
                dev.queue_submit(qs[0], slice::from_ref(&submit_info), Fence::null())
                    .unwrap();
            }
            dev.device_wait_idle().unwrap();
            let ptr: &[u8] = slice::from_raw_parts(ptr.cast(), size as usize);
            const BYTE_OFFSET: usize = 4 * OFFSET;
            for i in 0..3 {
                let actual = &ptr[BYTE_OFFSET * i..][..BYTE_OFFSET];
                assert_eq!(&actual[..4], [255; 4], "trial = {trial}");
                assert_eq!(&actual[4..], [0; BYTE_OFFSET - 4], "trial = {trial}");
            }
        }
    }
}
