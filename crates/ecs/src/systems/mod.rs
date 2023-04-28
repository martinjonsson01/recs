//! Systems are one of the core parts of ECS, which are responsible for operating on data
//! in the form of [`Entity`]s and components.

pub mod iteration;

use crate::systems::iteration::{ParallelIterable, SequentiallyIterable};
use crate::{
    intersection_of_multiple_sets, ArchetypeIndex, Entity, ReadComponentVec, World, WorldError,
    WriteComponentVec,
};
use itertools::{cloned, izip};
use paste::paste;
use std::any::TypeId;
use std::collections::HashSet;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::iter::{Flatten, Map};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::vec::IntoIter;
use std::{any, cmp, fmt, iter};
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
    /// See if the system can be executed in parallel, and if it can then transform it into one.
    fn try_as_parallel_iterable(&self) -> Option<&dyn ParallelIterable>;
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
    Parameters: SystemParameters + 'static,
    FunctionSystem<Function, Parameters>: SequentiallyIterable + ParallelIterable,
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

    fn try_as_parallel_iterable(&self) -> Option<&dyn ParallelIterable> {
        Parameters::supports_parallelization().then_some(self)
    }
}

impl<Function, Parameters> IntoSystem<Parameters> for Function
where
    Function: SystemParameterFunction<Parameters> + Send + Sync + 'static,
    Parameters: SystemParameters + 'static,
    FunctionSystem<Function, Parameters>: SequentiallyIterable + ParallelIterable,
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
    /// The type of borrowed data for all system parameters
    type BorrowedData<'components>;

    /// The type of segments for all system parameters
    type SegmentData: Send + Sync + Clone;

    /// A description of all of the data that is accessed and how (read/write).
    fn component_accesses() -> Vec<ComponentAccessDescriptor>;

    /// If a system with these [`SystemParameters`] can use intra-system parallelization.
    fn supports_parallelization() -> bool;

    /// Get archetypes used by the system parameters
    fn get_archetype_indices(world: &World) -> Vec<ArchetypeIndex>;
}

/// Something that can be passed to a [`System`].
pub trait SystemParameter: Send + Sync + Sized
//where for<'b> &'b mut Self::SegmentData: IntoIterator<Item=Self>,
{
    /// Contains components borrowed from `ecs::World`.
    type BorrowedData<'components>;

    /// The type for segments that reference [`BorrowedData`](Self::BorrowedData).
    /// One segment is created for each thread.
    type SegmentData: Send + Sync + IntoIterator<Item = Self> + Clone;

    /// Borrows the collection of components of the given type from [`World`].
    fn borrow<'world>(
        world: &'world World,
        archetypes: &[ArchetypeIndex],
    ) -> SystemParameterResult<Self::BorrowedData<'world>>;

    /// Split the borrowed data to different segments. There will always be at least one segment.
    fn split_borrowed_data<'borrowed>(
        borrowed: &'borrowed mut Self::BorrowedData<'_>,
        segment: FixedSegment,
    ) -> Vec<Self::SegmentData>;

    /// Fetches the parameter from the segment data for a given entity.
    /// This should be implemented for all [`SystemParameter`] where [`iterates_over_entities`](Self::iterates_over_entities) is true.
    /// # Safety
    /// The returned value is only guaranteed to be valid until BorrowedData is dropped
    unsafe fn fetch_parameter_for_entity(
        segment: &mut Self::SegmentData,
        archetype: BorrowedArchetypeIndex,
        component_index: ComponentIndex,
    ) -> Option<Option<Self>> {
        let (_, _, _) = (segment, archetype, component_index);
        None
    }

    /// A description of what data is accessed and how (read/write).
    fn component_accesses() -> Vec<ComponentAccessDescriptor>;

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
    /// The queried archetypes are found by taking the intersection of the `universe` and the filtered
    /// versions of the `universe` using the `filter` function.
    fn base_signature() -> Option<TypeId>;

    /// Perform a filter operation on a set of archetype indices.
    /// The `universe` is a set with all archetype indices used by the `base_signature`.
    fn filter(universe: &HashSet<ArchetypeIndex>, world: &World) -> HashSet<ArchetypeIndex> {
        let _ = world;
        universe.clone()
    }

    /// If a system with this [`SystemParameter`] can use intra-system parallelization.
    fn support_parallelization() -> bool {
        true
    }
}

