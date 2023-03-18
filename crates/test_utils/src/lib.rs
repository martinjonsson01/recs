use ecs::{ComponentAccessDescriptor, IntoSystem, Read, System, SystemParameters, World, Write};
use proptest::collection::hash_set;
use proptest::prop_compose;
use std::any::TypeId;
use std::fmt::{Display, Formatter};

#[derive(Debug, Default)]
pub struct A(i32);
#[derive(Debug, Default)]
pub struct B(String);
#[derive(Debug, Default)]
pub struct C(f32);
#[derive(Debug, Default)]
pub struct D;
#[derive(Debug, Default)]
pub struct E;
#[derive(Debug, Default)]
pub struct F;
#[derive(Debug, Default)]
pub struct G;
#[derive(Debug, Default)]
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

#[derive(Debug)]
struct MockSystem {
    name: String,
    parameters: Vec<ComponentAccessDescriptor>,
}

impl Display for MockSystem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

impl System for MockSystem {
    fn name(&self) -> &str {
        &self.name
    }

    fn run(&self, _world: &World) {
        panic!("mocked system, not meant to be run")
    }

    fn component_accesses(&self) -> Vec<ComponentAccessDescriptor> {
        self.parameters.clone()
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

    name.push_str("read_");
    for access in accesses.iter().filter(|access| access.is_read()) {
        name.push_str(access.name());
        name.push('_');
    }
    name.push_str("write_");
    for access in accesses.iter().filter(|access| access.is_write()) {
        name.push_str(access.name());
        name.push('_');
    }

    name
}

prop_compose! {
    pub fn arb_systems(min_system_count: usize, max_system_count: usize)
                      (systems_count in min_system_count..=max_system_count)
                      (systems in hash_set(arb_system(), systems_count))
                      -> Vec<Box<dyn System>> {
        systems.into_iter().collect()
    }
}
