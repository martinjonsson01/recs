use ecs::{ComponentAccessDescriptor, IntoSystem, Read, System, SystemParameters, World, Write};
use proptest::collection::hash_set;
use proptest::prop_compose;
use std::any::TypeId;

#[derive(Debug, Default)]
pub struct A(i32);
#[derive(Debug, Default)]
pub struct B(String);
#[derive(Debug, Default)]
pub struct C(f32);

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

pub fn read_a_write_b(_: Read<A>, _: Write<B>) {}
pub fn read_ab(_: Read<A>, _: Read<B>) {}

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
                             -> TypeId {
        let types = vec![
            TypeId::of::<u8>(),
            TypeId::of::<i8>(),
            TypeId::of::<u16>(),
            TypeId::of::<i16>(),
            TypeId::of::<u32>(),
            TypeId::of::<i32>(),
            TypeId::of::<u64>(),
            TypeId::of::<i64>(),
        ];
        types[type_index]
    }
}

prop_compose! {
    pub fn arb_component_access()
                               (write in proptest::arbitrary::any::<bool>(),
                                type_id in arb_component_type())
                               -> ComponentAccessDescriptor {
        if write {
            ComponentAccessDescriptor::Write(type_id)
        } else {
            ComponentAccessDescriptor::Read(type_id)
        }
    }
}

prop_compose! {
    #[allow(trivial_casts)] // Compiler won't coerce `MockSystem` to `dyn System` for some reason.
    pub fn arb_system()
                     (name in proptest::arbitrary::any::<String>(),
                      parameters in proptest::collection::vec(arb_component_access(), 1..8))
                     -> Box<dyn System> {
        Box::new(MockSystem {
            name,
            parameters,
        }) as Box<dyn System>
    }
}

prop_compose! {
    pub fn arb_systems(min_system_count: usize, max_system_count: usize)
                      (systems_count in min_system_count..=max_system_count)
                      (systems in hash_set(arb_system(), systems_count))
                      -> Vec<Box<dyn System>> {
        systems.into_iter().collect()
    }
}
