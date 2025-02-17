use crate::geometry::Point;
use ash::version::DeviceV1_0;
use ash::{vk, Device};

use anyhow::Result;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use crate::app::selection::SelectionBuffer;

use crate::vulkan::{
    draw_system::{nodes::NodeVertices, Vertex},
    GfaestusVk,
};

use super::{ComputeManager, ComputePipeline};

pub struct NodeMotion {
    translation: NodeTranslation,

    vertices: NodeVertices,

    node_count: usize,
}

impl NodeMotion {
    pub fn new(app: &GfaestusVk, node_count: usize) -> Result<Self> {
        let translation = NodeTranslation::new(app, node_count)?;

        let vertices = NodeVertices::new();

        Ok(Self {
            translation,

            vertices,

            node_count,
        })
    }

    pub fn upload_vertices(
        &mut self,
        app: &GfaestusVk,
        vertices: &[Vertex],
    ) -> Result<()> {
        self.vertices.upload_vertices(app, vertices)
    }

    pub fn copy_vertices(&self, app: &GfaestusVk, other: &NodeVertices) {
        GfaestusVk::copy_buffer(
            app.vk_context().device(),
            app.transient_command_pool,
            app.graphics_queue,
            self.vertices.buffer(),
            other.buffer(),
            vk::WHOLE_SIZE,
        )
    }
}

pub struct NodeTranslation {
    compute_pipeline: ComputePipeline,

    descriptor_set: vk::DescriptorSet,

    node_count: usize,
}

impl NodeTranslation {
    pub fn new(app: &GfaestusVk, node_count: usize) -> Result<Self> {
        let device = app.vk_context().device();

        let desc_set_layout = Self::create_descriptor_set_layout(device)?;

        let pipeline_layout = {
            use vk::ShaderStageFlags as Flags;

            let pc_range = vk::PushConstantRange::builder()
                .stage_flags(Flags::COMPUTE)
                .offset(0)
                .size(8)
                .build();

            let pc_ranges = [pc_range];

            let layouts = [desc_set_layout];

            let layout_info = vk::PipelineLayoutCreateInfo::builder()
                .set_layouts(&layouts)
                .push_constant_ranges(&pc_ranges)
                .build();

            unsafe { device.create_pipeline_layout(&layout_info, None) }
        }?;

        let compute_pipeline = ComputePipeline::new(
            device,
            desc_set_layout,
            pipeline_layout,
            crate::include_shader!("compute/node_translate.comp.spv"),
        )?;

        let descriptor_sets = {
            let layouts = vec![desc_set_layout];

            let alloc_info = vk::DescriptorSetAllocateInfo::builder()
                .descriptor_pool(compute_pipeline.descriptor_pool)
                .set_layouts(&layouts)
                .build();

            unsafe { device.allocate_descriptor_sets(&alloc_info) }
        }?;

        // let selection_buffer = SelectionBuffer::new(app, node_count)?;

        Ok(Self {
            compute_pipeline,

            descriptor_set: descriptor_sets[0],
            // selection_buffer,
            node_count,
        })
    }

    pub fn translate_nodes(
        &self,
        comp_manager: &mut ComputeManager,
        vertices: &NodeVertices,
        selection_buffer: &SelectionBuffer,
        delta: Point,
    ) -> Result<usize> {
        self.write_descriptor_set(selection_buffer, vertices);

        let fence_id = comp_manager.dispatch_with(|_device, cmd_buf| {
            self.translate_cmd(cmd_buf, delta).unwrap();
        })?;

        Ok(fence_id)
    }