trait SystemParameterFunction<Parameters: SystemParameters>: 'static {}

type BorrowedArchetypeIndex = usize;
/// Index into a [`crate::ComponentVec`], where the component value of an entity is located.
pub type ComponentIndex = usize;

/// Description of the segments used when fetching parameters
#[derive(Debug, Copy, Clone)]
pub enum Segment {
    /// Create a single segment
    Single,
    /// Create segments with a given maximum size
    Size(u32),
    /// Automatically determine segment size
    Auto,
}

/// Segment size and count calculated using [`Segment`], entity count and thread count
#[derive(Debug, Copy, Clone)]
pub enum FixedSegment {
    /// Create a single segment
    Single,
    /// Create [`segment_count`](Self::Size::segment_count) segments with a maximum segment size of [`segment_size`](Self::Size::segment_size)
    Size {
        /// The size of all segments (except the last one)
        segment_size: usize,
        /// The number of segments
        segment_count: usize,
    },
}

const MINIMUM_ENTITIES_PER_SEGMENT: usize = 25;
const SEGMENTS_PER_THREAD: usize = 4;

fn calculate_auto_segment_size(entity_count: usize) -> usize {
    let thread_count = rayon::current_num_threads();
    cmp::max(
        MINIMUM_ENTITIES_PER_SEGMENT,
        entity_count / (thread_count * SEGMENTS_PER_THREAD),
    )
}

fn calculate_segment_count(entity_count: usize, segment_size: usize) -> usize {
    if entity_count != 0 {
        1 + (entity_count - 1) / segment_size
    } else {
        1
    }
}

pub(crate) fn unit_segments(segment: FixedSegment) -> Vec<()> {
    match segment {
        FixedSegment::Single => {
            vec![()]
        }
        FixedSegment::Size {
            segment_size: _,
            segment_count,
        } => vec![(); segment_count],
    }
}

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

pub struct ReadSegment<'components, Component: Debug + Send + Sync + 'static + Sized> {
    slices: Vec<&'components [Option<Component>]>,
}

impl<'components, Component: Debug + Send + Sync + 'static + Sized> Clone
    for ReadSegment<'components, Component>
{
    fn clone(&self) -> Self {
        Self {
            slices: self.slices.clone(),
        }
    }
}

impl<'segment, Component: Debug + Send + Sync + 'static + Sized> IntoIterator
    for ReadSegment<'segment, Component>
{
    type Item = Read<'segment, Component>;
    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.slices
            .into_iter()
            .flatten()
            .flatten()
            .map(|component| Read { output: component })
    }
}
/*
impl<'a, Component: Debug + Send + Sync + 'static + Sized> IntoIterator
for &'a mut ReadSegment<'static, Component>
{
    type Item = Read<'a, Component>;
    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.slices
            .into_iter()
            .flatten()
            .flatten()
            .map(|component| Read { output: component })
    }
}*/

