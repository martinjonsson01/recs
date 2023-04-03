//! A simple graphics engine that provides a simple API for simple graphics.

// rustc lints
#![warn(
    let_underscore,
    nonstandard_style,
    unused,
    explicit_outlives_requirements,
    meta_variable_misuse,
    missing_debug_implementations,
    missing_docs,
    non_ascii_idents,
    noop_method_call,
    pointer_structural_match,
    trivial_casts,
    trivial_numeric_casts
)]
// clippy lints
#![warn(
    clippy::cognitive_complexity,
    clippy::dbg_macro,
    clippy::if_then_some_else_none,
    clippy::print_stdout,
    clippy::rc_mutex,
    clippy::unwrap_used,
    clippy::large_enum_variant
)]

use cgmath::{Matrix4, SquareMatrix};
use crossbeam::queue::ArrayQueue;
pub use egui;

use crate::camera::{Camera, Projection};
pub use crate::instance::Transform;
use crate::renderer::InstancesHandle;
pub use crate::renderer::ModelHandle;

mod camera;
pub mod engine;
mod instance;
mod model;
mod renderer;
mod resources;
mod texture;
pub mod time;
mod uniform;
mod window;

mod shader_locations {
    use wgpu::ShaderLocation;

    pub const VERTEX_POSITION: ShaderLocation = 0;
    pub const VERTEX_TEXTURE_COORDINATES: ShaderLocation = 1;
    pub const VERTEX_NORMAL: ShaderLocation = 2;
    pub const VERTEX_TANGENT: ShaderLocation = 3;
    pub const VERTEX_BITANGENT: ShaderLocation = 4;

    pub const FRAGMENT_MATERIAL_UNIFORM: ShaderLocation = 0;
    pub const FRAGMENT_DIFFUSE_TEXTURE: ShaderLocation = 1;
    pub const FRAGMENT_DIFFUSE_SAMPLER: ShaderLocation = 2;
    pub const FRAGMENT_NORMAL_TEXTURE: ShaderLocation = 3;
    pub const FRAGMENT_NORMAL_SAMPLER: ShaderLocation = 4;

    pub const INSTANCE_MODEL_MATRIX_COLUMN_0: ShaderLocation = 5;
    pub const INSTANCE_MODEL_MATRIX_COLUMN_1: ShaderLocation = 6;
    pub const INSTANCE_MODEL_MATRIX_COLUMN_2: ShaderLocation = 7;
    pub const INSTANCE_MODEL_MATRIX_COLUMN_3: ShaderLocation = 8;
    pub const INSTANCE_NORMAL_MATRIX_COLUMN_0: ShaderLocation = 9;
    pub const INSTANCE_NORMAL_MATRIX_COLUMN_1: ShaderLocation = 10;
    pub const INSTANCE_NORMAL_MATRIX_COLUMN_2: ShaderLocation = 11;
}

/// A representation of the [`Camera`] that can be sent into shaders through a uniform buffer.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    /// The world-space location of the camera.
    view_position: [f32; 4],
    /// Transformation matrix that transforms from world space to view space to clip space.
    view_projection: [[f32; 4]; 4],
}

impl CameraUniform {
    pub fn new() -> Self {
        Self {
            view_position: [0.0; 4],
            view_projection: Matrix4::identity().into(),
        }
    }

    pub fn update_view_projection(&mut self, camera: &Camera, projection: &Projection) {
        self.view_position = camera.position.to_homogeneous().into();
        self.view_projection = (projection.perspective_matrix() * camera.view_matrix()).into();
    }
}

/// A buffer that contains simulation results that need to be rendered.
pub type SimulationBuffer<T> = ArrayQueue<T>;

/// A graphical object that can be rendered.
#[derive(Debug, Copy, Clone)]
pub struct Object {
    /// The orientation and location.
    pub transform: Transform,
    /// Which visual representation to use.
    pub model: ModelHandle,
    /// Which group of instances it belongs to.
    instances_group: InstancesHandle,
}

/// Whether an event should continue to propagate, or be consumed.
#[derive(Eq, PartialEq)]
pub(crate) enum EventPropagation {
    /// Ends the propagation, the event is seen as handled.
    Consume,
    /// Continues the propagation, the event is not seen as handled.
    Propagate,
}
