//! System command buffers are a way for systems to modify entities
//! without introducing race conditions.

use super::*;
use crate::archetypes::{ComponentVec, ComponentVecImpl};
use crate::{ApplicationRunner, ArchetypeIndex, BasicApplicationError};
use fnv::FnvHashSet;
use itertools::Itertools;
use std::any::{Any, TypeId};
use std::iter;
use std::iter::Map;
use tracing::{debug, instrument};

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
///
/// ```no_run
/// # use ecs::Entity;
/// # use ecs::filter::Without;
/// use ecs::systems::command_buffers::Commands;
/// # use ecs::systems::Read;
/// # #[derive(Debug)]
/// # struct Tag;
///
/// fn tagging_system(entity: Entity, _: Without<Tag>, commands: Commands) {
///     commands.add_component(entity, Tag);
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
    pub fn create<ComponentIter>(&self, components: ComponentIter)
    where
        ComponentIter: IntoBoxedComponentIter,
    {
        let command = EntityCommand::Create(components.into_iter());
        self.command_sender
            .send(command)
            .expect("System command buffer should not be disconnected during system iteration");
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
    /// Creates a new [`Entity`] with the given components.
    Create(BoxedComponentIterator),
    /// Removes the given [`Entity`], if it still exists.
    Remove(Entity),
    /// Adds the given component to the given [`Entity`], if it still exists.
    AddComponent(ComponentAddition),
    /// Removes the given component from the given [`Entity`], if it still exists.
    RemoveComponent(ComponentRemoval),
}

impl EntityCommand {
    fn try_into_creation(self) -> Option<BoxedComponentIterator> {
        match self {
            EntityCommand::Create(components) => Some(components),
            _ => None,
        }
    }

    fn try_into_removal(self) -> Option<Entity> {
        match self {
            EntityCommand::Remove(entity) => Some(entity),
            _ => None,
        }
    }

    fn try_into_component_addition(self) -> Option<ComponentAddition> {
        match self {
            EntityCommand::AddComponent(addition) => Some(addition),
            _ => None,
        }
    }

    fn try_into_component_removal(self) -> Option<ComponentRemoval> {
        match self {
            EntityCommand::RemoveComponent(removal) => Some(removal),
            _ => None,
        }
    }
}

/// An addition of a specific component to a specific entity.
#[derive(Debug)]
pub struct ComponentAddition {
    pub(crate) entity: Entity,
    pub(crate) component: BoxedComponent,
}

impl ComponentAddition {
    fn new<IntoBoxed: IntoBoxedComponent>(entity: Entity, component: IntoBoxed) -> Self {
        Self {
            entity,
            component: component.into_box(),
        }
    }
}

/// An opaque type which stores a boxed component.
// This type helps us hide the `AnyComponent` trait, so we don't need to
// make `ComponentVec` public.
#[derive(Debug)]
pub struct BoxedComponent(pub(crate) Box<dyn AnyComponent>);

impl BoxedComponent {
    pub(crate) fn as_ref(&self) -> &dyn AnyComponent {
        self.0.as_ref()
    }
}

/// Components which can be boxed, so they can be stored uniformly in a collection.
pub trait IntoBoxedComponent {
    /// Boxes the component, hiding its type.
    fn into_box(self) -> BoxedComponent;
}

impl<ComponentType> IntoBoxedComponent for ComponentType
where
    ComponentType: AnyComponent + 'static,
{
    fn into_box(self) -> BoxedComponent {
        BoxedComponent(Box::new(self))
    }
}

/// An iterator over boxed components.
#[derive(Debug)]
pub struct BoxedComponentIterator {
    boxed_components: Vec<Option<BoxedComponent>>,
    current_index: usize,
}

/// Something that can be turned into a [`BoxedComponentIterator`].
pub trait IntoBoxedComponentIter {
    /// Turns `self` into a [`BoxedComponentIterator`].
    fn into_iter(self) -> BoxedComponentIterator;
}

impl IntoBoxedComponentIter for BoxedComponentIterator {
    fn into_iter(self) -> BoxedComponentIterator {
        self
    }
}

impl IntoBoxedComponentIter for Vec<BoxedComponent> {
    fn into_iter(self) -> BoxedComponentIterator {
        BoxedComponentIterator {
            boxed_components: <Vec<_> as IntoIterator>::into_iter(self)
                .map(Some)
                .collect(),
            current_index: 0,
        }
    }
}

