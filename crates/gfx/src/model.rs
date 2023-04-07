use std::mem::size_of;
use std::ops::Range;

use cgmath::Vector3;
use image::Rgb;
use palette::Srgb;
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use wgpu::{BindGroup, Buffer, BufferUsages};

use crate::shader_locations::*;
use crate::texture::Texture;

/// A 3D model representing a visible object.
#[derive(Debug)]
pub struct Model {
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
}

/// A description of how a [`Mesh`] looks.
#[derive(Debug)]
pub struct Material {
    pub name: String,
    pub diffuse_color: Vector3<f32>,
    pub material_buffer: Buffer,
    pub diffuse_texture: Texture,
    /// A normal map, where r, g and b map to x, y and z of the normals.
    pub normal_texture: Texture,
    pub bind_group: BindGroup,
}

impl Material {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        name: &str,
        diffuse_color: [f32; 3],
        maybe_diffuse_texture: Option<Texture>,
        normal_texture: Texture,
        layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let has_diffuse_texture = maybe_diffuse_texture.is_some();
        let diffuse_texture = match maybe_diffuse_texture {
            Some(diffuse_texture) => diffuse_texture,
            None => {
                // Can't exclude texture from bind group, so put a 1-pixel texture there.
                let blank_pixel = Rgb([255, 0, 255]);
                Texture::from_pixel(device, queue, blank_pixel, None)
                    .expect("should be able to create blank single-pixel texture in-memory")
            }
        };

        let base_color = Srgb::new(diffuse_color[0], diffuse_color[1], diffuse_color[2]);
        let linear_rgb = base_color.into_linear();
        let material_uniform = MaterialUniform::new(
            [linear_rgb.red, linear_rgb.green, linear_rgb.blue],
            has_diffuse_texture.into(),
        );
        let material_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Material Buffer"),
            contents: bytemuck::cast_slice(&[material_uniform]),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: FRAGMENT_MATERIAL_UNIFORM,
                    resource: material_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: FRAGMENT_DIFFUSE_TEXTURE,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: FRAGMENT_DIFFUSE_SAMPLER,
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: FRAGMENT_NORMAL_TEXTURE,
                    resource: wgpu::BindingResource::TextureView(&normal_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: FRAGMENT_NORMAL_SAMPLER,
                    resource: wgpu::BindingResource::Sampler(&normal_texture.sampler),
                },
            ],
            label: Some(name),
        });

        Self {
            name: String::from(name),
            diffuse_color: diffuse_color.into(),
            material_buffer,
            diffuse_texture,
            normal_texture,
            bind_group,
        }
    }
}

/// A representation of a [`Material`] that can be sent into shaders through a uniform buffer.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    diffuse_color: [f32; 3],
    has_diffuse_texture: i32,
}

impl MaterialUniform {
    pub fn new(diffuse_color: [f32; 3], has_diffuse_texture: i32) -> Self {
        Self {
            diffuse_color,
            has_diffuse_texture,
        }
    }
}

/// The geometry of an object, composed of discrete vertices.
#[derive(Debug)]
pub struct Mesh {
    pub name: String,
    pub vertex_buffer: Buffer,
    pub index_buffer: Buffer,
    /// The number of indices present.
    pub num_elements: u32,
    /// An index into the [`materials`] list in the [`Model`].
    pub material_index: usize,
}

pub trait Vertex {
    fn descriptor<'a>() -> wgpu::VertexBufferLayout<'a>;
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelVertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub normal: [f32; 3],
    pub tangent: [f32; 3],
    pub bitangent: [f32; 3],
}

impl Vertex for ModelVertex {
    fn descriptor<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<ModelVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: VERTEX_POSITION,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: VERTEX_TEXTURE_COORDINATES,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: size_of::<[f32; 5]>() as wgpu::BufferAddress,
                    shader_location: VERTEX_NORMAL,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: VERTEX_TANGENT,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: size_of::<[f32; 11]>() as wgpu::BufferAddress,
                    shader_location: VERTEX_BITANGENT,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

pub trait DrawModel<'a> {
    fn draw_mesh(
        &mut self,
        mesh: &'a Mesh,
        material: &'a Material,
        camera_bind_group: &'a BindGroup,
        light_bind_group: &'a BindGroup,
    );
    fn draw_mesh_instanced(
        &mut self,
        mesh: &'a Mesh,
        material: &'a Material,
        instances: Range<u32>,
        camera_bind_group: &'a BindGroup,
        light_bind_group: &'a BindGroup,
    );

