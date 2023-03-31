use std::fmt::Debug;

use wgpu::util::DeviceExt;

/// A value that stays the same during shader execution.
///
/// This is a uniform with its own bind group, so it should only be used when the
/// update frequency of the data is unique. Otherwise, if it is commonly updated along with
/// other data, it should be a part of a bind group containing all the commonly updated data.
#[derive(Debug)]
pub(crate) struct Uniform<TData> {
    data: TData,
    buffer: wgpu::Buffer,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
    // Need to own these strings somewhere, if they're used as labels for the wgpu data types.
    _labels: Labels,
}

#[derive(Debug)]
struct Labels {
    buffer_label: Option<String>,
    bind_group_layout_label: Option<String>,
    bind_group_label: Option<String>,
}

pub(crate) struct UniformBinding(pub wgpu::ShaderLocation);

impl<TData> Uniform<TData>
where
    TData: Debug + Copy + Clone + bytemuck::Pod + bytemuck::Zeroable,
{
    pub(crate) fn builder(device: &wgpu::Device, data: TData) -> UniformBuilder<TData> {
        UniformBuilder::new(device, data)
    }

    pub(crate) fn update_data<TUpdater>(&mut self, queue: &wgpu::Queue, mut updater: TUpdater)
    where
        TUpdater: FnMut(&mut TData),
    {
        updater(&mut self.data);
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&[self.data]));
    }
}

pub(crate) struct UniformBuilder<'device, 'name, TData> {
    device: &'device wgpu::Device,
    name: Option<&'name str>,
    data: TData,
    binding: wgpu::ShaderLocation,
    shader_visibility: wgpu::ShaderStages,
}

impl<'device, 'name, TData> UniformBuilder<'device, 'name, TData>
where
    TData: Debug + Copy + Clone + bytemuck::Pod + bytemuck::Zeroable,
{
    pub(crate) fn new(device: &wgpu::Device, data: TData) -> UniformBuilder<TData> {
        UniformBuilder {
            device,
            name: None,
            data,
            binding: 0,
            shader_visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        }
    }

    pub(crate) fn name(mut self, name: &'name str) -> Self {
        self.name = Some(name);
        self
    }

    pub(crate) fn binding(mut self, binding: UniformBinding) -> Self {
        self.binding = binding.0;
        self
    }

    pub(crate) fn build(self) -> Uniform<TData> {
        // Need to store labels here, so ownership is not given to the function itself.
        // (can't create &str referencing data owned by the function, since those will be dropped)
        let labels = Labels {
            buffer_label: self.name.map(|name| format!("{name} Buffer")),
            bind_group_layout_label: self.name.map(|name| format!("{name} Bind Group Layout")),
            bind_group_label: self.name.map(|name| format!("{name} Bind Group")),
        };
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: labels.buffer_label.as_ref().map(|label| &label[..]),
                contents: bytemuck::cast_slice(&[self.data]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: self.binding,
                        visibility: self.shader_visibility,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                    label: labels // Borrow-fuckery required since 'label' only takes &str
                        .bind_group_layout_label
                        .as_ref()
                        .map(|label| &label[..]),
                });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: self.binding,
                resource: buffer.as_entire_binding(),
            }],
            label: labels.bind_group_label.as_ref().map(|label| &label[..]),
        });
        Uniform {
            data: self.data,
            buffer,
            bind_group_layout,
            bind_group,
            _labels: labels,
        }
    }
}