impl<Iter, Function> IntoBoxedComponentIter for Map<Iter, Function>
where
    Iter: Iterator,
    Function: FnMut(Iter::Item) -> BoxedComponent,
{
    fn into_iter(self) -> BoxedComponentIterator {
        BoxedComponentIterator {
            boxed_components: self.map(Some).collect(),
            current_index: 0,
        }
    }
}

macro_rules! impl_into_boxed_component_iterator {
    ($($component:expr),*) => {
        paste! {
            impl<$([<C$component>]: IntoBoxedComponent,)*> IntoBoxedComponentIter for ($([<C$component>],)*) {
                fn into_iter(self) -> BoxedComponentIterator {
                    let boxed_components = vec![$(Some(self.$component.into_box()),)*];
                    BoxedComponentIterator {
                        boxed_components,
                        current_index: 0,
                    }
                }
            }
        }
    }
}

invoke_for_each_parameter_count!(impl_into_boxed_component_iterator);

impl Iterator for BoxedComponentIterator {
    type Item = BoxedComponent;

    fn next(&mut self) -> Option<Self::Item> {
        let maybe_component = self.boxed_components.get_mut(self.current_index)?;
        let component = maybe_component.take()?;
        self.current_index += 1;
        Some(component)
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

/// Segment for [`Commands`]
#[derive(Debug, Clone)]
pub struct CommandsSegment {
    system: Box<dyn System>,
}

impl IntoIterator for CommandsSegment {
    type Item = Commands;
    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        iter::from_fn(move || Some(Commands::from_system(self.system.as_ref())))
    }
}

impl SystemParameter for Commands {
    type BorrowedData<'components> = Box<dyn System>;
    type SegmentData = CommandsSegment;

    fn borrow<'world>(
        _world: &'world World,
        _archetypes: &[ArchetypeIndex],
        system: &Box<dyn System>,
    ) -> SystemParameterResult<Self::BorrowedData<'world>> {
        Ok(system.clone())
    }

    fn split_borrowed_data(
        borrowed: &mut Self::BorrowedData<'_>,
        segment_config: SegmentConfig,
    ) -> Vec<Self::SegmentData> {
        let segment_count = segment_config.get_segment_count();
        (0..segment_count)
            .map(|_| CommandsSegment {
                system: borrowed.clone(),
            })
            .collect()
    }

    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![]
    }

    fn controls_iteration() -> bool {
        false
    }

    fn base_signature() -> Option<TypeId> {
        None
    }
}

pub(crate) trait CommandPlayer {
    /// The type of errors returned by the object.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Plays back all commands recorded since the last playback.
    #[instrument(skip_all)]
    #[cfg_attr(feature = "profile", inline(never))]
    fn playback_commands(&mut self) -> Result<(), Self::Error> {
        let mut commands = self.receive_all_commands();

        let entity_components = commands
            .drain_filter(|command| matches!(command, EntityCommand::Create(_)))
            .filter_map(EntityCommand::try_into_creation);
        self.playback_creates(entity_components)?;

        let entities_to_be_removed: NoHashHashSet<_> = commands
            .drain_filter(|command| matches!(command, EntityCommand::Remove(_)))
            .filter_map(EntityCommand::try_into_removal)
            .collect();
        match self.playback_removes(entities_to_be_removed.clone()) {
            Ok(_) => {}
            Err(error) => debug!("failed to remove some entities: {error:?}"),
        }

        let component_additions = commands
            .drain_filter(|command| matches!(command, EntityCommand::AddComponent(_)))
            .filter_map(EntityCommand::try_into_component_addition)
            .filter(|addition| !entities_to_be_removed.contains(&addition.entity))
            .collect_vec();
        let added_component_types: FnvHashSet<_> = component_additions
            .iter()
            .map(|addition| addition.component.stored_type())
            .collect();

        let component_removals = commands
            .drain_filter(|command| matches!(command, EntityCommand::RemoveComponent(_)))
            .filter_map(EntityCommand::try_into_component_removal)
            .filter(|addition| !entities_to_be_removed.contains(&addition.entity))
            .collect_vec();
        let removed_component_types: FnvHashSet<_> = component_removals
            .iter()
            .map(|removal| removal.component_type)
            .collect();

        let additions_without_removed = component_additions
            .into_iter()
            .filter(|addition| !removed_component_types.contains(&addition.stored_type()));
        self.playback_add_components(additions_without_removed)?;

        let removals_without_added = component_removals
            .into_iter()
            .filter(|removal| !added_component_types.contains(&removal.stored_type()));
        match self.playback_remove_components(removals_without_added) {
            Ok(_) => {}
            Err(error) => debug!("failed to remove some components from entities: {error:?}"),
        }

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

    /// Executes all create-operations recorded since last playback.
    fn playback_creates(
        &mut self,
        to_create: impl IntoIterator<Item = impl IntoBoxedComponentIter>,
    ) -> Result<(), Self::Error>;

    /// Executes all remove-operations recorded since last playback.
    fn playback_removes(
        &mut self,
        to_remove: impl IntoIterator<Item = Entity>,
    ) -> Result<(), Self::Error>;

    /// Executes all add-component-operations recorded since last playback.
    fn playback_add_components(
        &mut self,
        additions: impl IntoIterator<Item = ComponentAddition>,
    ) -> Result<(), Self::Error>;

    /// Executes all remove-component-operations recorded since last playback.
    fn playback_remove_components(
        &mut self,
        removals: impl IntoIterator<Item = ComponentRemoval>,
    ) -> Result<(), Self::Error>;
}

impl<Executor, Schedule> CommandPlayer for ApplicationRunner<Executor, Schedule> {
    type Error = BasicApplicationError;