    fn draw_model(
        &mut self,
        model: &'a Model,
        camera_bind_group: &'a BindGroup,
        light_bind_group: &'a BindGroup,
    );
    fn draw_model_instanced(
        &mut self,
        model: &'a Model,
        instances: Range<u32>,
        camera_bind_group: &'a BindGroup,
        light_bind_group: &'a BindGroup,
    );
}

impl<'a, 'b> DrawModel<'b> for wgpu::RenderPass<'a>
where
    'b: 'a,
{
    fn draw_mesh(
        &mut self,
        mesh: &'b Mesh,
        material: &'b Material,
        camera_bind_group: &'b BindGroup,
        light_bind_group: &'b BindGroup,
    ) {
        self.draw_mesh_instanced(mesh, material, 0..1, camera_bind_group, light_bind_group);
    }

    fn draw_mesh_instanced(
        &mut self,
        mesh: &'b Mesh,
        material: &'b Material,
        instances: Range<u32>,
        camera_bind_group: &'b BindGroup,
        light_bind_group: &'b BindGroup,
    ) {
        self.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        self.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        self.set_bind_group(0, &material.bind_group, &[]);
        self.set_bind_group(1, camera_bind_group, &[]);
        self.set_bind_group(2, light_bind_group, &[]);
        self.draw_indexed(0..mesh.num_elements, 0, instances);
    }

    fn draw_model(
        &mut self,
        model: &'b Model,
        camera_bind_group: &'b BindGroup,
        light_bind_group: &'b BindGroup,
    ) {
        self.draw_model_instanced(model, 0..1, camera_bind_group, light_bind_group);
    }

    fn draw_model_instanced(
        &mut self,
        model: &'b Model,
        instances: Range<u32>,
        camera_bind_group: &'b BindGroup,
        light_bind_group: &'b BindGroup,
    ) {
        for mesh in &model.meshes {
            #[cfg(debug_assertions)]
            self.push_debug_group(&format!("Drawing {0}.", mesh.name));
            let material = &model.materials[mesh.material_index];
            self.draw_mesh_instanced(
                mesh,
                material,
                instances.clone(),
                camera_bind_group,
                light_bind_group,
            );
            #[cfg(debug_assertions)]
            self.pop_debug_group();
        }
    }
}

pub trait DrawLight<'a> {
    fn draw_light_mesh(
        &mut self,
        mesh: &'a Mesh,
        camera_bind_group: &'a BindGroup,
        light_bind_group: &'a BindGroup,
    );
    fn draw_light_mesh_instanced(
        &mut self,
        mesh: &'a Mesh,
        instances: Range<u32>,
        camera_bind_group: &'a BindGroup,
        light_bind_group: &'a BindGroup,
    );

    fn draw_light_model(
        &mut self,
        model: &'a Model,
        camera_bind_group: &'a BindGroup,
        light_bind_group: &'a BindGroup,
    );
    fn draw_light_model_instanced(
        &mut self,
        model: &'a Model,
        instances: Range<u32>,
        camera_bind_group: &'a BindGroup,
        light_bind_group: &'a BindGroup,
    );
}

impl<'a, 'b> DrawLight<'b> for wgpu::RenderPass<'a>
where
    'b: 'a,
{
    fn draw_light_mesh(
        &mut self,
        mesh: &'b Mesh,
        camera_bind_group: &'b BindGroup,
        light_bind_group: &'b BindGroup,
    ) {
        self.draw_light_mesh_instanced(mesh, 0..1, camera_bind_group, light_bind_group);
    }

    fn draw_light_mesh_instanced(
        &mut self,
        mesh: &'b Mesh,
        instances: Range<u32>,
        camera_bind_group: &'b BindGroup,
        light_bind_group: &'b BindGroup,
    ) {
        self.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        self.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        self.set_bind_group(0, camera_bind_group, &[]);
        self.set_bind_group(1, light_bind_group, &[]);
        self.draw_indexed(0..mesh.num_elements, 0, instances);
    }

    fn draw_light_model(
        &mut self,
        model: &'b Model,
        camera_bind_group: &'b BindGroup,
        light_bind_group: &'b BindGroup,
    ) {
        self.draw_light_model_instanced(model, 0..1, camera_bind_group, light_bind_group);
    }
    fn draw_light_model_instanced(
        &mut self,
        model: &'b Model,
        instances: Range<u32>,
        camera_bind_group: &'b BindGroup,
        light_bind_group: &'b BindGroup,
    ) {
        for mesh in &model.meshes {
            self.draw_light_mesh_instanced(
                mesh,
                instances.clone(),
                camera_bind_group,
                light_bind_group,
            );
        }
    }
}
