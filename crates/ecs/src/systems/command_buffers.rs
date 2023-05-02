//! System command buffers are a way for systems to modify entities
//! without introducing race conditions.

use super::*;
use crate::archetypes::{ComponentVec, ComponentVecImpl};
use crate::{ApplicationRunner, ArchetypeIndex, BasicApplicationError};
use std::any::{Any, TypeId};
use std::iter;

/// A way to send [`EntityCommand`]s.
pub type CommandBuffer = Sender<EntityCommand>;

/// A way to receive [`EntityCommand`]s.
pub type CommandReceiver = Receiver<EntityCommand>;

/// Allows a system to make changes to entities in the [`World`].
///
/// Changes are not applied immediately, they are collected and executed later
/// during the tick.
///
/// # Examples
/// ```no_run
/// # use ecs::Entity;
/// use ecs::systems::command_buffers::Commands;
/// # use ecs::systems::Read;
/// # #[derive(Debug)]
/// # struct Health(f32);
///
/// fn death_system(entity: Entity, health: Read<Health>, commands: Commands) {
///     if health.0 <= 0.0 {
///         commands.remove(entity);
///     }
/// }
/// ```
#[derive(Debug)]
pub struct Commands {
    command_sender: CommandBuffer,
}

impl PartialEq for Commands {
    fn eq(&self, other: &Self) -> bool {
        self.command_sender.same_channel(&other.command_sender)
    }
}

impl Commands {
    fn from_system(system: &dyn System) -> Self {
        Self {
            command_sender: system.command_buffer(),
        }
    }

    /// Adds a command to the buffer which will remove a given [`Entity`] upon buffer playback.
    pub fn remove(&self, entity: Entity) {
        let command = EntityCommand::Remove(entity);
        self.command_sender
            .send(command)
            .expect("System command buffer should not be disconnected during system iteration");
    }

    /// Adds a command to the buffer which will add a given [`ComponentType`] to the given
    /// [`Entity`] upon buffer playback.
    pub fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &self,
        entity: Entity,
        new_component: ComponentType,
    ) {
        let addition = ComponentAddition::new(entity, new_component);
        let command = EntityCommand::AddComponent(addition);
        self.command_sender
            .send(command)
            .expect("System command buffer should not be disconnected during system iteration");
    }

    /// Adds a command to the buffer which will remove a given [`ComponentType`] from the given
    /// [`Entity`] upon buffer playback.
    pub fn remove_component<ComponentType: Debug + Send + Sync + 'static>(&self, entity: Entity) {
        let removal = ComponentRemoval::new::<ComponentType>(entity);
        let command = EntityCommand::RemoveComponent(removal);
        self.command_sender
            .send(command)
            .expect("System command buffer should not be disconnected during system iteration");
    }
}

/// An action on an entity.
#[derive(Debug)]
pub enum EntityCommand {
    /// Removes the given [`Entity`], if it still exists.
    Remove(Entity),
    /// Adds the given component to the given [`Entity`], if it still exists.
    AddComponent(ComponentAddition),
    /// Removes the given component from the given [`Entity`], if it still exists.
    RemoveComponent(ComponentRemoval),
}

impl EntityCommand {
    fn target_entity(&self) -> Entity {
        match self {
            EntityCommand::Remove(entity)
            | EntityCommand::AddComponent(ComponentAddition { entity, .. })
            | EntityCommand::RemoveComponent(ComponentRemoval { entity, .. }) => *entity,
        }
    }

    fn into_component_addition(self) -> Result<ComponentAddition, Self> {
        match self {
            EntityCommand::AddComponent(addition) => Ok(addition),
            _ => Err(self),
        }
    }

    fn into_component_removal(self) -> Result<ComponentRemoval, Self> {
        match self {
            EntityCommand::RemoveComponent(removal) => Ok(removal),
            _ => Err(self),
        }
    }
}

/// An addition of a specific component to a specific entity.
#[derive(Debug)]
pub struct ComponentAddition {
    pub(crate) entity: Entity,
    pub(crate) component: Box<dyn AnyComponent>,
}

impl ComponentAddition {
    fn new<Component: AnyComponent + 'static>(entity: Entity, component: Component) -> Self {
        Self {
            entity,
            component: Box::new(component),
        }
    }
}

/// A removal of a specific component type from a specific entity.
#[derive(Debug)]
pub struct ComponentRemoval {
    pub(crate) entity: Entity,
    pub(crate) component_type: TypeId,
}

impl ComponentRemoval {
    fn new<ComponentType: AnyComponent>(entity: Entity) -> Self {
        Self {
            entity,
            component_type: TypeId::of::<ComponentType>(),
        }
    }
}

