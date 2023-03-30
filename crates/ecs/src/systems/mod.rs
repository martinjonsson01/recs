//! Systems are one of the core parts of ECS, which are responsible for operating on data
//! in the form of [`crate::Entity`]s and components.

pub mod iteration;

use crate::systems::iteration::SequentiallyIterable;
use crate::{ArchetypeIndex, ReadComponentVec, World, WorldError, WriteComponentVec};
use paste::paste;
use std::any::TypeId;
use std::collections::HashSet;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::{any, fmt};
use thiserror::Error;

/// An error occurred during execution of a system.
#[derive(Error, Debug)]
pub enum SystemError {
    /// Could not execute system due to missing parameter.
    #[error("could not execute system due to missing parameter")]
    MissingParameter(#[source] SystemParameterError),
    /// The system cannot be iterated sequentially.
    #[error("the system cannot be iterated sequentially")]
    CannotRunSequentially,
}

/// Whether a system succeeded in its execution.
pub type SystemResult<T, E = SystemError> = Result<T, E>;

/// An executable unit of work that may operate on entities and their component data.
pub trait System: Debug + Send + Sync {
    /// What the system is called.
    fn name(&self) -> &str;
    /// Which component types the system accesses and in what manner (read/write).
    fn component_accesses(&self) -> Vec<ComponentAccessDescriptor>;
    /// See if the system can be executed sequentially, and if it can then transform it into one.
    fn try_as_sequentially_iterable(&self) -> Option<&dyn SequentiallyIterable>;
}

impl Display for dyn System + '_ {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl PartialEq<Self> for dyn System + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name() && self.component_accesses() == other.component_accesses()
    }
}

impl Eq for dyn System + '_ {}

impl Hash for dyn System + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name().hash(state);
        self.component_accesses().hash(state);
    }
}

/// What component is accessed and in what manner (read/write).
#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum ComponentAccessDescriptor {
    /// Reads from component of provided type.
    Read(TypeId, String),
    /// Reads and writes from component of provided type.
    Write(TypeId, String),
}

impl ComponentAccessDescriptor {
    /// Creates a [`ComponentAccessDescriptor`] that represents
    /// a read of the given [`ComponentType`].
    fn read<ComponentType: 'static>() -> Self {
        let type_id = TypeId::of::<ComponentType>();
        let type_name = any::type_name::<ComponentType>();
        Self::Read(type_id, type_name.to_owned())
    }

    /// Creates a [`ComponentAccessDescriptor`] that represents
    /// a read or write of the given [`ComponentType`].
    fn write<ComponentType: 'static>() -> Self {
        let type_id = TypeId::of::<ComponentType>();
        let type_name = any::type_name::<ComponentType>();
        Self::Write(type_id, type_name.to_owned())
    }

    /// The name of the type of component accessed.
    pub fn name(&self) -> &str {
        match self {
            ComponentAccessDescriptor::Read(_, name)
            | ComponentAccessDescriptor::Write(_, name) => name,
        }
    }

    /// Gets the inner component type.
    pub fn component_type(&self) -> TypeId {
        match &self {
            ComponentAccessDescriptor::Read(component_type, _)
            | ComponentAccessDescriptor::Write(component_type, _) => *component_type,
        }
    }

    /// Whether the access is mutable (read/write).
    pub fn is_write(&self) -> bool {
        match &self {
            ComponentAccessDescriptor::Read(_, _) => false,
            ComponentAccessDescriptor::Write(_, _) => true,
        }
    }

    /// Whether the access is immutable (read).
    pub fn is_read(&self) -> bool {
        !self.is_write()
    }
}

/// Something that can be turned into a `ecs::System`.
pub trait IntoSystem<Parameters> {
    /// What type of system is created.
    type Output: System + 'static;

    /// Turns `self` into an `ecs::System`.
    fn into_system(self) -> Self::Output;
}

/// A `ecs::System` represented by a Rust function/closure.
pub struct FunctionSystem<Function: Send + Sync, Parameters: SystemParameters> {
    pub(crate) function: Function,
    function_name: String,
    parameters: PhantomData<Parameters>,
}

