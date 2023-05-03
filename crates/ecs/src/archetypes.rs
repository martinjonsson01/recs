use crate::systems::command_buffers::{AnyComponent, BoxedComponent, IntoBoxedComponentIter};
use crate::systems::ComponentIndex;
use crate::{
    get_mut_at_two_indices, ArchetypeIndex, Entity, NoHashHashMap, ReadComponentVec, World,
    WorldError, WorldResult, WriteComponentVec,
};
use fnv::{FnvHashMap, FnvHashSet};
use itertools::Itertools;
use parking_lot::RwLock;
use std::any;
use std::any::{Any, TypeId};
use std::fmt::Debug;
use thiserror::Error;

pub(crate) type ComponentVecImpl<ComponentType> = RwLock<Vec<ComponentType>>;

#[derive(Error, Debug)]
pub(crate) enum ComponentVecError {
    #[error("the provided type does not match the vector's: {0:?}")]
    IncorrectType(&'static str),
}

pub(crate) type ComponentVecResult<T> = Result<T, ComponentVecError>;

pub(crate) trait ComponentVec: Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Returns the type stored in the component vector.
    fn stored_type(&self) -> TypeId;
    /// Returns the number of components stored in the component vector.
    fn len(&self) -> usize;
    /// Removes the entity from the component vector.
    fn remove(&self, index: usize);
    /// Removes a component and pushes it to another
    /// archetypes component vector of the same data type.
    fn move_element(&self, source_index: usize, target_arch: &mut Archetype)
        -> ArchetypeResult<()>;
    /// Tries to add the given element, returning whether it succeeded.
    fn try_add_element(&mut self, element: Box<dyn Any>) -> ComponentVecResult<()>;
}

impl<T: Debug + Send + Sync + 'static> ComponentVec for ComponentVecImpl<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn stored_type(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn len(&self) -> usize {
        Vec::len(&self.read())
    }

    fn remove(&self, index: usize) {
        self.write().swap_remove(index);
    }

    fn move_element(
        &self,
        source_index: usize,
        target_arch: &mut Archetype,
    ) -> ArchetypeResult<()> {
        let value = self.write().swap_remove(source_index);

        if !target_arch.contains_component_type::<T>() {
            target_arch.add_component_vec::<T>()
        }

        let mut component_vec = target_arch.borrow_component_vec_mut::<T>().ok_or(
            ArchetypeError::ComponentTypeNotPresent(any::type_name::<T>()),
        )?;

        component_vec.push(value);

        Ok(())
    }

    fn try_add_element(&mut self, element: Box<dyn Any>) -> ComponentVecResult<()> {
        let element = element
            .downcast::<T>()
            .map_err(|_| ComponentVecError::IncorrectType(any::type_name::<Box<dyn Any>>()))?;
        self.write().push(*element);
        Ok(())
    }
}

trait BorrowableComponentVec {
    /// Tries to borrow the component vec as a given type.
    ///
    /// Will return `None` if the typecast doesn't work, otherwise
    /// it will return the component vec with the requested static type.
    fn try_cast_to<ComponentType: 'static>(&self) -> ReadComponentVec<ComponentType>;

    /// Tries to mutably borrow the component vec as a given type.
    ///
    /// Will return `None` if the typecast doesn't work, otherwise
    /// it will return the component vec with the requested static type.
    fn try_cast_to_mut<ComponentType: 'static>(&self) -> WriteComponentVec<ComponentType>;
}

impl BorrowableComponentVec for Box<dyn ComponentVec> {
    fn try_cast_to<ComponentType: 'static>(&self) -> ReadComponentVec<ComponentType> {
        if let Some(component_vec) = self
            .as_any()
            .downcast_ref::<ComponentVecImpl<ComponentType>>()
        {
            // This method should only be called once the scheduler has verified
            // that component access can be done without contention.
            // Panicking helps us detect errors in the scheduling algorithm more quickly.
            return match component_vec.try_read() {
                Some(component_vec) => Some(component_vec),
                None => panic_locked_component_vec::<ComponentType>(),
            };
        }
        None
    }

    fn try_cast_to_mut<ComponentType: 'static>(&self) -> WriteComponentVec<ComponentType> {
        if let Some(component_vec) = self
            .as_any()
            .downcast_ref::<ComponentVecImpl<ComponentType>>()
        {
            // This method should only be called once the scheduler has verified
            // that component access can be done without contention.
            // Panicking helps us detect errors in the scheduling algorithm more quickly.
            return match component_vec.try_write() {
                Some(component_vec) => Some(component_vec),
                None => panic_locked_component_vec::<ComponentType>(),
            };
        }
        None
    }
}