/// An opaquely-stored component, which can be used to store different types of
/// components together.
pub(crate) trait AnyComponent: Any + Debug + Send + Sync + 'static {
    fn stored_type(&self) -> TypeId;
    fn stored_type_name(&self) -> &'static str;
    fn into_any(self) -> Box<dyn Any>;
    fn create_raw_component_vec(&self) -> Box<dyn ComponentVec>;
}

impl<ComponentType> AnyComponent for ComponentType
where
    ComponentType: Debug + Send + Sync + 'static,
{
    fn stored_type(&self) -> TypeId {
        TypeId::of::<ComponentType>()
    }

    fn stored_type_name(&self) -> &'static str {
        any::type_name::<ComponentType>()
    }

    fn into_any(self) -> Box<dyn Any> {
        Box::new(self)
    }

    fn create_raw_component_vec(&self) -> Box<dyn ComponentVec> {
        Box::<ComponentVecImpl<ComponentType>>::default()
    }
}

pub(crate) trait CastableComponent {
    fn new<ComponentType>(value: ComponentType) -> Self
    where
        ComponentType: Debug + Send + Sync + 'static;
    fn try_downcast_into<Target: 'static>(self) -> Option<Target>;
}

impl CastableComponent for Box<dyn AnyComponent> {
    fn new<ComponentType>(value: ComponentType) -> Self
    where
        ComponentType: Debug + Send + Sync + 'static,
    {
        Box::new(value)
    }

    fn try_downcast_into<Target: 'static>(self) -> Option<Target> {
        let target_box = self.into_any().downcast().ok()?;
        *target_box
    }
}

impl SystemParameter for Commands {
    type BorrowedData<'components> = &'components dyn System;

    fn borrow<'world>(
        _world: &'world World,
        _archetypes: &[ArchetypeIndex],
        system: &'world dyn System,
    ) -> SystemParameterResult<Self::BorrowedData<'world>> {
        Ok(system)
    }

    unsafe fn fetch_parameter(system: &mut Self::BorrowedData<'_>) -> Option<Self> {
        Some(Commands::from_system(*system))
    }

    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![]
    }

    fn iterates_over_entities() -> bool {
        false
    }

    fn base_signature() -> Option<TypeId> {
        None
    }
}

trait CommandPlayer {
    /// The type of errors returned by the object.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Plays back all commands recorded since the last playback.
    fn playback_commands(&mut self) -> Result<(), Self::Error> {
        let mut commands = self.receive_all_commands();

        let remove_commands =
            commands.drain_filter(|command| matches!(command, EntityCommand::Remove(_)));
        self.playback_removes(remove_commands)?;

        let add_component_commands =
            commands.drain_filter(|command| matches!(command, EntityCommand::AddComponent(_)));
        self.playback_add_components(add_component_commands)?;

        let remove_component_commands =
            commands.drain_filter(|command| matches!(command, EntityCommand::RemoveComponent(_)));
        self.playback_remove_components(remove_component_commands)?;

        if !commands.is_empty() {
            panic!(
                "A new type of command has been added but the code for handling it has not.\
                The remaining commands are: {commands:?}"
            );
        }

        Ok(())
    }

    /// Receives all commands recorded since the last playback.
    fn receive_all_commands(&mut self) -> Vec<EntityCommand>;

    /// Executes all remove-operations recorded since last playback.
    fn playback_removes(
        &mut self,
        remove_commands: impl Iterator<Item = EntityCommand>,
    ) -> Result<(), Self::Error>;

    /// Executes all add-component-operations recorded since last playback.
    fn playback_add_components(
        &mut self,
        add_component_commands: impl Iterator<Item = EntityCommand>,
    ) -> Result<(), Self::Error>;

    /// Executes all remove-component-operations recorded since last playback.
    fn playback_remove_components(
        &mut self,
        remove_component_commands: impl Iterator<Item = EntityCommand>,
    ) -> Result<(), Self::Error>;
}

impl<Executor, Schedule> CommandPlayer for ApplicationRunner<Executor, Schedule> {
    type Error = BasicApplicationError;

    fn receive_all_commands(&mut self) -> Vec<EntityCommand> {
        self.command_receivers
            .iter()
            .flat_map(|receiver| {
                iter::repeat_with(|| receiver.try_recv()).take_while(Result::is_ok)
            })
            .filter_map(Result::ok)
            .collect()
    }

    fn playback_removes(
        &mut self,
        remove_commands: impl Iterator<Item = EntityCommand>,
    ) -> Result<(), Self::Error> {
        let entities = remove_commands.map(|command| command.target_entity());
        self.world
            .delete_entities(entities)
            .map_err(BasicApplicationError::World)
    }

