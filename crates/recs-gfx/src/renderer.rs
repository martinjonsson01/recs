use std::iter;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use cgmath::prelude::*;
use cgmath::{Deg, Quaternion, Vector3};
use derivative::Derivative;
use itertools::Itertools;
use thiserror::Error;
use tracing::info;
use winit::window::Window;

use crate::camera::{Camera, Projection};
use crate::instance::{ModelInstances, Transform, TransformRaw};
use crate::model::{DrawLight, DrawModel, Model, ModelVertex, Vertex};
use crate::renderer::RendererError::ModelLoad;
use crate::shader_locations::{
    FRAGMENT_DIFFUSE_SAMPLER, FRAGMENT_DIFFUSE_TEXTURE, FRAGMENT_MATERIAL_UNIFORM,
    FRAGMENT_NORMAL_SAMPLER, FRAGMENT_NORMAL_TEXTURE,
};
use crate::texture::Texture;
use crate::time::UpdateRate;
use crate::uniform::{Uniform, UniformBinding};
use crate::{resources, CameraUniform, Object};

#[derive(Error, Debug)]
pub enum RendererError {
    #[error("the window width can't be 0")]
    WindowWidthZero,
    #[error("the window height can't be 0")]
    WindowHeightZero,
    #[error("an adapter that matches the requirements can not be found")]
    AdapterNotFound,
    #[error("a device that matches the requirements can not be found")]
    DeviceNotFound(#[source] wgpu::RequestDeviceError),
    #[error("the surface `{surface}` is not compatible with the available adapter `{adapter}`")]
    SurfaceIncompatibleWithAdapter { surface: String, adapter: String },
    #[error("failed to load model from path `{1}`")]
    ModelLoad(#[source] resources::LoadError, Box<PathBuf>),
    #[error("failed to get output texture")]
    MissingOutputTexture(#[source] wgpu::SurfaceError),
    #[error("model handle `{0}` is invalid")]
    InvalidModelHandle(ModelHandle),
}

pub type RendererResult<T, E = RendererError> = Result<T, E>;

/// A point-light that emits light in every direction and has no area.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct PointLightUniform {
    position: [f32; 3],
    _padding: u32,
    color: [f32; 3],
    _padding2: u32,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub(crate) struct Renderer<UIFn, Data> {
    /// The part of the [`Window`] that we draw to.
    surface: wgpu::Surface,
    /// Current GPU.
    device: wgpu::Device,
    /// Command queue to the GPU.
    queue: wgpu::Queue,
    /// Defines how the [`wgpu::Surface`] creates its underlying [`wgpu::SurfaceTexture`]s
    config: wgpu::SurfaceConfiguration,
    /// The physical size of the [`Window`]'s content area.
    pub(crate) size: winit::dpi::PhysicalSize<u32>,
    /// What color to clear the display with every frame.
    clear_color: wgpu::Color,
    /// The camera that views the scene.
    camera: Camera,
    /// A description of the viewport to project onto.
    projection: Projection,
    /// The uniform-representation of the camera.
    camera_uniform: Uniform<CameraUniform>,
    /// How the GPU acts on a set of data.
    render_pipeline: wgpu::RenderPipeline,
    /// How textures are laid out in memory.
    material_bind_group_layout: wgpu::BindGroupLayout,
    /// Model instances, to allow for one model to be shown multiple times with different transforms.
    instances: Vec<ModelInstances>,
    /// Used for depth-testing (z-culling) to render pixels in front of each other correctly.
    depth_texture: Texture,
    /// The models that can be rendered.
    models: Vec<Model>,
    /// The uniform-representation of the light. (currently only single light supported)
    light_uniform: Uniform<PointLightUniform>,
    light_model: Model,
    /// How the GPU acts on lights.
    light_render_pipeline: wgpu::RenderPipeline,
    /// A render pass for the GUI from egui.
    #[derivative(Debug = "ignore")]
    egui_renderer: egui_wgpu::Renderer,
    /// The function called every frame to render the UI.
    user_interface: Option<UIFn>,
    _data: PhantomData<Data>,
}

/// An identifier for a specific model that has been loaded into the engine.
pub type ModelHandle = usize;

/// An identifier for a group of model instances.
pub type InstancesHandle = usize;

impl<UIFn, Data> Renderer<UIFn, Data> {
    pub(crate) fn load_model(&mut self, path: &Path) -> RendererResult<ModelHandle> {
        let obj_model = resources::load_model(
            path,
            &self.device,
            &self.queue,
            &self.material_bind_group_layout,
        )
        .map_err(|e| ModelLoad(e, Box::new(path.to_owned())))?;

        let index = self.models.len();
        self.models.push(obj_model);
        Ok(index)
    }

    pub(crate) fn create_model_instances(
        &mut self,
        model: ModelHandle,
        transforms: Vec<Transform>,
    ) -> RendererResult<InstancesHandle> {
        if model >= self.models.len() {
            return Err(RendererError::InvalidModelHandle(model));
        }

        let instances = ModelInstances::new(&self.device, model, transforms);

        let index = self.instances.len();
        self.instances.push(instances);
        Ok(index)
    }

    pub(crate) fn new(window: &Window, user_interface: Option<UIFn>) -> RendererResult<Self> {
        let size = window.inner_size();

        if size.width == 0 {
            return Err(RendererError::WindowWidthZero);
        }
        if size.height == 0 {
            return Err(RendererError::WindowHeightZero);
        }

        // The instance is a handle to our GPU
        // Backends::all => Vulkan + Metal + DX12 + Browser WebGPU
        let instance = wgpu::Instance::new(wgpu::Backends::all());
        let surface = unsafe { instance.create_surface(window) };
        let adapter_options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        };

        // Block on `request_adapter` and `request_device` to not bleed `async` into the client
        // code. The benefit of async is being able to handle a lot of IO simultaneously, but this
        // is only a one-off initialization step.
        let adapter =
            pollster::block_on(async { instance.request_adapter(&adapter_options).await })
                .ok_or(RendererError::AdapterNotFound)?;

        let (device, queue) = pollster::block_on(async {
            adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        features: wgpu::Features::empty(),
                        // WebGL doesn't support all of wgpu's features, so if
                        // we're building for the web we'll have to disable some.
                        limits: if cfg!(target_arch = "wasm32") {
                            wgpu::Limits::downlevel_webgl2_defaults()
                        } else {
                            wgpu::Limits::default()
                        },
                        label: None,
                    },
                    None,
                )
                .await
        })
        .map_err(RendererError::DeviceNotFound)?;

        let surface_format = *surface
            .get_supported_formats(&adapter)
            .first()
            .ok_or_else(|| RendererError::SurfaceIncompatibleWithAdapter {
                surface: format!("{surface:?}"),
                adapter: format!("{adapter:?}"),
            })?;
        let present_mode = {
            let supported_modes = surface.get_supported_present_modes(&adapter);
            if let Some(mailbox) = supported_modes
                .iter()
                .find(|&mode| mode == &wgpu::PresentMode::Mailbox)
            {
                *mailbox // Fast VSync, but not supported everywhere
            } else {
                wgpu::PresentMode::AutoVsync // Supported everywhere
            }
        };
        info!("Using `{present_mode:?}` presentation mode");
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
        };
        surface.configure(&device, &config);

