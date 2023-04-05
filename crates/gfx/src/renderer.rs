use std::any::TypeId;
use std::collections::HashMap;
use std::iter;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use cgmath::Deg;
use derivative::Derivative;
use derive_builder::Builder;
use thiserror::Error;
use tracing::info;
use winit::window::Window;

use crate::camera::{Camera, Projection};
use crate::engine::{NoUI, UIRenderer};
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
use crate::{resources, CameraUniform, PointLight, Position};

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
    #[error("can't create surface")]
    SurfaceCreation(#[source] wgpu::CreateSurfaceError),
}

pub type RendererResult<T, E = RendererError> = Result<T, E>;

/// A point-light that emits light in every direction and has no area.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct PointLightUniform {
    // Uniforms require 16-byte alignment.
    position: [f32; 3],
    is_visible: i32,
    color: [f32; 3],
    _padding2: u32,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub(crate) struct Renderer<UIFn, Data, LightData> {
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
    /// Maps each model to which group of instances they belong to.
    model_to_instance: HashMap<ModelHandle, InstancesHandle>,
    /// Used for depth-testing (z-culling) to render pixels in front of each other correctly.
    depth_texture: Texture,
    /// The models that can be rendered.
    models: Vec<Model>,
    /// The uniform-representation of the light. (currently only single light supported)
    light_uniform: Uniform<PointLightUniform>,
    /// Which model to render the light with, when light "debugging" is enabled.
    light_model: ModelHandle,
    /// How the GPU acts on lights.
    light_render_pipeline: wgpu::RenderPipeline,
    /// A render pass for the GUI from egui.
    #[derivative(Debug = "ignore")]
    egui_renderer: egui_wgpu::Renderer,
    /// Where to find the assets folder (which contains models and the like).
    pub(crate) output_directory: PathBuf,
    /// The function called every frame to render the UI.
    user_interface: PhantomData<UIFn>,
    _data: PhantomData<Data>,
    _light_data: PhantomData<LightData>,
}

/// An identifier for a specific model that has been loaded into the engine.
pub type ModelHandle = usize;

/// An identifier for a group of model instances.
pub type InstancesHandle = usize;

/// Configurable aspects of the renderer.
#[derive(Debug, Builder, Clone, PartialEq)]
pub struct RendererOptions {
    /// How far away (in camera-space along the z-axis) the far clipping plane is located.
    ///
    /// This defines the far-end of the view frustum, outside of which nothing is rendered.
    #[builder(default = "100.0")]
    pub far_clipping_plane: f32,
    /// How close (in camera-space along the z-axis) the near clipping plane is located.
    ///
    /// This defines the start of the view frustum, outside of which nothing is rendered.
    #[builder(default = "0.1")]
    pub near_clipping_plane: f32,
    /// How much the camera can see at once, in degrees.
    ///
    /// Note: this is the vertical FOV.
    #[builder(default = "Deg(70.0)")]
    pub field_of_view: Deg<f32>,
    /// Directory in which build artifacts are placed into (i.e. where `/assets` is located).
    #[builder(default = r#"env!("OUT_DIR").to_owned()"#)]
    pub output_directory: String,
}

const ASSETS_PATH: &str = "assets";

impl<UIFn, Data, LightData> Renderer<UIFn, Data, LightData> {
    pub(crate) fn load_model(&mut self, file_path: &Path) -> RendererResult<ModelHandle> {
        Self::load_model_raw(
            file_path,
            &self.device,
            &self.queue,
            &self.material_bind_group_layout,
            &mut self.models,
            self.output_directory.as_path(),
        )
    }

    /// A version of [`Renderer::load_model`] that needs all its parameters explicitly passed in.
    /// (i.e. no need for `self`.)
    fn load_model_raw(
        file_path: &Path,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        models: &mut Vec<Model>,
        output_directory: &Path,
    ) -> Result<ModelHandle, RendererError> {
        let path_buffer = output_directory.join(ASSETS_PATH).join(file_path);
        let path = path_buffer.as_path();

        let obj_model = resources::load_model(path, device, queue, layout)
            .map_err(|e| ModelLoad(e, Box::new(path.to_owned())))?;

        let index = models.len();
        models.push(obj_model);
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
        self.model_to_instance.insert(model, index);
        Ok(index)
    }