    fn playback_add_components(
        &mut self,
        add_component_commands: impl Iterator<Item = EntityCommand>,
    ) -> Result<(), Self::Error> {
        let additions =
            add_component_commands.filter_map(|command| command.into_component_addition().ok());
        self.world
            .add_components_to_entities(additions)
            .map_err(BasicApplicationError::World)
    }

    fn playback_remove_components(
        &mut self,
        remove_component_commands: impl Iterator<Item = EntityCommand>,
    ) -> Result<(), Self::Error> {
        let removals =
            remove_component_commands.filter_map(|command| command.into_component_removal().ok());
        self.world
            .remove_component_types_from_entities(removals)
            .map_err(BasicApplicationError::World)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Application, ApplicationBuilder, BasicApplication, BasicApplicationBuilder, IntoTickable,
        Sequential, Tickable, Unordered,
    };
    use itertools::Itertools;
    use test_utils::{A, D, E, F};

    #[test]
    fn command_buffer_is_unique_per_system() {
        let system0 = (|| {}).into_system();
        let mut system0: &dyn System = &system0;
        let system1 = (|| {}).into_system();
        let mut system1: &dyn System = &system1;

        // SAFETY: buffers will be dropped at same time as borrowed data.
        let (buffer0, buffer1) = unsafe {
            let buffer0 = <Commands as SystemParameter>::fetch_parameter(&mut system0).unwrap();
            let buffer1 = <Commands as SystemParameter>::fetch_parameter(&mut system1).unwrap();
            (buffer0, buffer1)
        };

        assert_ne!(
            buffer0, buffer1,
            "different systems should have different buffers"
        )
    }

    fn set_up_app_with_system_and_entities(
        removing_system: fn(Entity, Commands),
    ) -> (BasicApplication, Entity, Entity, Entity) {
        let mut app = BasicApplicationBuilder::default()
            .add_system(removing_system)
            .build();

        // Create entities with various sets of components...
        let entity0 = app.create_entity().unwrap();
        app.add_component(entity0, D).unwrap();
        app.add_component(entity0, E).unwrap();
        app.add_component(entity0, F).unwrap();
        let entity1 = app.create_entity().unwrap();
        app.add_component(entity1, D).unwrap();
        app.add_component(entity1, E).unwrap();
        let entity2 = app.create_entity().unwrap();
        app.add_component(entity2, D).unwrap();
        (app, entity0, entity1, entity2)
    }

    // todo: test removal of already removed entities, or double-removals, etc...
    #[test]
    fn system_can_remove_entities_until_next_tick() {
        let removing_system = |entity: Entity, commands: Commands| {
            commands.remove(entity);
        };

        let (app, _, _, _) = set_up_app_with_system_and_entities(removing_system);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        assert!(
            runner.world.entities.is_empty(),
            "all entities should be removed, but these remain: {:?}",
            runner.world.entities
        )
    }

    fn read_component_values<ComponentType>(
        runner: ApplicationRunner<Sequential, Unordered>,
    ) -> Vec<ComponentType>
    where
        ComponentType: Debug + Clone + Send + Sync + 'static,
    {
        let archetypes = runner
            .world
            .get_archetype_indices(&[TypeId::of::<A>()])
            .into_iter()
            .map(|archetype_index| runner.world.get_archetype(archetype_index).unwrap());
        archetypes
            .map(|archetype| archetype.borrow_component_vec::<ComponentType>().unwrap())
            .flat_map(|component_vec| component_vec.iter().cloned().collect_vec())
            .collect()
    }

    // todo: test addition of already added components to entities, or double-adds, etc...
    #[test]
    fn system_can_add_components_to_entities_until_next_tick() {
        let adding_system = |entity: Entity, commands: Commands| {
            commands.add_component(entity, A(entity.id as i32));
        };

        let (app, entity0, entity1, entity2) = set_up_app_with_system_and_entities(adding_system);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        let component_values: Vec<_> = read_component_values::<A>(runner)
            .iter()
            .map(|value| value.0 as u32)
            .collect();

        assert_eq!(
            itertools::sorted(vec![entity0.id, entity1.id, entity2.id]).collect_vec(),
            itertools::sorted(component_values).collect_vec(),
            "newly added component values should contain entity ids"
        );
    }

    // todo: test removal of already removed components to entities, or double-removes, etc...
    #[test]
    fn system_can_remove_components_from_entities_until_next_tick() {
        let removal_system = |entity: Entity, commands: Commands| {
            commands.remove_component::<D>(entity);
        };

        let (app, _, _, _) = set_up_app_with_system_and_entities(removal_system);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        let component_values: Vec<_> = read_component_values::<D>(runner);

        assert_eq!(
            component_values.len(),
            0,
            "all components of type D should have been removed"
        );
    }
}