fn create_raw_component_vec<ComponentType: Debug + Send + Sync + 'static>() -> Box<dyn ComponentVec>
{
    Box::<ComponentVecImpl<ComponentType>>::default()
}

fn panic_locked_component_vec<ComponentType: 'static>() -> ! {
    let component_type_name = any::type_name::<ComponentType>();
    panic!(
        "Lock of ComponentVec<{}> is already taken!",
        component_type_name
    )
}

/// An error occurred during a archetype operation.
#[derive(Error, Debug)]
pub enum ArchetypeError {
    /// Could not find component index of entity
    #[error("could not find component index of entity: {0:?}")]
    MissingEntityIndex(Entity),

    /// The requested component type is not present
    #[error("the requested component type is not present: {0:?}")]
    ComponentTypeNotPresent(&'static str),

    /// Component vec not found for type id
    #[error("component vec not found for type id: {0:?}")]
    MissingComponentType(TypeId),

    /// Archetype already contains entity
    #[error("archetype already contains entity: {0:?}")]
    EntityAlreadyExists(Entity),
}

/// Whether a archetype operation succeeded.
pub type ArchetypeResult<T, E = ArchetypeError> = Result<T, E>;

/// Stores components associated with entity ids.
#[derive(Debug, Default)]
pub struct Archetype {
    component_typeid_to_component_vec: FnvHashMap<TypeId, Box<dyn ComponentVec>>,
    entity_to_component_index: NoHashHashMap<Entity, ComponentIndex>,
    entities: Vec<Entity>,
}

/// Newly created entities with no components on them, are placed in this archetype.
pub(super) const EMPTY_ENTITY_ARCHETYPE_INDEX: ArchetypeIndex = 0;

type TargetArchSourceTargetIDs = (
    Option<ArchetypeIndex>,
    FnvHashSet<TypeId>,
    FnvHashSet<TypeId>,
);

enum EntityComponentMutation<'a> {
    Removal(Entity, &'a [TypeId]),
    Addition(Entity, &'a [TypeId]),
}

impl Archetype {
    /// Gets the [`ComponentIndex`] of a given [`Entity`] in this archetype.
    pub(crate) fn get_component_index_of(&self, entity: Entity) -> ArchetypeResult<ComponentIndex> {
        self.entity_to_component_index
            .get(&entity)
            .cloned()
            .ok_or(ArchetypeError::MissingEntityIndex(entity))
    }

    /// Gets an [`Entity`] stored in this archetype by the [`ComponentIndex`]
    /// which its component data is stored at.
    ///
    /// Note: the [`ComponentIndex`] is only valid in the same archetype it was
    /// created. Using a [`ComponentIndex`] from another archetype will not provide
    /// the correct [`Entity`].
    pub(crate) fn get_entity(&self, component_index: ComponentIndex) -> Option<Entity> {
        self.entities.get(component_index).cloned()
    }

    /// Adds an entity to keep track of and store components for.
    ///
    /// Returns error if entity with same `id` has been stored previously.
    fn store_entity(&mut self, entity: Entity) -> ArchetypeResult<()> {
        let entity_index = self.entity_to_component_index.len();

        match self.entity_to_component_index.insert(entity, entity_index) {
            None => {
                self.entities.push(entity);
                Ok(())
            }
            Some(_) => Err(ArchetypeError::EntityAlreadyExists(entity)),
        }
    }

    /// Removes the given [`Entity`].
    pub(super) fn remove_entity(&mut self, entity: Entity) -> ArchetypeResult<()> {
        let entity_component_index = self.get_component_index_of(entity)?;

        self.component_typeid_to_component_vec
            .values()
            .for_each(|component_vec| component_vec.remove(entity_component_index));

        self.update_after_entity_removal(entity)?;

        Ok(())
    }

    /// Returns a `ReadComponentVec` with the specified generic type `ComponentType` if it is stored.
    pub(crate) fn borrow_component_vec<ComponentType: Debug + Send + Sync + 'static>(
        &self,
    ) -> ReadComponentVec<ComponentType> {
        let component_typeid = TypeId::of::<ComponentType>();
        self.component_typeid_to_component_vec
            .get(&component_typeid)
            .and_then(BorrowableComponentVec::try_cast_to)
    }

    /// Returns a `WriteComponentVec` with the specified generic type `ComponentType` if it is stored.
    pub(crate) fn borrow_component_vec_mut<ComponentType: Debug + Send + Sync + 'static>(
        &self,
    ) -> WriteComponentVec<ComponentType> {
        let component_typeid = TypeId::of::<ComponentType>();
        self.component_typeid_to_component_vec
            .get(&component_typeid)
            .and_then(BorrowableComponentVec::try_cast_to_mut)
    }

