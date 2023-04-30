use ecs::systems::command_buffers::{CommandBuffer, CommandReceiver};
use ecs::systems::iteration::{SegmentIterable, SequentiallyIterable};
use ecs::systems::{ComponentAccessDescriptor, IntoSystem, Read, System, SystemParameters, Write};
use proptest::collection::hash_set;
use proptest::prop_compose;
use std::any::TypeId;
use std::fmt::{Debug, Display, Formatter};

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct A(pub i32);
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct B(pub String);
#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub struct C(pub f32);
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct D;
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct E;
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct F;
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct G;
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct H;

pub fn read_a(_: Read<A>) {}
pub fn read_b(_: Read<B>) {}
pub fn read_c(_: Read<C>) {}
pub fn other_read_a(_: Read<A>) {}
pub fn write_a(_: Write<A>) {}
pub fn write_b(_: Write<B>) {}
pub fn write_c(_: Write<C>) {}
pub fn other_write_a(_: Write<A>) {}

pub fn read_b_write_a(_: Read<B>, _: Write<A>) {}
pub fn read_a_write_c(_: Read<A>, _: Write<C>) {}
pub fn read_c_write_b(_: Read<C>, _: Write<B>) {}
pub fn read_a_write_b(_: Read<A>, _: Write<B>) {}

pub fn read_ab(_: Read<A>, _: Read<B>) {}
pub fn write_ab(_: Write<A>, _: Write<B>) {}

pub fn into_system<F: IntoSystem<Parameters>, Parameters: SystemParameters>(
    function: F,
) -> Box<dyn System> {
    Box::new(function.into_system())
}

#[derive(Clone)]
struct MockSystem {
    name: String,
    parameters: Vec<ComponentAccessDescriptor>,
}

impl Display for MockSystem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

impl Debug for MockSystem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockSystem")
            .field("name", &self.name)
            .field("parameters", &"see below")
            .finish()?;

        f.debug_list().entries(&self.parameters).finish()
    }
}

impl System for MockSystem {
    fn name(&self) -> &str {
        &self.name
    }

    fn component_accesses(&self) -> Vec<ComponentAccessDescriptor> {
        self.parameters.clone()
    }

    fn try_as_sequentially_iterable(&self) -> Option<Box<dyn SequentiallyIterable>> {
        None
    }

    fn try_as_segment_iterable(&self) -> Option<Box<dyn SegmentIterable>> {
        None
    }

    fn command_buffer(&self) -> CommandBuffer {
        unimplemented!()
    }

    fn command_receiver(&self) -> CommandReceiver {
        unimplemented!()
    }
}

prop_compose! {
    pub fn arb_component_type()
                             (type_index in 0_usize..8)
                             -> (TypeId, &'static str) {
        // Hardcoded names instead of any::type_name to avoid module prefix.
        let types = vec![
            (TypeId::of::<A>(), "A"),
            (TypeId::of::<B>(), "B"),
            (TypeId::of::<C>(), "C"),
            (TypeId::of::<D>(), "D"),
            (TypeId::of::<E>(), "E"),
            (TypeId::of::<F>(), "F"),
            (TypeId::of::<G>(), "G"),
            (TypeId::of::<H>(), "H"),
        ];
        types[type_index]
    }
}

prop_compose! {
    pub fn arb_component_access()
                               (write in proptest::arbitrary::any::<bool>(),
                                (type_id, type_name) in arb_component_type())
                               -> ComponentAccessDescriptor {
        if write {
            ComponentAccessDescriptor::Write(type_id, type_name.to_owned())
        } else {
            ComponentAccessDescriptor::Read(type_id, type_name.to_owned())
        }
    }
}

prop_compose! {
    #[allow(trivial_casts)] // Compiler won't coerce `MockSystem` to `dyn System` for some reason.
    pub fn arb_system()
                     (parameters in hash_set(arb_component_access(), 1..8))
                     -> Box<dyn System> {
        let parameters: Vec<_> = parameters.into_iter().collect();
        Box::new(MockSystem {
            name: system_name_from_accesses(parameters.clone()),
            parameters,
        }) as Box<dyn System>
    }
}

fn system_name_from_accesses(accesses: Vec<ComponentAccessDescriptor>) -> String {
    let mut name = String::new();

    let read = access_name(&accesses, "read_", |access| access.is_read());
    let write = access_name(&accesses, "write_", |access| access.is_write());
    name.push_str(&read);
    if !read.is_empty() && !write.is_empty() {
        name.push('_');
    }
    name.push_str(&write);

    name
}

fn access_name(
    accesses: &[ComponentAccessDescriptor],
    prefix: &'static str,
    filter: fn(&&ComponentAccessDescriptor) -> bool,
) -> String {
    let mut name = String::new();
    let filtered_accesses: Vec<_> = accesses.iter().filter(filter).collect();
    if !filtered_accesses.is_empty() {
        name.push_str(prefix);
        let mut reads = filtered_accesses.iter().peekable();
        while let Some(access) = reads.next() {
            name.push_str(access.name());
            if reads.peek().is_some() {
                name.push('_');
            }
        }
    }
    name
}

prop_compose! {
    pub fn arb_systems(min_system_count: usize, max_system_count: usize)
                      (systems in hash_set(arb_system(), min_system_count..=max_system_count))
                      -> Vec<Box<dyn System>> {
        systems.into_iter().collect()
    }
}