    pub(crate) fn new(window: &Window, options: RendererOptions) -> RendererResult<Self> {
        let size = window.inner_size();

        if size.width == 0 {
            return Err(RendererError::WindowWidthZero);
        }
        if size.height == 0 {
            return Err(RendererError::WindowHeightZero);
        }

        // The instance is a handle to our GPU
        let instance = wgpu::Instance::default();
        let surface = unsafe {
            instance
                .create_surface(window)
                .map_err(RendererError::SurfaceCreation)?
        };
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

        let capabilities = surface.get_capabilities(&adapter);
        let surface_format = capabilities.formats.first().ok_or_else(|| {
            RendererError::SurfaceIncompatibleWithAdapter {
                surface: format!("{surface:?}"),
                adapter: format!("{adapter:?}"),
            }
        })?;
        let present_mode = {
            if let Some(mailbox) = capabilities
                .present_modes
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
            format: *surface_format,
            width: size.width,
            height: size.height,
            present_mode,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let egui_renderer = egui_wgpu::Renderer::new(&device, *surface_format, None, 1);

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

        let projection = Projection::new(
            config.width,
            config.height,
            options.field_of_view,
            options.near_clipping_plane,
            options.far_clipping_plane,
        );
        let mut camera_uniform_data = CameraUniform::new();
        camera_uniform_data.update_view_projection(&camera, &projection);
        let camera_uniform = Uniform::builder(&device, camera_uniform_data)
            .name("Camera")
            .binding(UniformBinding(0))
            .build();

        let light_uniform_data = PointLightUniform {
            position: [5.0, 5.0, 5.0],
            is_visible: 0, // Invisible by default.
            color: [1.0, 1.0, 1.0],
            _padding2: 0,
        };
        let light_uniform = Uniform::builder(&device, light_uniform_data)
            .name("Light")
            .binding(UniformBinding(0))
            .build();

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

        let mut models = vec![];

        let light_model_file_name = Path::new("cube.obj");
        let light_model = Self::load_model_raw(
            light_model_file_name,
            &device,
            &queue,
            &material_bind_group_layout,
            &mut models,
            Path::new(&options.output_directory),
        )?;

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
            model_to_instance: HashMap::default(),
            depth_texture,
            models,
            light_uniform,
            light_render_pipeline,
            light_model,
            egui_renderer,
            output_directory: PathBuf::from(options.output_directory),
            user_interface: PhantomData,
            _data: PhantomData,
            _light_data: PhantomData,
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

impl<UI, Data, LightData> Renderer<UI, Data, LightData>
where
    UI: UIRenderer + 'static,
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

            if self.light_uniform.get_data().is_visible != 0 {
                #[cfg(debug_assertions)]
                render_pass.push_debug_group("Drawing light.");
                render_pass.set_pipeline(&self.light_render_pipeline);
                render_pass.draw_light_model(
                    &self.models[self.light_model],
                    &self.camera_uniform.bind_group,
                    &self.light_uniform.bind_group,
                );
                #[cfg(debug_assertions)]
                render_pass.pop_debug_group();
            }

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
        if TypeId::of::<UI>() != TypeId::of::<NoUI>() {
            context.begin_frame(egui_input);
            UI::render(context);
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

impl<UIFn, Data, LightData> Renderer<UIFn, Data, LightData>
where
    for<'a> Data: IntoIterator<Item = (ModelHandle, Vec<Transform>)> + Send + 'a,
    for<'a> LightData: IntoIterator<Item = (PointLight, Position)> + Send + 'a,
{
    pub(crate) fn update<CameraUpdateFn>(
        &mut self,
        time: &UpdateRate,
        objects: Data,
        lights: LightData,
        mut update_camera: CameraUpdateFn,
    ) where
        CameraUpdateFn: FnMut(&mut Camera, &UpdateRate),
    {
        self.camera_uniform.update_data(&self.queue, |camera_data| {
            // todo(#91): don't handle camera controlling in here (the CameraUpdateFn should be
            // todo(#91): a system in the ECS).
            update_camera(&mut self.camera, time);

            camera_data.update_view_projection(&self.camera, &self.projection)
        });

        objects.into_iter().for_each(
            |(model_handle, transforms): (ModelHandle, Vec<Transform>)| {
                if let Some(&instances_handle) = self.model_to_instance.get(&model_handle) {
                    if let Some(instances) = self.instances.get_mut(instances_handle) {
                        instances.update_transforms(&self.device, transforms)
                    } else {
                        panic!(
                            "A model was mapped to an instance which no longer exists.\
                        This should be impossible, but if it happens it might mean that \
                        a ModelInstances was removed without updating the hashmap."
                        )
                    }
                } else {
                    self.create_model_instances(model_handle, transforms)
                        .expect("the end user can't mess it up, handles are constructed correctly");
                }
            },
        );

        // todo(#94): support multiple lights
        if let Some((light, position)) = lights.into_iter().next() {
            self.light_uniform
                .update_data(&self.queue, |light_uniform| {
                    light_uniform.position = position.vector.into();
                    light_uniform.color = light.color.into();
                    light_uniform.is_visible = 1; // Make sure light is visible now.
                });
        }
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