    #[instrument(skip_all)]
    #[cfg_attr(feature = "profile", inline(never))]
    fn receive_all_commands(&mut self) -> Vec<EntityCommand> {
        self.command_receivers
            .iter()
            .flat_map(|receiver| {
                iter::repeat_with(|| receiver.try_recv()).take_while(Result::is_ok)
            })
            .filter_map(Result::ok)
            .collect()
    }

    #[instrument(skip_all)]
    #[cfg_attr(feature = "profile", inline(never))]
    fn playback_creates(
        &mut self,
        to_create: impl IntoIterator<Item = impl IntoBoxedComponentIter>,
    ) -> Result<(), Self::Error> {
        drop(
            // Don't need the returned entity IDs.
            self.world
                .create_entities(to_create)
                .map_err(BasicApplicationError::World)?,
        );
        Ok(())
    }

    #[instrument(skip_all)]
    #[cfg_attr(feature = "profile", inline(never))]
    fn playback_removes(
        &mut self,
        to_remove: impl IntoIterator<Item = Entity>,
    ) -> Result<(), Self::Error> {
        self.world
            .delete_entities(to_remove)
            .map_err(BasicApplicationError::World)
    }

    #[instrument(skip_all)]
    #[cfg_attr(feature = "profile", inline(never))]
    fn playback_add_components(
        &mut self,
        additions: impl IntoIterator<Item = ComponentAddition>,
    ) -> Result<(), Self::Error> {
        self.world
            .add_components_to_entities(additions)
            .map_err(BasicApplicationError::World)
    }