    /// Adds many different components to the archetype simultaneously.
    fn add_components(&mut self, components: impl IntoBoxedComponentIter) -> ArchetypeResult<()> {
        components.into_iter().try_for_each(|component| {
            let component_vec = self
                .component_typeid_to_component_vec
                .get_mut(&component.as_ref().stored_type())
                .ok_or(ArchetypeError::ComponentTypeNotPresent(
                    component.as_ref().stored_type_name(),
                ))?;
            let casted_component: Box<dyn Any> = component.0;
            component_vec
                .try_add_element(casted_component)
                .expect("vector type should match component type id");
            Ok(())
        })
    }

    /// Adds a component vec of type `ComponentType` if no such vec already exists.
    ///
    /// This function is idempotent when trying to add the same `ComponentType` multiple times.
    fn add_component_vec<Component: AnyComponent>(&mut self) {
        if !self.contains_component_type::<Component>() {
            let raw_component_vec = create_raw_component_vec::<Component>();

            let component_typeid = TypeId::of::<Component>();
            self.component_typeid_to_component_vec
                .insert(component_typeid, raw_component_vec);
        }
    }

    fn add_component_vec_for_any_component(&mut self, component: &dyn AnyComponent) {
        if !self.contains_component_type_id(component.stored_type()) {
            let raw_component_vec = component.create_raw_component_vec();

            self.component_typeid_to_component_vec
                .insert(component.stored_type(), raw_component_vec);
        }
    }

    /// Returns `true` if the archetype stores components of type ComponentType.
    fn contains_component_type<ComponentType: Debug + Send + Sync + 'static>(&self) -> bool {
        self.component_typeid_to_component_vec
            .contains_key(&TypeId::of::<ComponentType>())
    }

    fn contains_component_type_id(&self, type_id: TypeId) -> bool {
        self.component_typeid_to_component_vec
            .contains_key(&type_id)
    }

    fn update_after_entity_removal(&mut self, entity: Entity) -> ArchetypeResult<()> {
        let removed_entity_component_index = self.get_component_index_of(entity)?;

        // Update component index of entity which will be swapped during removal.
        if let Some(&will_be_swapped_entity) = self.entities.last() {
            self.entity_to_component_index
                .insert(will_be_swapped_entity, removed_entity_component_index);
        }
        self.entities.swap_remove(removed_entity_component_index);

        self.entity_to_component_index.remove(&entity);

        Ok(())
    }
    fn component_types(&self) -> FnvHashSet<TypeId> {
        self.component_typeid_to_component_vec
            .keys()
            .cloned()
            .collect()
    }
}

impl World {
    pub(super) fn create_empty_entity(&mut self) -> WorldResult<Entity> {
        if self.archetypes.is_empty() {
            // Insert the "empty entity archetype" if not already created.
            self.archetypes.push(Archetype::default());
        }

        let entity_id = u32::try_from(self.entities.len())
            .expect("entities vector should be short enough for its length to be 32-bit");
        let entity = Entity {
            id: entity_id,
            generation: 0, // Freshly created entities are given entirely new IDs
        };
        self.entities.push(entity);
        self.store_entity_in_archetype(entity, EMPTY_ENTITY_ARCHETYPE_INDEX)?;
        Ok(entity)
    }