        let egui_renderer = egui_wgpu::Renderer::new(&device, surface_format, None, 1);

        let material_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    // Material properties.
                    wgpu::BindGroupLayoutEntry {
                        binding: FRAGMENT_MATERIAL_UNIFORM,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // Diffuse texture.
                    Self::create_texture_2d_layout(FRAGMENT_DIFFUSE_TEXTURE),
                    Self::create_sampler_2d_layout(FRAGMENT_DIFFUSE_SAMPLER),
                    // Normal map.
                    Self::create_texture_2d_layout(FRAGMENT_NORMAL_TEXTURE),
                    Self::create_sampler_2d_layout(FRAGMENT_NORMAL_SAMPLER),
                ],
                label: Some("material_bind_group_layout"),
            });

        let clear_color = wgpu::Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        };

        let camera = Camera::new((0.0, 5.0, 10.0), Deg(-90.0), Deg(-20.0));
        let projection = Projection::new(config.width, config.height, Deg(70.0), 0.1, 100.0);
        let mut camera_uniform_data = CameraUniform::new();
        camera_uniform_data.update_view_projection(&camera, &projection);
        let camera_uniform = Uniform::builder(&device, camera_uniform_data)
            .name("Camera")
            .binding(UniformBinding(0))
            .build();

        let light_uniform_data = PointLightUniform {
            position: [5.0, 5.0, 5.0],
            _padding: 0,
            color: [1.0, 1.0, 1.0],
            _padding2: 0,
        };
        let light_uniform = Uniform::builder(&device, light_uniform_data)
            .name("Light")
            .binding(UniformBinding(0))
            .build();

        let light_model_file_name = Path::new("cube.obj");
        let light_model = resources::load_model(
            light_model_file_name,
            &device,
            &queue,
            &material_bind_group_layout,
        )
        .map_err(|e| ModelLoad(e, Box::new(light_model_file_name.to_owned())))?;

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[
                    &material_bind_group_layout,
                    &camera_uniform.bind_group_layout,
                    &light_uniform.bind_group_layout,
                ],
                push_constant_ranges: &[],
            });
        let render_pipeline = {
            let shader = wgpu::ShaderModuleDescriptor {
                label: Some("Normal Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
            };
            create_render_pipeline(
                &device,
                &render_pipeline_layout,
                config.format,
                Some(Texture::DEPTH_FORMAT),
                &[ModelVertex::descriptor(), TransformRaw::descriptor()],
                shader,
            )
        };

        let light_render_pipeline = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Light Pipeline Layout"),
                bind_group_layouts: &[
                    &camera_uniform.bind_group_layout,
                    &light_uniform.bind_group_layout,
                ],
                push_constant_ranges: &[],
            });
            let shader = wgpu::ShaderModuleDescriptor {
                label: Some("Light Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("light.wgsl").into()),
            };
            create_render_pipeline(
                &device,
                &layout,
                config.format,
                Some(Texture::DEPTH_FORMAT),
                &[ModelVertex::descriptor()],
                shader,
            )
        };

        let depth_texture = Texture::create_depth_texture(&device, &config, "depth_texture");

        Ok(Self {
            surface,
            device,
            queue,
            size,
            config,
            clear_color,
            camera,
            projection,
            camera_uniform,
            render_pipeline,
            material_bind_group_layout,
            instances: vec![],
            depth_texture,
            models: vec![],
            light_uniform,
            light_render_pipeline,
            light_model,
            egui_renderer,
            user_interface,
            _data: PhantomData,
        })
    }

    fn create_sampler_2d_layout(binding: wgpu::ShaderLocation) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        }
    }

    fn create_texture_2d_layout(binding: wgpu::ShaderLocation) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                view_dimension: wgpu::TextureViewDimension::D2,
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
            },
            count: None,
        }
    }

    pub(crate) fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            self.depth_texture =
                Texture::create_depth_texture(&self.device, &self.config, "depth_texture");

            self.projection.resize(new_size.width, new_size.height);
        }
    }
}