impl<Function: Send + Sync, Parameters: SystemParameters> Debug
    for FunctionSystem<Function, Parameters>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let parameters_name = any::type_name::<Parameters>();
        let mut parameter_names_text = String::with_capacity(parameters_name.len());
        for parameter_name in parameters_name.split(',') {
            parameter_names_text.push_str(parameter_name);
        }

        f.debug_struct("FunctionSystem")
            .field("system", &self.function_name)
            .field("parameters", &parameter_names_text)
            .finish()
    }
}

impl<Function, Parameters> System for FunctionSystem<Function, Parameters>
where
    Function: SystemParameterFunction<Parameters> + Send + Sync + 'static,
    Parameters: SystemParameters,
    FunctionSystem<Function, Parameters>: SequentiallyIterable,
{
    fn name(&self) -> &str {
        &self.function_name
    }

    fn component_accesses(&self) -> Vec<ComponentAccessDescriptor> {
        Parameters::component_accesses()
    }

    fn try_as_sequentially_iterable(&self) -> Option<&dyn SequentiallyIterable> {
        Some(self)
    }
}

impl<Function, Parameters> IntoSystem<Parameters> for Function
where
    Function: SystemParameterFunction<Parameters> + Send + Sync + 'static,
    Parameters: SystemParameters + 'static,
    FunctionSystem<Function, Parameters>: SequentiallyIterable,
{
    type Output = FunctionSystem<Function, Parameters>;

    fn into_system(self) -> Self::Output {
        let function_name = get_function_name::<Function>();
        FunctionSystem {
            function: self,
            function_name,
            parameters: PhantomData,
        }
    }
}

fn get_function_name<Function>() -> String {
    let function_type_name = any::type_name::<Function>();
    let name_start_index = function_type_name
        .rfind("::")
        .map_or(0, |colon_index| colon_index + 2);
    let function_name = &function_type_name[name_start_index..];
    function_name.to_owned()
}

/// An error occurred during a world operation.
#[derive(Error, Debug)]
pub enum SystemParameterError {
    /// Could not find archetype with the given index.
    #[error("could not borrow component vecs with signature: {0:?}")]
    BorrowComponentVecs(WorldError),
}

/// Whether a world operation succeeded.
pub type SystemParameterResult<T, E = SystemParameterError> = Result<T, E>;

/// A collection of `ecs::SystemParameter`s that can be passed to a `ecs::System`.
pub trait SystemParameters: Send + Sync {
    /// A description of all of the data that is accessed and how (read/write).
    fn component_accesses() -> Vec<ComponentAccessDescriptor>;
}

/// Something that can be passed to a [`System`].
pub(crate) trait SystemParameter: Send + Sync + Sized {
    /// Contains a borrow of components from `ecs::World`.
    type BorrowedData<'components>;

    /// Borrows the collection of components of the given type from [`World`].
    fn borrow<'world>(
        world: &'world World,
        archetypes: &[ArchetypeIndex],
    ) -> SystemParameterResult<Self::BorrowedData<'world>>;

    /// Fetches the parameter from the borrowed data for a given entity.
    /// # Safety
    /// The returned value is only guaranteed to be valid until BorrowedData is dropped
    unsafe fn fetch_parameter(borrowed: &mut Self::BorrowedData<'_>) -> Option<Option<Self>>;

    /// A description of what data is accessed and how (read/write).
    fn component_access() -> Option<ComponentAccessDescriptor>;

    /// If the [`SystemParameter`] require that the system iterates over entities.
    /// The system will only run once if all [`SystemParameter`]'s `iterates_over_entities` is `false`.
    fn iterates_over_entities() -> bool;

    /// The `base_signature` is used for [`SystemParameter`]s that always require a component on the
    /// entity. This will be `None` for binary/unary filters.
    ///
    /// For example, if `(Read<A>, With<B>, With<C>)` is queried, then the `base_signature` will be
    /// the [`TypeId`]s of `{A, B, C}`. If instead `(Read<A>, Or<With<B>, With<C>>)` is queried,
    /// then it will just be the [`TypeId`]s of `{A}`. A set of the archetype indices that includes
    /// all components of the `base_signature` is created and this set is called the `universe`.
    /// The queried archetypes is find by taking the intersection of the `universe` and the filtered
    /// versions of the `universe` using the `filter` function.
    fn base_signature() -> Option<TypeId>;

    /// Perform a filter operation on a set of archetype indices.
    /// The `universe` is a set with all archetype indices used by the `base_signature`.
    fn filter(universe: &HashSet<ArchetypeIndex>, _world: &World) -> HashSet<ArchetypeIndex> {
        universe.clone()
    }
}