impl<'a, Component: Debug + Send + Sync + 'static + Sized> SystemParameter for Read<'a, Component> {
    type BorrowedData<'components> = Vec<ReadComponentVec<'components, Component>>;
    type SegmentData = ReadSegment<'a, Component>;

    fn borrow<'world>(
        world: &'world World,
        archetypes: &[ArchetypeIndex],
    ) -> SystemParameterResult<Self::BorrowedData<'world>> {
        let component_vecs = world
            .borrow_component_vecs::<Component>(archetypes)
            .map_err(SystemParameterError::BorrowComponentVecs)?;

        Ok(component_vecs)
    }

    fn split_borrowed_data<'borrowed>(
        borrowed: &'borrowed mut Self::BorrowedData<'_>,
        segment: FixedSegment,
    ) -> Vec<Self::SegmentData> {
        let (segment_size, segment_count) = match segment {
            FixedSegment::Single => {
                return vec![ReadSegment {
                    slices: borrowed
                        .iter()
                        .flatten()
                        .map(|component_vec| unsafe {
                            std::mem::transmute(component_vec.as_slice())
                        })
                        .collect::<Vec<_>>(),
                }];
            }
            FixedSegment::Size {
                segment_size,
                segment_count,
            } => (segment_size, segment_count),
        };
        let mut segments = Vec::with_capacity(segment_count);

        let mut current_segment_size = 0;
        let mut current_segment = vec![];

        for component_vec in borrowed.iter().flatten() {
            if segment_size - current_segment_size > component_vec.len() {
                current_segment_size += component_vec.len();
                current_segment.push(component_vec.as_slice());
                continue;
            }

            let (left, right) = component_vec.split_at(segment_size - current_segment_size);

            if !left.is_empty() {
                current_segment.push(left);
                segments.push(current_segment);
                current_segment = vec![];
                current_segment_size = 0;
            }

            let chunks = right.chunks_exact(segment_size);

            let remainder = chunks.remainder();
            if !remainder.is_empty() {
                current_segment_size += remainder.len();
                current_segment.push(remainder);
            }

            for chunk in chunks {
                segments.push(vec![chunk]);
            }
        }

        if current_segment_size > 0 || segments.is_empty() {
            segments.push(current_segment);
        }

        segments
            .into_iter()
            .map(|segment| ReadSegment {
                slices: unsafe { std::mem::transmute(segment) },
            })
            .collect()
    }

    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![ComponentAccessDescriptor::read::<Component>()]
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

pub struct WriteSegment<'components, Component: Debug + Send + Sync + 'static + Sized> {
    slices: Vec<&'components mut [Option<Component>]>,
}

impl<'components, Component: Debug + Send + Sync + 'static + Sized> Clone
    for WriteSegment<'components, Component>
{
    fn clone(&self) -> Self {
        panic!("Cloning a segment for a mutable SystemParameter is not allowed.")
    }
}

impl<'components, Component: Debug + Send + Sync + 'static + Sized> IntoIterator
    for WriteSegment<'components, Component>
{
    type Item = Write<'components, Component>;
    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.slices
            .into_iter()
            .flatten()
            .flatten()
            .map(|component| Write { output: component })
    }
}

impl<'a, Component: Debug + Send + Sync + 'static + Sized> SystemParameter
    for Write<'a, Component>
{
    type BorrowedData<'components> = Vec<WriteComponentVec<'components, Component>>;
    type SegmentData = WriteSegment<'a, Component>;

    fn borrow<'world>(
        world: &'world World,
        archetypes: &[ArchetypeIndex],
    ) -> SystemParameterResult<Self::BorrowedData<'world>> {
        let component_vecs = world
            .borrow_component_vecs_mut::<Component>(archetypes)
            .map_err(SystemParameterError::BorrowComponentVecs)?;

        Ok(component_vecs)
    }

    fn split_borrowed_data<'borrowed>(
        borrowed: &'borrowed mut Self::BorrowedData<'_>,
        segment: FixedSegment,
    ) -> Vec<Self::SegmentData> {
        let (segment_size, segment_count) = match segment {
            FixedSegment::Single => {
                return vec![WriteSegment {
                    slices: borrowed
                        .iter_mut()
                        .flatten()
                        .map(|component_vec| unsafe {
                            std::mem::transmute(component_vec.as_mut_slice())
                        })
                        .collect::<Vec<_>>(),
                }];
            }
            FixedSegment::Size {
                segment_size,
                segment_count,
            } => (segment_size, segment_count),
        };
        let mut segments = Vec::with_capacity(segment_count);

        let mut current_segment_size = 0;
        let mut current_segment = vec![];

        for component_vec in borrowed.iter_mut().flatten() {
            if segment_size - current_segment_size > component_vec.len() {
                current_segment_size += component_vec.len();
                current_segment.push(component_vec.as_mut_slice());
                continue;
            }

            let (left, right) = component_vec.split_at_mut(segment_size - current_segment_size);

            if !left.is_empty() {
                current_segment.push(left);
                segments.push(current_segment);
                current_segment = vec![];
                current_segment_size = 0;
            }

            let chunks = right.chunks_mut(segment_size);
            for chunk in chunks {
                if chunk.len() == segment_size {
                    segments.push(vec![chunk]);
                } else {
                    current_segment_size += chunk.len();
                    current_segment.push(chunk);
                }
            }
        }

        if current_segment_size > 0 || segments.is_empty() {
            segments.push(current_segment);
        }

        segments
            .into_iter()
            .map(|segment| WriteSegment {
                slices: unsafe { std::mem::transmute(segment) },
            })
            .collect()
    }

    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![ComponentAccessDescriptor::write::<Component>()]
    }

    fn iterates_over_entities() -> bool {
        true
    }

    fn base_signature() -> Option<TypeId> {
        Some(TypeId::of::<Component>())
    }
}