    pub fn translate_cmd(
        &self,
        cmd_buf: vk::CommandBuffer,
        delta: Point,
    ) -> Result<()> {
        let device = &self.compute_pipeline.device;

        unsafe {
            device.cmd_bind_pipeline(
                cmd_buf,
                vk::PipelineBindPoint::COMPUTE,
                self.compute_pipeline.pipeline,
            )
        };

        unsafe {
            let desc_sets = [self.descriptor_set];

            let null = [];
            device.cmd_bind_descriptor_sets(
                cmd_buf,
                vk::PipelineBindPoint::COMPUTE,
                self.compute_pipeline.pipeline_layout,
                0,
                &desc_sets[0..=0],
                &null,
            );
        };

        trace!("Translating selected nodes by {}, {}", delta.x, delta.y);

        let push_constants = DeltaPushConstants::new(delta);
        let pc_bytes = push_constants.bytes();

        unsafe {
            use vk::ShaderStageFlags as Flags;
            device.cmd_push_constants(
                cmd_buf,
                self.compute_pipeline.pipeline_layout,
                Flags::COMPUTE,
                0,
                &pc_bytes,
            )
        };

        let x_group_count = {
            let div = self.node_count / 256;
            let rem = self.node_count % 256;

            let mut count = div;
            if rem > 0 {
                count += 1;
            }
            count as u32
        };

        trace!(
            "Dispatching node translation with x_group_count {}",
            x_group_count
        );

        unsafe { device.cmd_dispatch(cmd_buf, x_group_count, 1, 1) };

        Ok(())
    }

    pub fn write_descriptor_set(
        &self,
        selection_buffer: &SelectionBuffer,
        vertices: &NodeVertices,
    ) {
        let sel_buf_info = vk::DescriptorBufferInfo::builder()
            .buffer(selection_buffer.buffer)
            .offset(0)
            .range(vk::WHOLE_SIZE)
            .build();

        let sel_buf_infos = [sel_buf_info];

        let sel_write = vk::WriteDescriptorSet::builder()
            .dst_set(self.descriptor_set)
            .dst_binding(0)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&sel_buf_infos)
            .build();

        let node_buf_info = vk::DescriptorBufferInfo::builder()
            .buffer(vertices.buffer())
            .offset(0)
            .range(vk::WHOLE_SIZE)
            .build();

        let node_buf_infos = [node_buf_info];

        let node_write = vk::WriteDescriptorSet::builder()
            .dst_set(self.descriptor_set)
            .dst_binding(1)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&node_buf_infos)
            .build();

        let desc_writes = [sel_write, node_write];

        unsafe {
            self.compute_pipeline
                .device
                .update_descriptor_sets(&desc_writes, &[])
        };
    }

    fn layout_binding() -> [vk::DescriptorSetLayoutBinding; 2] {
        use vk::ShaderStageFlags as Stages;

        let selection = vk::DescriptorSetLayoutBinding::builder()
            .binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .stage_flags(Stages::COMPUTE)
            .build();

        let node_vertices = vk::DescriptorSetLayoutBinding::builder()
            .binding(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .stage_flags(Stages::COMPUTE)
            .build();

        [selection, node_vertices]
    }

    fn create_descriptor_set_layout(
        device: &Device,
    ) -> Result<vk::DescriptorSetLayout> {
        let bindings = Self::layout_binding();

        let layout_info = vk::DescriptorSetLayoutCreateInfo::builder()
            .bindings(&bindings)
            .build();

        let layout =
            unsafe { device.create_descriptor_set_layout(&layout_info, None) }?;

        Ok(layout)
    }
}

pub struct DeltaPushConstants {
    delta: Point,
}

impl DeltaPushConstants {
    #[inline]
    pub fn new(delta: Point) -> Self {
        Self { delta }
    }

    #[inline]
    pub fn bytes(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];

        {
            let mut offset = 0;

            {
                let mut add_float = |f: f32| {
                    let f_bytes = f.to_ne_bytes();
                    for i in 0..4 {
                        bytes[offset] = f_bytes[i];
                        offset += 1;
                    }
                };
                add_float(self.delta.x);
                add_float(self.delta.y);
            }
        }

        bytes
    }
}