    #[instrument(skip_all)]
    #[cfg_attr(feature = "profile", inline(never))]
    fn playback_remove_components(
        &mut self,
        removals: impl IntoIterator<Item = ComponentRemoval>,
    ) -> Result<(), Self::Error> {
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
    use test_utils::{A, B, C, D, E, F};

    #[test]
    fn command_buffer_is_unique_per_system() {
        let system0 = (|| {}).into_system();
        let segment0 = CommandsSegment {
            system: Box::new(system0),
        };
        let system1 = (|| {}).into_system();
        let segment1 = CommandsSegment {
            system: Box::new(system1),
        };

        // SAFETY: buffers will be dropped at same time as borrowed data.
        let (buffer0, buffer1) = {
            let buffer0 = segment0.into_iter().next().unwrap();
            let buffer1 = segment1.into_iter().next().unwrap();
            (buffer0, buffer1)
        };

        assert_ne!(
            buffer0, buffer1,
            "different systems should have different buffers"
        )
    }

    fn set_up_app_with_systems_and_entities(
        systems: impl IntoIterator<Item = impl IntoSystem<(Entity, Commands)>>,
    ) -> (BasicApplication, Entity, Entity, Entity) {
        let app_builder = BasicApplicationBuilder::default();
        let app_builder = systems
            .into_iter()
            .fold(app_builder, |app_builder, system| {
                app_builder.add_system(system)
            });
        let mut app = app_builder.build();

        // Create entities with various sets of components...
        let entity0 = app.create_entity((D, E, F)).unwrap();
        let entity1 = app.create_entity((D, E)).unwrap();
        let entity2 = app.create_entity((D,)).unwrap();

        (app, entity0, entity1, entity2)
    }

    fn remaining_entities_in_world(world: &World) -> Vec<Entity> {
        world.entities.iter().cloned().flatten().collect()
    }

    #[test]
    fn system_can_remove_entities_until_next_tick() {
        let removing_system = |entity: Entity, commands: Commands| {
            commands.remove(entity);
        };

        let (app, _, _, _) = set_up_app_with_systems_and_entities([removing_system]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        let remaining_entities = remaining_entities_in_world(&runner.world);
        assert!(
            remaining_entities.is_empty(),
            "all entities should be removed, but these remain: {:?}",
            remaining_entities
        )
    }

    #[test]
    fn system_removing_same_entity_second_time_is_a_noop() {
        let twice_removal_system = |entity: Entity, commands: Commands| {
            commands.remove(entity);
            commands.remove(entity);
        };

        let (app, _, _, _) = set_up_app_with_systems_and_entities([twice_removal_system]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        let remaining_entities = remaining_entities_in_world(&runner.world);
        assert!(
            remaining_entities.is_empty(),
            "all entities should be removed, but these remain: {:?}",
            remaining_entities
        )
    }

    #[test]
    fn two_systems_removing_same_entity_removes_that_entity_once() {
        let removing_system0 = |entity: Entity, commands: Commands| {
            commands.remove(entity);
        };
        let removing_system1 = |entity: Entity, commands: Commands| {
            commands.remove(entity);
        };

        let (app, _, _, _) =
            set_up_app_with_systems_and_entities([removing_system0, removing_system1]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        let remaining_entities = remaining_entities_in_world(&runner.world);
        assert!(
            remaining_entities.is_empty(),
            "all entities should be removed, but these remain: {:?}",
            remaining_entities
        )
    }

    fn read_component_values<ComponentType>(
        runner: &ApplicationRunner<Sequential, Unordered>,
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

    #[test]
    fn system_can_add_components_to_entities_until_next_tick() {
        let adding_system = |entity: Entity, commands: Commands| {
            commands.add_component(entity, A(entity.id as i32));
        };

        let (app, entity0, entity1, entity2) =
            set_up_app_with_systems_and_entities([adding_system]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        let component_values: Vec<_> = read_component_values::<A>(&runner)
            .iter()
            .map(|value| value.0 as u32)
            .collect();

        assert_eq!(
            itertools::sorted(vec![entity0.id, entity1.id, entity2.id]).collect_vec(),
            itertools::sorted(component_values).collect_vec(),
            "newly added component values should contain entity ids"
        );
    }

    #[test]
    fn system_cannot_add_same_component_type_twice() {
        let double_adding_system = |entity: Entity, commands: Commands| {
            commands.add_component(entity, A(entity.id as i32));
            commands.add_component(entity, A(-123));
        };

        let (app, _, _, _) = set_up_app_with_systems_and_entities([double_adding_system]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner
            .playback_commands()
            .expect_err("playback should result in error");

        let component_values = read_component_values::<A>(&runner);

        assert_eq!(
            Vec::<A>::new(),
            component_values,
            "no components should have been added"
        );
    }

    #[test]
    fn two_systems_cannot_add_same_component_type_to_same_entity() {
        let adding_system0 = |entity: Entity, commands: Commands| {
            commands.add_component(entity, A(entity.id as i32));
        };
        let adding_system1 = |entity: Entity, commands: Commands| {
            commands.add_component(entity, A(-123));
        };

        let (app, _, _, _) = set_up_app_with_systems_and_entities([adding_system0, adding_system1]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner
            .playback_commands()
            .expect_err("playback should result in error");

        let component_values = read_component_values::<A>(&runner);

        assert_eq!(
            Vec::<A>::new(),
            component_values,
            "no components should have been added"
        );
    }

    #[test]
    fn system_can_remove_components_from_entities_until_next_tick() {
        let removal_system = |entity: Entity, commands: Commands| {
            commands.remove_component::<D>(entity);
        };

        let (app, _, _, _) = set_up_app_with_systems_and_entities([removal_system]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        let component_values: Vec<_> = read_component_values::<D>(&runner);

        assert_eq!(
            component_values.len(),
            0,
            "all components of type D should have been removed"
        );
    }

    #[test]
    fn system_removing_same_component_type_from_entity_second_time_is_a_noop() {
        let twice_removal_system = |entity: Entity, commands: Commands| {
            commands.remove_component::<D>(entity);
            commands.remove_component::<D>(entity);
        };

        let (app, _, _, _) = set_up_app_with_systems_and_entities([twice_removal_system]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        let component_values: Vec<_> = read_component_values::<D>(&runner);

        assert_eq!(
            component_values.len(),
            0,
            "all components of type D should have been removed"
        );
    }

    #[test]
    fn two_systems_removing_same_component_type_from_same_entity_removes_that_component_type_once()
    {
        let removing_system0 = |entity: Entity, commands: Commands| {
            commands.remove_component::<D>(entity);
        };
        let removing_system1 = |entity: Entity, commands: Commands| {
            commands.remove_component::<D>(entity);
        };

        let (app, _, _, _) =
            set_up_app_with_systems_and_entities([removing_system0, removing_system1]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        let component_values: Vec<_> = read_component_values::<D>(&runner);

        assert_eq!(
            component_values.len(),
            0,
            "all components of type D should have been removed"
        );
    }

    #[test]
    fn system_can_create_new_entity_with_multiple_components() {
        let creation_system = |entity: Entity, commands: Commands| {
            commands.create((A(entity.id as i32), B("hi".to_owned()), C(1.0)));
        };

        let (app, _, _, _) = set_up_app_with_systems_and_entities([creation_system]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        runner.tick().unwrap();
        runner.playback_commands().unwrap();

        let a_values: Vec<_> = read_component_values::<A>(&runner);
        let b_values: Vec<_> = read_component_values::<B>(&runner);
        let c_values: Vec<_> = read_component_values::<C>(&runner);

        assert_eq!(
            runner.world.entities.len(),
            6,
            "three new entities should have been created"
        );
        assert_eq!(
            a_values.len(),
            3,
            "3 entities with components A, B and C should have been created"
        );
        assert_eq!(a_values.len(), b_values.len());
        assert_eq!(b_values.len(), c_values.len());
    }

    #[test]
    fn creating_and_removing_entities_multiple_times_does_not_cause_panic() {
        let removing_creation_system = |entity: Entity, commands: Commands| {
            commands.remove(entity);
            commands.create((C(1.0),));
        };

        let (app, _, _, _) = set_up_app_with_systems_and_entities([removing_creation_system]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        run_ticks(&mut runner);
    }

    fn run_ticks(runner: &mut ApplicationRunner<Sequential, Unordered>) {
        for i in 0..10 {
            runner.tick().unwrap();
            if let Err(error) = runner.playback_commands() {
                eprintln!("{error:#?}");
                panic!("failed on iteration {i}")
            }
        }
    }

    #[test]
    fn system_removing_entities_while_other_system_adds_component_to_them_removes_the_entities() {
        let adding_system = |entity: Entity, commands: Commands| {
            commands.add_component(entity, A(0));
        };
        let entity_removal_system = |entity: Entity, commands: Commands| {
            commands.remove(entity);
        };

        let (app, _, _, _) =
            set_up_app_with_systems_and_entities([adding_system, entity_removal_system]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        run_ticks(&mut runner);

        let a_values: Vec<_> = read_component_values::<A>(&runner);
        assert_eq!(a_values.len(), 0, "no A-components should have been added");
    }

    #[test]
    fn system_adding_component_other_system_removes_is_a_noop() {
        let adding_system = |entity: Entity, commands: Commands| {
            commands.add_component(entity, A(0));
        };
        let removal_system = |entity: Entity, commands: Commands| {
            commands.remove_component::<A>(entity);
        };

        let (app, _, _, _) = set_up_app_with_systems_and_entities([adding_system, removal_system]);

        let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
        run_ticks(&mut runner);

        let a_values: Vec<_> = read_component_values::<A>(&runner);
        assert_eq!(a_values.len(), 0, "no A-components should have been added");
    }
}