/// `Query` allows a system to access components from entities other than the currently iterated.
///
/// # Example
/// ```
/// # use ecs::filter::With;
/// # use ecs::systems::{Query, Read, Write};
/// # #[derive(Debug)] struct Mass(f32);
/// # #[derive(Debug)] struct Velocity(f32);
/// fn print_largest_momentum(query: Query<(Read<Mass>, Read<Velocity>)>) {
///     let mut largest_momentum = 0.0;
///     for (mass, velocity) in query {
///         let momentum = mass.0 * velocity.0;
///         if momentum > largest_momentum {
///             largest_momentum = momentum;
///         }
///     }
///     println!("The largest momentum of all entities is {largest_momentum} kg*m/s.");
/// }
/// ```
pub struct Query<'segment, P: SystemParameters> {
    segments: &'segment mut P::SegmentData,
    //world: &'segment World,
    //archetypes: Vec<ArchetypeIndex>,
    iterate_over_entities: bool,
    //iterated_once: bool,
}

impl<'world, P: Debug + SystemParameters> Debug for Query<'world, P> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Query<{}>", any::type_name::<P>())
    }
}

pub struct QuerySegment<'segment, P: SystemParameters> {
    /// fn system(query: Query<(Read<A>, Read<B>)>)
    /// borrow -> [A] [B]
    /// split ->
    phantom: PhantomData<&'segment P>,
    segment: P::SegmentData,
}

impl<'a, P: SystemParameters> Clone for QuerySegment<'a, P> {
    fn clone(&self) -> Self {
        Self {
            phantom: PhantomData::default(),
            segment: self.segment.clone(),
        }
    }
}

impl SystemParameters for () {
    type BorrowedData<'components> = ();
    type SegmentData = ();

    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![]
    }

    fn supports_parallelization() -> bool {
        // No reason to parallelize iteration over nothing.
        false
    }

    fn get_archetype_indices(_: &World) -> Vec<ArchetypeIndex> {
        vec![]
    }
}

impl<F> SystemParameterFunction<()> for F where F: Fn() + 'static {}