trait SystemParameterFunction<Parameters: SystemParameters>: 'static {}

type BorrowedArchetypeIndex = usize;
/// Index into a [`crate::ComponentVec`], where the component value of an entity is located.
pub type ComponentIndex = usize;

/// A read-only access to a component of the given type.
#[derive(Debug)]
pub struct Read<'a, Component: 'static> {
    output: &'a Component,
}

impl<'a, Component> Deref for Read<'a, Component> {
    type Target = Component;

    fn deref(&self) -> &Self::Target {
        self.output
    }
}

impl<Component: Debug + Send + Sync + 'static + Sized> SystemParameter for Read<'_, Component> {
    type BorrowedData<'components> = (
        BorrowedArchetypeIndex,
        Vec<(ComponentIndex, ReadComponentVec<'components, Component>)>,
    );

    fn borrow<'world>(
        world: &'world World,
        archetypes: &[ArchetypeIndex],
    ) -> SystemParameterResult<Self::BorrowedData<'world>> {
        let component_vecs = world
            .borrow_component_vecs::<Component>(archetypes)
            .map_err(SystemParameterError::BorrowComponentVecs)?;

        let component_vecs = component_vecs
            .into_iter()
            .map(|component_vec| (0, component_vec))
            .collect();

        Ok((0, component_vecs))
    }

    unsafe fn fetch_parameter(borrowed: &mut Self::BorrowedData<'_>) -> Option<Option<Self>> {
        let (ref mut current_archetype, archetypes) = borrowed;
        if let Some((component_index, Some(component_vec))) = archetypes.get_mut(*current_archetype)
        {
            return if let Some(component) = component_vec.get(*component_index) {
                *component_index += 1;
                Some(component.as_ref().map(|component| Self {
                    // The caller is responsible to only use the
                    // returned value when BorrowedData is still in scope.
                    #[allow(trivial_casts)]
                    output: &*(component as *const Component),
                }))
            } else {
                // End of archetype
                *current_archetype += 1;
                Self::fetch_parameter(borrowed)
            };
        }
        // No more entities
        None
    }

    fn component_access() -> Option<ComponentAccessDescriptor> {
        Some(ComponentAccessDescriptor::read::<Component>())
    }

    fn iterates_over_entities() -> bool {
        true
    }

    fn base_signature() -> Option<TypeId> {
        Some(TypeId::of::<Component>())
    }
}

impl<Component: Debug + Send + Sync + 'static + Sized> SystemParameter for Write<'_, Component> {
    type BorrowedData<'components> = (
        BorrowedArchetypeIndex,
        Vec<(ComponentIndex, WriteComponentVec<'components, Component>)>,
    );

    fn borrow<'world>(
        world: &'world World,
        archetypes: &[ArchetypeIndex],
    ) -> SystemParameterResult<Self::BorrowedData<'world>> {
        Ok((0, {
            let component_vecs = world
                .borrow_component_vecs_mut::<Component>(archetypes)
                .map_err(SystemParameterError::BorrowComponentVecs)?;

            component_vecs
                .into_iter()
                .map(|component_vec| (0, component_vec))
                .collect()
        }))
    }

    unsafe fn fetch_parameter(borrowed: &mut Self::BorrowedData<'_>) -> Option<Option<Self>> {
        let (ref mut current_archetype, archetypes) = borrowed;
        if let Some((ref mut component_index, Some(component_vec))) =
            archetypes.get_mut(*current_archetype)
        {
            return if let Some(ref mut component) = component_vec.get_mut(*component_index) {
                *component_index += 1;
                Some(component.as_mut().map(|component| Self {
                    // The caller is responsible to only use the
                    // returned value when BorrowedData is still in scope.
                    #[allow(trivial_casts)]
                    output: &mut *(component as *mut Component),
                }))
            } else {
                // End of archetype
                *current_archetype += 1;
                Self::fetch_parameter(borrowed)
            };
        }
        // No more entities
        None
    }

    fn component_access() -> Option<ComponentAccessDescriptor> {
        Some(ComponentAccessDescriptor::write::<Component>())
    }

    fn iterates_over_entities() -> bool {
        true
    }

    fn base_signature() -> Option<TypeId> {
        Some(TypeId::of::<Component>())
    }
}

