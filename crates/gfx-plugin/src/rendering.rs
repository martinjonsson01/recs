//! An abstraction over things that are visible in ECS.
use crate::GraphicalApplication;
pub use cgmath::{One, Quaternion, Vector3};
use ecs::systems::command_buffers::{BoxedComponent, IntoBoxedComponent};
use ecs::systems::{Query, Read};
use ecs::{Application, Entity};
use gfx::engine::RingSender;
pub use gfx::ModelHandle;
use gfx::Transform;
pub use gfx::{PointLight, Position};
use itertools::Itertools;
use std::error::Error;
use std::fmt::Debug;
use thiserror::Error;

/// A 3D representation of the shape of an object.
#[derive(Debug, Copy, Clone)]
pub struct Model {
    pub(crate) handle: ModelHandle,
}

/// An orientation in 3D-space.
#[derive(Debug, Copy, Clone)]
pub struct Rotation {
    /// Rotation represented as a quaternion.
    pub quaternion: Quaternion<f32>,
}

impl Default for Rotation {
    fn default() -> Self {
        Self {
            quaternion: Quaternion::one(),
        }
    }
}

/// At which scale to render the object.
///
/// Each axis can be scaled differently to stretch the object.
#[derive(Debug, Copy, Clone)]
pub struct Scale {
    /// The vector-representation of the scale.
    pub vector: Vector3<f32>,
}

impl Default for Scale {
    fn default() -> Self {
        Self {
            vector: Vector3::new(1.0, 1.0, 1.0),
        }
    }
}

pub(crate) type RenderQuery<'parameters> = Query<
    'parameters,
    (
        Read<'parameters, Model>,
        Read<'parameters, Position>,
        Read<'parameters, Rotation>,
        Read<'parameters, Scale>,
    ),
>;

pub(crate) type RenderData = Vec<(ModelHandle, Vec<Transform>)>;

pub(crate) fn rendering_system(mut render_data_sender: RingSender<RenderData>, query: RenderQuery) {
    let grouped_transforms: Vec<_> = query
        .into_iter()
        .map(|(model, position, rotation, scale)| {
            (
                model,
                Transform {
                    position: position.point,
                    rotation: rotation.quaternion,
                    scale: scale.vector,
                },
            )
        })
        .group_by(|(model, _)| model.handle)
        .into_iter()
        .map(|(model, group)| {
            let (_, transforms): (Vec<_>, Vec<_>) = group.unzip();
            (model, transforms)
        })
        .collect();

    render_data_sender
        .send(grouped_transforms)
        .expect("this system should have stopped running when the receiver is dropped");
}

pub(crate) type LightQuery<'parameters> =
    Query<'parameters, (Read<'parameters, PointLight>, Read<'parameters, Position>)>;

pub(crate) type LightData = Vec<(PointLight, Position)>;

pub(crate) fn light_system(mut light_data_sender: RingSender<LightData>, query: LightQuery) {
    let lights_and_position: Vec<_> = query
        .into_iter()
        .map(|(light, position)| (*light, *position))
        .collect();

    light_data_sender
        .send(lights_and_position)
        .expect("this system should have stopped running when the receiver is dropped");
}

/// An error in construction of a rendered entity.
#[derive(Error, Debug)]
pub enum RenderedEntityBuilderError {
    /// Failed to create a new entity.
    #[error("failed to create a new entity")]
    EntityCreation(#[source] Box<dyn Error + Send + Sync>),
    /// Failed to add component to entity.
    #[error("failed to add component to entity")]
    ComponentAdding(#[source] Box<dyn Error + Send + Sync>),
}

/// Whether an operation on the rendered entity builder succeeded.
pub type RenderedEntityBuilderResult<T, E = RenderedEntityBuilderError> = Result<T, E>;

/// Builds a visible (i.e. rendered) entity.
#[derive(Debug)]
pub struct RenderedEntityBuilder<'app, App> {
    application: &'app mut App,
    components: Vec<BoxedComponent>,
    model: Model,
    position: Option<Position>,
    rotation: Option<Rotation>,
    scale: Option<Scale>,
}

impl<InnerApp: Application> GraphicalApplication<InnerApp> {
    /// Constructs a new [`RenderedEntityBuilder`] which can be used to build entities which
    /// will be visible in the application window.
    pub fn rendered_entity_builder(
        &mut self,
        model: Model,
    ) -> RenderedEntityBuilderResult<RenderedEntityBuilder<InnerApp>> {
        RenderedEntityBuilder::new(&mut self.application, model)
    }
}

impl<'app, App: Application> RenderedEntityBuilder<'app, App> {
    /// Creates a new instance of the builder.
    ///
    /// Note: the entity is created immediately in this call.
    pub fn new(application: &'app mut App, model: Model) -> RenderedEntityBuilderResult<Self> {
        Ok(RenderedEntityBuilder {
            application,
            components: vec![],
            model,
            position: None,
            rotation: None,
            scale: None,
        })
    }

    /// Sets the position of the entity.
    pub fn with_position(mut self, position: Position) -> Self {
        self.position = Some(position);
        self
    }

    /// Sets the rotation of the entity.
    pub fn with_rotation(mut self, rotation: Rotation) -> Self {
        self.rotation = Some(rotation);
        self
    }

    /// Sets the scale of the entity.
    pub fn with_scale(mut self, scale: Scale) -> Self {
        self.scale = Some(scale);
        self
    }

    /// Finishes the building of the rendered entity and returns it, if successful.
    pub fn build(self) -> RenderedEntityBuilderResult<Entity> {
        let RenderedEntityBuilder {
            application,
            mut components,
            model,
            position,
            rotation,
            scale,
        } = self;

        components.push(model.into_box());

        add_component_or_default(position, &mut components);
        add_component_or_default(rotation, &mut components);
        add_component_or_default(scale, &mut components);

        let entity = application
            .create_entity(components)
            .map_err(to_creation_error)?;

        Ok(entity)
    }
}

fn add_component_or_default<Component>(
    maybe_component: Option<Component>,
    components: &mut Vec<BoxedComponent>,
) where
    Component: Default + Debug + Send + Sync + 'static,
{
    let component = if let Some(component) = maybe_component {
        component
    } else {
        Component::default()
    };
    components.push(component.into_box());
}

fn to_creation_error<E: Error + Send + Sync + 'static>(error: E) -> RenderedEntityBuilderError {
    RenderedEntityBuilderError::EntityCreation(Box::new(error))
}