    /// Returns the archetype index of the archetype that contains all
    /// components types that the entity is tied to +- the generic type
    /// supplied to the function given Addition/ Removal enum status.
    /// `None` is returned if no archetype containing only the sought after component types exist.
    /// The type ids contained within the archetype the entity is existing in and the type ids for
    /// sought archetype are also returned.
    ///
    /// Set mutation to ArchetypeMutation::Addition(entity)/ ArchetypeMutation::Removal(entity)
    /// depending on if the supplied generic type should be added to or
    /// removed from the specified entity.
    ///
    /// For example call world.find_target_archetype::<u32>(ArchetypeMutation::Removal(entity))
    /// to fetch the archetype index of the archetype
    /// containing all component types except u32 that the entity is tied to.
    ///
    /// world.find_target_archetype::<u32>(ArchetypeMutation::Addition(entity))
    /// would return the archetype index of the
    /// archetype containing all component types that the entity is tied to with the addition
    /// of u32.
    fn find_target_archetype(
        &self,
        mutation: EntityComponentMutation,
    ) -> WorldResult<TargetArchSourceTargetIDs> {
        let source_archetype_type_ids: FnvHashSet<TypeId>;

        let mut target_archetype_type_ids: FnvHashSet<TypeId>;

        match mutation {
            EntityComponentMutation::Removal(entity, component_types) => {
                let source_archetype = self.get_archetype_of_entity(entity)?;

                source_archetype_type_ids = source_archetype.component_types();

                target_archetype_type_ids = source_archetype_type_ids.clone();

                let failed_removals: Vec<_> = component_types
                    .iter()
                    .map(|type_to_remove| {
                        let was_present = target_archetype_type_ids.remove(type_to_remove);
                        (was_present, type_to_remove)
                    })
                    .filter(|(component_type_exists, _)| !*component_type_exists)
                    .map(|(_, &non_existent_type)| {
                        WorldError::ComponentTypeNotPresentForEntity(entity, non_existent_type)
                    })
                    .collect();

                if !failed_removals.is_empty() {
                    return Err(WorldError::ComponentMutationFailed(failed_removals));
                }
            }

            EntityComponentMutation::Addition(entity, component_types) => {
                let source_archetype = self.get_archetype_of_entity(entity)?;

                source_archetype_type_ids = source_archetype.component_types();

                target_archetype_type_ids = source_archetype_type_ids.clone();

                target_archetype_type_ids.extend(component_types.iter());

                let already_added_components: Vec<_> = component_types
                    .iter()
                    .map(|type_to_add| {
                        let was_present = source_archetype_type_ids.contains(type_to_add);
                        (was_present, type_to_add)
                    })
                    .filter(|(component_type_exists, _)| *component_type_exists)
                    .map(|(_, &already_present_type)| {
                        WorldError::ComponentTypeAlreadyExistsForEntity(
                            entity,
                            already_present_type,
                        )
                    })
                    .collect();

                if !already_added_components.is_empty() {
                    return Err(WorldError::ComponentMutationFailed(
                        already_added_components,
                    ));
                }
            }
        }

        let target_archetype_type_ids_vec: Vec<TypeId> = target_archetype_type_ids
            .iter()
            .cloned()
            .collect::<Vec<TypeId>>();

        let maybe_target_archetype =
            self.get_exactly_matching_archetype(&target_archetype_type_ids_vec);

        Ok((
            maybe_target_archetype,
            source_archetype_type_ids,
            target_archetype_type_ids,
        ))
    }