/// A read-only access to a component of the given type.
#[derive(Debug)]
pub struct Write<'a, Component: 'static> {
    output: &'a mut Component,
}

impl<'a, Component> Deref for Write<'a, Component> {
    type Target = Component;

    fn deref(&self) -> &Self::Target {
        self.output
    }
}

impl<'a, Component> DerefMut for Write<'a, Component> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.output
    }
}

impl SystemParameters for () {
    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![]
    }
}

impl<F> SystemParameterFunction<()> for F where F: Fn() + 'static {}

macro_rules! impl_system_parameter_function {
    ($($parameter:expr),*) => {
        paste! {
            impl<$([<P$parameter>]: SystemParameter,)*> SystemParameters for ($([<P$parameter>],)*) {
                fn component_accesses() -> Vec<ComponentAccessDescriptor> {
                    [$([<P$parameter>]::component_access(),)*]
                        .into_iter()
                        .flatten()
                        .collect()
                }
            }

            impl<F, $([<P$parameter>]: SystemParameter,)*> SystemParameterFunction<($([<P$parameter>],)*)>
                for F where F: Fn($([<P$parameter>],)*) + 'static, {}
        }
    }
}

macro_rules! invoke_for_each_parameter_count {
    ($expression:ident) => {
        $expression!(0);
        $expression!(0, 1);
        $expression!(0, 1, 2);
        $expression!(0, 1, 2, 3);
        $expression!(0, 1, 2, 3, 4);
        $expression!(0, 1, 2, 3, 4, 5);
        $expression!(0, 1, 2, 3, 4, 5, 6);
        $expression!(0, 1, 2, 3, 4, 5, 6, 7);
        $expression!(0, 1, 2, 3, 4, 5, 6, 7, 8);
        $expression!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9);
        $expression!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
        $expression!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11);
        $expression!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12);
        $expression!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13);
        $expression!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14);
        $expression!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
    };
}

// So it can be accessed from other modules such as `iteration`.
pub(crate) use invoke_for_each_parameter_count;

invoke_for_each_parameter_count!(impl_system_parameter_function);

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;
    use test_utils::{A, B, C};

    #[test_case(|_: Read<A>| {}, vec![ComponentAccessDescriptor::read::<A>()]; "when reading")]
    #[test_case(|_: Write<A>| {}, vec![ComponentAccessDescriptor::write::<A>()]; "when writing")]
    #[test_case(|_: Read<A>, _:Read<B>| {}, vec![ComponentAccessDescriptor::read::<A>(), ComponentAccessDescriptor::read::<B>()]; "when reading two components")]
    #[test_case(|_: Write<A>, _:Write<B>| {}, vec![ComponentAccessDescriptor::write::<A>(), ComponentAccessDescriptor::write::<B>()]; "when writing two components")]
    #[test_case(|_: Read<A>, _: Write<B>| {}, vec![ComponentAccessDescriptor::read::<A>(), ComponentAccessDescriptor::write::<B>()]; "when reading and writing to components")]
    #[test_case(|_: Read<A>, _: Read<B>, _: Read<C>| {}, vec![ComponentAccessDescriptor::read::<A>(), ComponentAccessDescriptor::read::<B>(), ComponentAccessDescriptor::read::<C>()]; "when reading three components")]
    fn component_accesses_return_actual_component_accesses<Params>(
        system: impl IntoSystem<Params>,
        expected_accesses: Vec<ComponentAccessDescriptor>,
    ) {
        let component_accesses = system.into_system().component_accesses();

        assert_eq!(expected_accesses, component_accesses)
    }
}
