//! System command buffers are a way for systems to modify entities
//! without introducing race conditions.

use super::*;
use crate::{ApplicationRunner, ArchetypeIndex, BasicApplicationError, WorldResult};
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
    
    pub fn add_component<Component: Debug + Send + Sync + 'static>(&self, entity: Entity, new_component: Component) {
        let command = EntityCommand::AddComponent(entity, Box::new(new_component));
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
    AddComponent(Entity, Box<dyn AnyComponent + Send + Sync>),
}

pub trait AnyComponent: Debug {
    fn stored_type(&self) -> TypeId;
    fn into_any(self) -> Box<dyn Any>;
}

impl<ComponentType> AnyComponent for ComponentType
where
    ComponentType: 'static + Debug + Send + Sync,
{
    fn stored_type(&self) -> TypeId {
        TypeId::of::<ComponentType>()
    }

    fn into_any(self) -> Box<dyn Any> {
        Box::new(self)
    }
}

impl EntityCommand {
    fn target_entity(&self) -> Entity {
        let a = Box::new(1);
        let b = *a;
        println!("{a}");
        match self {
            EntityCommand::Remove(entity) => *entity,
            EntityCommand::AddComponent(_, _) => {
                todo!()
            }
        }
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
            commands.drain_filter(|command| matches!(command, EntityCommand::AddComponent(_, _)));
        
        self.playback_add_components(add_component_commands)?;

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

    fn playback_add_components(
        &mut self,
        remove_commands: impl Iterator<Item = EntityCommand>,
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

    fn playback_add_components(&mut self, remove_commands: impl Iterator<Item=EntityCommand>) -> Result<(), Self::Error> {
        for command in remove_commands {
            match command {
                EntityCommand::AddComponent(entity, component) => {
                    self.world.add_component_to_entity(entity, component).map_err(BasicApplicationError::World)?
                }
                _ => unreachable!()
            }
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Application, ApplicationBuilder, BasicApplicationBuilder, IntoTickable, Sequential,
        Tickable, Unordered,
    };
    use test_utils::{D, E, F};

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

    // todo: test removal of already removed entities, or double-removals, etc...
    #[test]
    fn system_can_remove_entities_until_next_tick() {
        let removing_system = |entity: Entity, commands: Commands| {
            commands.remove(entity);
        };

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

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        assert!(
            runner.world.entities.is_empty(),
            "all entities should be removed, but these remain: {:?}",
            runner.world.entities
        )
    }
}