impl<UIFn, Data> Renderer<UIFn, Data>
where
    UIFn: Fn(&egui::Context),
{
    pub(crate) fn render(
        &mut self,
        window: &Window,
        egui_state: &mut egui_winit::State,
        context: &mut egui::Context,
    ) -> RendererResult<()> {
        let output = self
            .surface
            .get_current_texture()
            .map_err(RendererError::MissingOutputTexture)?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let egui_input = egui_state.take_egui_input(window);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear_color),
                        store: true,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: true,
                    }),
                    stencil_ops: None,
                }),
            });

            #[cfg(debug_assertions)]
            render_pass.push_debug_group("Drawing light.");
            render_pass.set_pipeline(&self.light_render_pipeline);
            render_pass.draw_light_model(
                &self.light_model,
                &self.camera_uniform.bind_group,
                &self.light_uniform.bind_group,
            );
            #[cfg(debug_assertions)]
            render_pass.pop_debug_group();

            #[cfg(debug_assertions)]
            render_pass.push_debug_group("Drawing model instances.");
            render_pass.set_pipeline(&self.render_pipeline);
            for model_instances in &self.instances {
                render_pass.set_vertex_buffer(1, model_instances.buffer_slice());
                render_pass.draw_model_instanced(
                    &self.models[model_instances.model],
                    model_instances.instances(),
                    &self.camera_uniform.bind_group,
                    &self.light_uniform.bind_group,
                );
            }
            #[cfg(debug_assertions)]
            render_pass.pop_debug_group();
        }
        if let Some(user_interface) = &self.user_interface {
            context.begin_frame(egui_input);
            user_interface(context);
            let full_output = context.end_frame();

            let paint_jobs = context.tessellate(full_output.shapes);
            egui_state.handle_platform_output(window, context, full_output.platform_output);

            // Upload all resources for the GPU.
            let size = window.inner_size();
            let screen_descriptor = egui_wgpu::renderer::ScreenDescriptor {
                size_in_pixels: size.into(),
                pixels_per_point: window.scale_factor() as f32,
            };
            let tdelta: egui::TexturesDelta = full_output.textures_delta;
            for (texture_id, image_delta) in tdelta.set.into_iter() {
                self.egui_renderer.update_texture(
                    &self.device,
                    &self.queue,
                    texture_id,
                    &image_delta,
                );
            }
            self.egui_renderer.update_buffers(
                &self.device,
                &self.queue,
                &mut encoder,
                &paint_jobs,
                &screen_descriptor,
            );

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("GUI Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            // Record all render passes.
            self.egui_renderer
                .render(&mut render_pass, paint_jobs.as_slice(), &screen_descriptor);
        }

        self.queue.submit(iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

impl<UIFn, Data> Renderer<UIFn, Data>
where
    Data: IntoIterator<Item = Object>,
{
    pub(crate) fn update<CameraUpdateFn>(
        &mut self,
        time: &UpdateRate,
        objects: Data,
        mut update_camera: CameraUpdateFn,
    ) where
        CameraUpdateFn: FnMut(&mut Camera, &UpdateRate),
    {
        self.camera_uniform.update_data(&self.queue, |camera_data| {
            update_camera(&mut self.camera, time);
            camera_data.update_view_projection(&self.camera, &self.projection)
        });

        Itertools::group_by(objects.into_iter(), |object| object.instances_group)
            .into_iter()
            .for_each(|(instances_handle, object_instances)| {
                let transforms = object_instances.map(|object| object.transform).collect();
                if let Some(instances) = self.instances.get_mut(instances_handle) {
                    instances.update_transforms(&self.device, transforms)
                }
            });

        // Animate light rotation
        const DEGREES_PER_SECOND: f32 = 60.0;
        self.light_uniform.update_data(&self.queue, |light| {
            let old_position: Vector3<_> = light.position.into();
            let rotation = Quaternion::from_axis_angle(
                Vector3::unit_y(),
                Deg(DEGREES_PER_SECOND * time.delta_time.as_secs_f32()),
            );
            light.position = (rotation * old_position).into();
        });
    }
}

fn create_render_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: Option<wgpu::TextureFormat>,
    vertex_layouts: &[wgpu::VertexBufferLayout],
    shader: wgpu::ShaderModuleDescriptor,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(shader);

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: vertex_layouts,
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent::REPLACE,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            // Setting this to anything other than Fill requires Features::POLYGON_MODE_LINE
            // or Features::POLYGON_MODE_POINT
            polygon_mode: wgpu::PolygonMode::Fill,
            // Requires Features::DEPTH_CLIP_CONTROL
            unclipped_depth: false,
            // Requires Features::CONSERVATIVE_RASTERIZATION
            conservative: false,
        },
        depth_stencil: depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        // If the pipeline will be used with a multiview render pass, this
        // indicates how many array layers the attachments will have.
        multiview: None,
    })
}