macro_rules! impl_system_parameter_function {
    ($($parameter:expr),*) => {
        paste! {
            impl<$([<P$parameter>]: SystemParameter,)*> SystemParameters for ($([<P$parameter>],)*) {
                type BorrowedData<'components> = ($([<P$parameter>]::BorrowedData<'components>,)*);
                type SegmentData =($(Vec<[<P$parameter>]::SegmentData>,)*);

                fn component_accesses() -> Vec<ComponentAccessDescriptor> {
                    [$([<P$parameter>]::component_accesses(),)*]
                        .into_iter()
                        .flatten()
                        .collect()
                }

                fn supports_parallelization() -> bool {
                    [$([<P$parameter>]::support_parallelization(),)*]
                        .into_iter()
                        .all(|supports_parallelization| supports_parallelization)
                }

                fn get_archetype_indices(world: &World) -> Vec<ArchetypeIndex> {
                    let base_signature: Vec<TypeId> = [$([<P$parameter>]::base_signature(),)*]
                        .into_iter()
                        .flatten()
                        .collect();

                    let universe = world.get_archetype_indices(&base_signature);

                    intersection_of_multiple_sets(&[
                        universe.clone(),
                        $([<P$parameter>]::filter(&universe, world),)*
                    ])
                    .into_iter()
                    .collect()
                }
            }

            /*impl<'a, $([<P$parameter>]: SystemParameter,)*> Query<'a, ($([<P$parameter>],)*)> {
                /// Get the queried data from a specific entity.
                pub fn get_entity(&mut self, entity: Entity) -> Option<($([<P$parameter>],)*)> {
                    let archetype_index = self.world.entity_to_archetype_index.get(&entity)?;
                    let archetype = self.world.archetypes.get(*archetype_index)?;
                    let component_index = archetype.entity_to_component_index.get(&entity)?;

                    let borrowed_archetype_index = self
                        .archetypes
                        .iter()
                        .position(|&element| element == *archetype_index)?;

                    // SAFETY: This is safe because the result from fetch_parameter_for_entity will not outlive self.segments
                    unsafe {
                        if let ($(Some(Some([<parameter_$parameter>])),)*) = (
                            $([<P$parameter>]::fetch_parameter_for_entity(&mut self.segments.$parameter[0], borrowed_archetype_index, *component_index),)*
                        ) {
                            return Some(($([<parameter_$parameter>],)*));
                        }
                    }
                    None
                }
            }*/

            impl<'a, $([<P$parameter>]: SystemParameter + 'a,)*> SystemParameter for Query<'a, ($([<P$parameter>],)*)> {
                type BorrowedData<'components> = (
                    <($([<P$parameter>],)*) as SystemParameters>::BorrowedData<'components>,
                    &'components World,
                    Vec<ArchetypeIndex>,
                );

                type SegmentData = QuerySegment<'a, ($([<P$parameter>],)*)>;
                /*(
                    <($([<P$parameter>],)*) as SystemParameters>::SegmentData<'components>,
                    &'components World,
                    Vec<ArchetypeIndex>,
                );*/

                fn borrow<'world>(
                    world: &'world World,
                    _: &[ArchetypeIndex],
                ) -> SystemParameterResult<Self::BorrowedData<'world>> {
                    let archetypes = <($([<P$parameter>],)*) as SystemParameters>::get_archetype_indices(world);

                    Ok((
                        ($([<P$parameter>]::borrow(world, &archetypes)?,)*),
                        world,
                        archetypes,
                    ))
                }

                fn split_borrowed_data<'borrowed>(
                    borrowed: &'borrowed mut Self::BorrowedData<'_>,
                    segment: FixedSegment,
                ) -> Vec<Self::SegmentData> {
                    let ($([<segment_$parameter>],)*) = (
                        $([<P$parameter>]::split_borrowed_data(&mut borrowed.0.$parameter, FixedSegment::Single),)*
                    );

                    match segment {
                        FixedSegment::Single => {
                            vec![
                                (QuerySegment {
                                    phantom: PhantomData::default(),
                                    segment: ($([<segment_$parameter>],)*),
                                }),
                            ]
                        }
                        FixedSegment::Size { segment_count, .. } => {
                            let mut segments = Vec::with_capacity(segment_count);

                            for _ in 0..segment_count {
                                segments.push(QuerySegment {
                                    phantom: PhantomData::default(),
                                    segment: ($([<segment_$parameter>].clone(),)*),
                                });
                            }

                            segments
                        }
                    }
                }

                fn component_accesses() -> Vec<ComponentAccessDescriptor> {
                    <($([<P$parameter>],)*) as SystemParameters>::component_accesses()
                }

                fn iterates_over_entities() -> bool {
                    false
                }

                fn base_signature() -> Option<TypeId> {
                    None
                }

                fn support_parallelization() -> bool {
                    <($([<P$parameter>],)*) as SystemParameters>::component_accesses()
                        .iter()
                        .all(|component_access| component_access.is_read())
                }
            }

            impl<'a, $([<P$parameter>]: SystemParameter,)*> IntoIterator for QuerySegment<'a, ($([<P$parameter>],)*)> {
                type Item = Query<'a, ($([<P$parameter>],)*)>;
                type IntoIter = impl Iterator<Item=Self::Item>;

                fn into_iter(mut self) -> Self::IntoIter {
                    iter::from_fn(move || {
                        Some(Query {
                            segments: unsafe { std::mem::transmute(&mut self.segment) },
                            iterate_over_entities: $([<P$parameter>]::iterates_over_entities())||*,
                        })
                    })
                }
            }

            impl<'a, $([<P$parameter>]: SystemParameter + 'a,)*> IntoIterator for Query<'a, ($([<P$parameter>],)*)> {
                type Item = ($([<P$parameter>],)*);
                type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;

                fn into_iter(self) -> Self::IntoIter {
                    $(let [<segment_$parameter>] = self.segments.$parameter[0].clone().into_iter();)*

                    let mut iter = izip_tuple!($([<segment_$parameter>]),*);
                    if self.iterate_over_entities {
                        Box::new(iter)
                    } else {
                        let parameters = iter.next().unwrap();
                        Box::new(iter::once(parameters))
                    }
                }
            }

            impl<F, $([<P$parameter>]: SystemParameter,)*> SystemParameterFunction<($([<P$parameter>],)*)>
                for F where F: Fn($([<P$parameter>],)*) + 'static, {}
        }
    }
}