    fn move_entity_components_between_archetypes(
        &mut self,
        entity: Entity,
        target_archetype_index: ArchetypeIndex,
        components_to_move: FnvHashSet<TypeId>,
    ) -> WorldResult<()> {
        let source_archetype_index = *self
            .entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))?;

        let (source_archetype, target_archetype) = get_mut_at_two_indices(
            source_archetype_index,
            target_archetype_index,
            &mut self.archetypes,
        );

        let source_component_index = *source_archetype
            .entity_to_component_index
            .get(&entity)
            .expect("Entity should have a component index tied to it since it is already established to be existing within the archetype");

        for type_id in components_to_move {
            let source_component_vec = source_archetype
                .component_typeid_to_component_vec
                .get(&type_id)
                .expect(
                    "Type that tried to be fetched should exist
                     in archetype since types are fetched from this archetype originally.",
                );

            source_component_vec
                .move_element(source_component_index, target_archetype)
                .map_err(WorldError::CouldNotMoveComponent)?;
        }

        target_archetype
            .store_entity(entity)
            .map_err(WorldError::CouldNotMoveComponent)?;

        self.entity_to_archetype_index
            .insert(entity, target_archetype_index);

        Ok(())
    }

    fn move_entity_components_to_new_archetype(
        &mut self,
        entity: Entity,
        components_to_move: FnvHashSet<TypeId>,
    ) -> WorldResult<ArchetypeIndex> {
        let new_archetype = Archetype::default();
        let new_archetype_index: ArchetypeIndex = self.archetypes.len();
        self.archetypes.push(new_archetype);

        for component_type in &components_to_move {
            self.component_typeid_to_archetype_indices
                .get_mut(component_type)
                .expect("Type ID should exist for previously existing types")
                .insert(new_archetype_index);
        }

        self.move_entity_components_between_archetypes(
            entity,
            new_archetype_index,
            components_to_move,
        )?;

        Ok(new_archetype_index)
    }

    pub(super) fn add_component_to_entity<ComponentType>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> WorldResult<()>
    where
        ComponentType: Debug + Send + Sync + 'static,
    {
        self.add_components_to_entity(entity, (component,))
    }

    pub(super) fn add_components_to_entity(
        &mut self,
        entity: Entity,
        components: impl IntoBoxedComponentIter,
    ) -> WorldResult<()> {
        let source_archetype_index = *self
            .entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))?;

        let components: Vec<_> = components.into_iter().collect();

        let component_types: Vec<_> = components
            .iter()
            .map(BoxedComponent::as_ref)
            .map(|component| component.stored_type())
            .collect();
        if let Some(&duplicated_component_type) = component_types.iter().duplicates().next() {
            return Err(WorldError::ComponentTypeAlreadyExistsForEntity(
                entity,
                duplicated_component_type,
            ));
        }
        let add = EntityComponentMutation::Addition(entity, &component_types);

        let (target_archetype_exists, source_type_ids, target_type_ids) =
            self.find_target_archetype(add)?;

        match target_archetype_exists {
            Some(target_archetype_index) => {
                self.move_entity_components_between_archetypes(
                    entity,
                    target_archetype_index,
                    source_type_ids,
                )?;

                let target_archetype = self.get_archetype_mut(target_archetype_index)?;
                target_archetype
                    .add_components(components)
                    .map_err(WorldError::CouldNotAddComponent)?;
            }
            None => {
                let target_archetype_index =
                    self.move_entity_components_to_new_archetype(entity, source_type_ids)?;

                let mut target_type_ids_vec = Vec::from_iter(target_type_ids);

                // Sort the vec since order of elements in vec matter when matching keys.
                // Sorting the key is done both when inserting and reading from this hashmap to
                // maintain consistent behaviour.
                target_type_ids_vec.sort();
                self.component_typeids_set_to_archetype_index
                    .insert(target_type_ids_vec, target_archetype_index);

                // Have to use manual implementation instead of self.get_archetype_mut
                // because otherwise the compiler can't see that the mutable accesses
                // to self (self.archetypes and self.component_typeid_to_archetype_indices)
                // below are disjoint.
                let target_archetype = self
                    .archetypes
                    .get_mut(target_archetype_index)
                    .ok_or(WorldError::ArchetypeDoesNotExist(target_archetype_index))?;

                // Handle incoming components
                components
                    .iter()
                    .map(BoxedComponent::as_ref)
                    .for_each(|component| {
                        target_archetype.add_component_vec_for_any_component(component);
                    });
                components
                    .iter()
                    .map(BoxedComponent::as_ref)
                    .for_each(|component| {
                        let archetype_indices = self
                            .component_typeid_to_archetype_indices
                            .entry(component.stored_type())
                            .or_default();
                        archetype_indices.insert(target_archetype_index);
                    });
                target_archetype
                    .add_components(components)
                    .map_err(WorldError::CouldNotAddComponent)?;
            }
        }

        let source_archetype = self.get_archetype_mut(source_archetype_index)?;

        source_archetype.update_after_entity_removal(entity).expect(
            "Entity should yield a component index
             in archetype since the the relevant archetype
              was fetched from the entity itself.",
        );

        Ok(())
    }

    pub(super) fn remove_component_type_from_entity<ComponentType>(
        &mut self,
        entity: Entity,
    ) -> WorldResult<()>
    where
        ComponentType: Debug + Send + Sync + 'static,
    {
        self.remove_component_types_from_entity(entity, [TypeId::of::<ComponentType>()])
    }

    pub(super) fn remove_component_types_from_entity(
        &mut self,
        entity: Entity,
        component_types: impl IntoIterator<Item = TypeId>,
    ) -> WorldResult<()> {
        let source_archetype_index = *self
            .entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))?;

        let component_types: Vec<_> = component_types.into_iter().collect();
        let removal = EntityComponentMutation::Removal(entity, &component_types);
        let (target_archetype_exists, _, target_type_ids) = self.find_target_archetype(removal)?;

        match target_archetype_exists {
            Some(target_archetype_index) => {
                self.move_entity_components_between_archetypes(
                    entity,
                    target_archetype_index,
                    target_type_ids,
                )?;
            }
            None => {
                let mut target_type_ids_vec: Vec<TypeId> =
                    target_type_ids.iter().cloned().collect();
                let target_archetype_index =
                    self.move_entity_components_to_new_archetype(entity, target_type_ids)?;

                // Sort the vec since order of elements in vec matter when matching keys.
                // Sorting the key is done both when inserting and reading from this hashmap to
                // maintain consistent behaviour.
                target_type_ids_vec.sort();
                self.component_typeids_set_to_archetype_index
                    .insert(target_type_ids_vec, target_archetype_index);
            }
        }

        let source_archetype = self.get_archetype_mut(source_archetype_index)?;

        let source_component_vec_index = *source_archetype.entity_to_component_index
            .get(&entity)
            .expect("Entity should have a component index tied to it since it is already established to be existing within the archetype");

        component_types.into_iter().for_each(|component_type| {
            let source_component_vec = source_archetype
                .component_typeid_to_component_vec
                .get(&component_type)
                .expect(
                    "Type that tried to be fetched should exist
                         in archetype since types are fetched from this archetype originally.",
                );
            source_component_vec.remove(source_component_vec_index);
        });

        source_archetype.update_after_entity_removal(entity).expect(
            "Entity should yield a component index
             in archetype since the the relevant archetype
              was fetched from the entity itself.",
        );

        Ok(())
    }

    fn store_entity_in_archetype(
        &mut self,
        entity: Entity,
        archetype_index: ArchetypeIndex,
    ) -> WorldResult<()> {
        let archetype = self.get_archetype_mut(archetype_index)?;

        archetype
            .store_entity(entity)
            .map_err(WorldError::CouldNotAddComponent)?;

        self.entity_to_archetype_index
            .insert(entity, archetype_index);
        Ok(())
    }

    pub(super) fn get_archetypes(
        &self,
        archetype_indices: &[ArchetypeIndex],
    ) -> WorldResult<Vec<&Archetype>> {
        let archetypes: Result<Vec<_>, _> = archetype_indices
            .iter()
            .map(|&archetype_index| self.get_archetype(archetype_index))
            .collect();
        archetypes
    }

    fn get_exactly_matching_archetype(
        &self,
        component_type_ids: &Vec<TypeId>,
    ) -> Option<ArchetypeIndex> {
        if component_type_ids.is_empty() {
            return Some(EMPTY_ENTITY_ARCHETYPE_INDEX);
        }
        let mut sorted_component_type_ids = component_type_ids.clone();
        // Sort the vec since order of elements in vec matter when matching keys.
        // Sorting the key is done both when inserting and reading from this hashmap to
        // maintain consistent behaviour.
        sorted_component_type_ids.sort();
        self.component_typeids_set_to_archetype_index
            .get(&sorted_component_type_ids)
            .copied()
    }

    pub(crate) fn get_archetype_index_of_entity(
        &self,
        entity: Entity,
    ) -> WorldResult<ArchetypeIndex> {
        self.entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))
            .cloned()
    }

    fn get_archetype_of_entity(&self, entity: Entity) -> WorldResult<&Archetype> {
        let source_archetype_index = self.get_archetype_index_of_entity(entity)?;

        let source_archetype = self.get_archetype(source_archetype_index)?;

        Ok(source_archetype)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use test_log::test;

    #[derive(Debug)]
    struct A;

    #[derive(Debug)]
    struct B;

    #[derive(Debug)]
    struct C;

    trait BorrowComponentVecsWithSignature {
        fn borrow_component_vecs_with_signature<ComponentType: Debug + Send + Sync + 'static>(
            &self,
            signature: &[TypeId],
        ) -> WorldResult<Vec<ReadComponentVec<ComponentType>>>;

        fn borrow_component_vecs_with_signature_mut<ComponentType: Debug + Send + Sync + 'static>(
            &self,
            signature: &[TypeId],
        ) -> WorldResult<Vec<WriteComponentVec<ComponentType>>>;
    }

    impl BorrowComponentVecsWithSignature for World {
        fn borrow_component_vecs_with_signature<ComponentType: Debug + Send + Sync + 'static>(
            &self,
            signature: &[TypeId],
        ) -> WorldResult<Vec<ReadComponentVec<ComponentType>>> {
            let archetype_indices: Vec<_> =
                self.get_archetype_indices(signature).into_iter().collect();
            self.borrow_component_vecs(&archetype_indices)
        }

        fn borrow_component_vecs_with_signature_mut<
            ComponentType: Debug + Send + Sync + 'static,
        >(
            &self,
            signature: &[TypeId],
        ) -> WorldResult<Vec<WriteComponentVec<ComponentType>>> {
            let archetype_indices: Vec<_> =
                self.get_archetype_indices(signature).into_iter().collect();
            self.borrow_component_vecs_mut(&archetype_indices)
        }
    }

    #[test]
    #[should_panic(expected = "Lock of ComponentVec<ecs::archetypes::tests::A> is already taken!")]
    fn world_panics_when_trying_to_mutably_borrow_same_components_twice() {
        let mut world = World::default();

        world.create_entity((A,)).unwrap();

        let _first = world
            .borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()])
            .unwrap();
        let _second = world
            .borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()])
            .unwrap();
    }

    #[test]
    fn world_doesnt_panic_when_mutably_borrowing_components_after_dropping_previous_mutable_borrow()
    {
        let mut world = World::default();

        world.create_entity((A,)).unwrap();

        let first = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
        drop(first);
        let _second = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
    }

    #[test]
    fn world_does_not_panic_when_trying_to_immutably_borrow_same_components_twice() {
        let mut world = World::default();

        world.create_entity((A,)).unwrap();

        let _first = world.borrow_component_vecs_with_signature::<A>(&[TypeId::of::<A>()]);
        let _second = world.borrow_component_vecs_with_signature::<A>(&[TypeId::of::<A>()]);
    }

    fn setup_world_with_3_entities_with_u32_and_i32_components(
    ) -> (World, ArchetypeIndex, Entity, Entity, Entity) {
        let mut world = World::default();

        let entity1 = world.create_entity((1_u32, 1_i32)).unwrap();
        let entity2 = world.create_entity((2_u32, 2_i32)).unwrap();
        let entity3 = world.create_entity((3_u32, 3_i32)).unwrap();

        let archetype_index = world.get_archetype_index_of_entity(entity1).unwrap();

        (world, archetype_index, entity1, entity2, entity3)
    }

    #[test]
    fn type_id_order_does_not_affect_fetching_of_correct_archetype() {
        let (world, _, _, _, _) = setup_world_with_3_entities_with_u32_and_i32_components();

        let type_vector_1 = vec![TypeId::of::<u32>(), TypeId::of::<i32>()];
        let type_vector_2 = vec![TypeId::of::<i32>(), TypeId::of::<u32>()];
        let result_1 = world
            .get_exactly_matching_archetype(&type_vector_1)
            .unwrap();
        let result_2 = world
            .get_exactly_matching_archetype(&type_vector_2)
            .unwrap();

        assert_eq!(result_1, result_2);
    }

    #[test]
    fn entities_are_in_expected_order_according_to_when_components_were_added() {
        let (world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        let archetype = world.get_archetype(relevant_archetype_index).unwrap();

        assert_eq!(archetype.entities, vec![entity1, entity2, entity3]);
    }

    #[test]
    fn entity_order_swap_index_after_entity_has_been_moved_by_addition() {
        let (mut world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world.add_component_to_entity(entity1, 1_usize).unwrap();

        let archetype = world.get_archetype(relevant_archetype_index).unwrap();

        assert_eq!(archetype.entities, vec![entity3, entity2]);
    }

    #[test]
    fn entity_order_swap_index_after_entity_has_been_moved_by_removal() {
        let (mut world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world
            .remove_component_type_from_entity::<u32>(entity1)
            .unwrap();

        let archetype = world.get_archetype(relevant_archetype_index).unwrap();

        assert_eq!(archetype.entities, vec![entity3, entity2]);
    }

    #[test]
    fn entity_to_component_index_gives_expected_values_after_addition() {
        let (world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        let archetype = world.get_archetype(relevant_archetype_index).unwrap();

        let entity1_component_index = *archetype.entity_to_component_index.get(&entity1).unwrap();
        let entity2_component_index = *archetype.entity_to_component_index.get(&entity2).unwrap();
        let entity3_component_index = *archetype.entity_to_component_index.get(&entity3).unwrap();

        assert_eq!(entity1_component_index, 0);
        assert_eq!(entity2_component_index, 1);
        assert_eq!(entity3_component_index, 2);
    }

    #[test]
    fn entity_to_component_index_is_updated_after_move_by_removal() {
        let (mut world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world
            .remove_component_type_from_entity::<u32>(entity1)
            .unwrap();

        let archetype = world.get_archetype(relevant_archetype_index).unwrap();

        let entity1_component_index = archetype.entity_to_component_index.get(&entity1);
        let entity2_component_index = archetype.entity_to_component_index.get(&entity2);
        let entity3_component_index = archetype.entity_to_component_index.get(&entity3);
        assert!(entity1_component_index.is_none());
        assert_eq!(entity2_component_index, Some(&1));
        assert_eq!(entity3_component_index, Some(&0));
    }

    #[test]
    fn entity_to_component_index_is_updated_after_move_by_addition() {
        let (mut world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world.add_component_to_entity(entity1, 1_usize).unwrap();

        let archetype = world.get_archetype(relevant_archetype_index).unwrap();

        let entity1_component_index = archetype.entity_to_component_index.get(&entity1);
        let entity2_component_index = archetype.entity_to_component_index.get(&entity2);
        let entity3_component_index = archetype.entity_to_component_index.get(&entity3);
        assert!(entity1_component_index.is_none());
        assert_eq!(entity2_component_index, Some(&1));
        assert_eq!(entity3_component_index, Some(&0));
    }

    #[test]
    fn entity_to_component_index_gives_expected_values_after_removal() {
        let (mut world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to another archetype
        world
            .remove_component_type_from_entity::<u32>(entity1)
            .unwrap();

        let archetype_1 = world.get_archetype_of_entity(entity1).unwrap();
        let archetype_2 = world.get_archetype(relevant_archetype_index).unwrap();

        let arch_1_entity1_component_index = archetype_1.entity_to_component_index.get(&entity1);

        let arch_2_entity1_component_index = archetype_2.entity_to_component_index.get(&entity1);
        let arch_2_entity2_component_index = archetype_2.entity_to_component_index.get(&entity2);
        let arch_2_entity3_component_index = archetype_2.entity_to_component_index.get(&entity3);

        assert_eq!(arch_1_entity1_component_index, Some(&0));
        assert_eq!(arch_2_entity1_component_index, None);
        assert_eq!(arch_2_entity2_component_index, Some(&1));
        assert_eq!(arch_2_entity3_component_index, Some(&0));
    }

    #[test]
    fn borrow_with_signature_returns_expected_values() {
        // Arrange
        let (world, _, _, _, _) = setup_world_with_3_entities_with_u32_and_i32_components();

        // Act
        // Borrow all vecs containing u32 from archetypes have the signature u32
        let vecs_u32 = world
            .borrow_component_vecs_with_signature::<u32>(&[TypeId::of::<u32>()])
            .unwrap();
        eprintln!("vecs_u32 = {vecs_u32:#?}");
        // Assert
        // Collect values from vecs
        let result: HashSet<u32> = vecs_u32
            .iter()
            .flat_map(|component_vec| component_vec.as_ref().unwrap().iter().copied())
            .collect();
        println!("{result:?}");

        assert_eq!(result, HashSet::from([1, 2, 3]))
    }

    #[test]
    fn borrowing_non_existent_component_returns_empty_vec() {
        // Arrange
        let (world, _, _, _, _) = setup_world_with_3_entities_with_u32_and_i32_components();

        // Act
        let vecs_f32 = world
            .borrow_component_vecs_with_signature::<f32>(&[TypeId::of::<f32>()])
            .unwrap();
        eprintln!("vecs_f32 = {vecs_f32:#?}");
        // Assert
        // Collect values from vecs
        let result: Vec<f32> = vecs_f32
            .iter()
            .flat_map(|component_vec| component_vec.as_ref().unwrap().iter().copied())
            .collect();
        println!("{result:?}");

        assert_eq!(result, Vec::<f32>::new())
    }

    #[test]
    #[should_panic]
    fn borrowing_component_vec_twice_from_archetype_causes_panic() {
        let mut archetype = Archetype::default();
        archetype.add_component_vec::<u32>();

        let borrow_1 = archetype.borrow_component_vec_mut::<u32>();
        let borrow_2 = archetype.borrow_component_vec_mut::<u32>();

        // Drop after both have been borrowed to make sure they both live this long.
        drop(borrow_1);
        drop(borrow_2);
    }

    #[test]
    fn borrowing_component_vec_after_reference_has_been_dropped_does_not_cause_panic() {
        let mut archetype = Archetype::default();
        archetype.add_component_vec::<u32>();

        let borrow_1 = archetype.borrow_component_vec_mut::<u32>();
        drop(borrow_1);

        let borrow_2 = archetype.borrow_component_vec_mut::<u32>();
        drop(borrow_2);
    }

    #[test]
    fn borrowing_two_different_component_vecs_from_archetype_does_not_cause_panic() {
        let mut archetype = Archetype::default();
        archetype.add_component_vec::<u32>();
        archetype.add_component_vec::<u64>();

        let a = archetype.borrow_component_vec_mut::<u32>();
        let b = archetype.borrow_component_vec_mut::<u64>();

        // Drop after both have been borrowed to make sure they both live this long.
        drop(a);
        drop(b);
    }

    #[test]
    fn get_entity_from_component_index() {
        let (world, _, _, _, _) = setup_world_with_3_entities_with_u32_and_i32_components();

        //Get the first entity added to world
        let comp_entity = world.entities.get(0).copied().unwrap();

        //Get the archetype index of the archetype that stores that entity
        let archetype = world.get_archetype_of_entity(comp_entity).unwrap();

        //Get the first entity stored in that archetype, check that it is the same
        let get_entity = archetype.get_entity(0).unwrap();

        assert_eq!(comp_entity, get_entity);
    }
}
