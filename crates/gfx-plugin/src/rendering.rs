//! An abstraction over things that are visible in ECS.

pub use cgmath::{Quaternion, Vector3};
pub use gfx::ModelHandle;

/// A 3D representation of the shape of an object.
#[derive(Debug)]
pub struct Model {
    _handle: ModelHandle,
}

/// A location in 3D-space.
#[derive(Debug)]
pub struct Position {
    /// The vector-representation of the position.
    pub vector: Vector3<f32>,
}

/// An orientation in 3D-space.
#[derive(Debug)]
pub struct Rotation {
    /// Rotation represented as a quaternion.
    pub rotation: Quaternion<f32>,
}

/// At which scale to render the object.
///
/// Each axis can be scaled differently to stretch the object.
#[derive(Debug)]
pub struct Scale {
    /// The vector-representation of the scale.
    pub vector: Vector3<f32>,
}
