use ecs::{ComponentAccessDescriptor, System, World};
use proptest::collection::hash_set;
use proptest::prop_compose;
use std::any::TypeId;

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