macro_rules! izip_tuple {
    ($x:expr) => {
        $x.map(|x| (x,))
    };
    ($($x:expr),+) => {
        izip!($($x),+)
    };
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

impl_system_parameter_function!(0);
impl_system_parameter_function!(0, 1);
impl_system_parameter_function!(0, 1, 2);
impl_system_parameter_function!(0, 1, 2, 3);
impl_system_parameter_function!(0, 1, 2, 3, 4);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);

//invoke_for_each_parameter_count!(impl_system_parameter_function);

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::prop_compose;
    use std::sync::RwLock;
    use test_case::test_case;
    use test_strategy::proptest;
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

    prop_compose! {
        fn arb_component_vecs()(vecs in prop::collection::vec(prop::collection::vec(1..=1, 1..10), 1..10))
            -> Vec<RwLock<Vec<Option<i32>>>> {
            vecs.into_iter().map(|vec|
                RwLock::new(vec.into_iter().map(Some).collect::<Vec<_>>())
            ).collect::<Vec<_>>()
        }
    }

    #[proptest]
    fn split_borrowed_read_data_returns_segments_with_correct_length(
        #[strategy(arb_component_vecs())] component_vecs: Vec<RwLock<Vec<Option<i32>>>>,
        #[strategy(1..100_usize)] segment_size: usize,
    ) {
        let borrowed: Vec<_> = component_vecs
            .iter()
            .map(|vec| vec.try_read().unwrap())
            .collect();
        let mut component_vecs: Vec<_> = borrowed.into_iter().map(Some).collect();

        let total_length: usize = component_vecs.iter().flatten().map(|vec| vec.len()).sum();

        let segments = <Read<i32> as SystemParameter>::split_borrowed_data(
            &mut component_vecs,
            FixedSegment::Size {
                segment_size,
                segment_count: calculate_segment_count(total_length, segment_size),
            },
        );

        let (last_segment, other_segments) = segments.split_last().unwrap();

        for segment in other_segments {
            let (_, archetypes) = segment;
            let component_count: usize = archetypes.iter().map(|(_, slice)| slice.len()).sum();
            assert_eq!(component_count, segment_size);
        }

        let (_, archetypes) = last_segment;
        let component_count: usize = archetypes.iter().map(|(_, slice)| slice.len()).sum();
        assert_eq!(component_count, 1 + (total_length - 1) % (segment_size));
    }

    #[proptest]
    fn split_borrowed_write_data_returns_segments_with_correct_length(
        #[strategy(arb_component_vecs())] component_vecs: Vec<RwLock<Vec<Option<i32>>>>,
        #[strategy(1..100_usize)] segment_size: usize,
    ) {
        let borrowed: Vec<_> = component_vecs
            .iter()
            .map(|vec| vec.try_write().unwrap())
            .collect();
        let mut component_vecs: Vec<_> = borrowed.into_iter().map(Some).collect();

        let total_length: usize = component_vecs.iter().flatten().map(|vec| vec.len()).sum();

        let segments = <Write<i32> as SystemParameter>::split_borrowed_data(
            &mut component_vecs,
            FixedSegment::Size {
                segment_size,
                segment_count: calculate_segment_count(total_length, segment_size),
            },
        );

        let (last_segment, other_segments) = segments.split_last().unwrap();

        for segment in other_segments {
            let (_, archetypes) = segment;
            let component_count: usize = archetypes.iter().map(|(_, slice)| slice.len()).sum();
            assert_eq!(component_count, segment_size);
        }

        let (_, archetypes) = last_segment;
        let component_count: usize = archetypes.iter().map(|(_, slice)| slice.len()).sum();
        assert_eq!(component_count, 1 + (total_length - 1) % (segment_size));
    }
}
