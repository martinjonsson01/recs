
use std::{ops::{Index, IndexMut}, any::{Any, TypeId}, collections::{HashMap}, rc, hash::Hash};

trait ComponentColumn: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: 'static> ComponentColumn for Vec<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

type ComponentId = usize;
type ArchetypeId = Vec<TypeId>;

pub struct Storage {
    // edges_added: HashMap<TypeId, Option<Box<Archetype>>>,
    // edges_removed: HashMap<TypeId, Option<Box<Archetype>>>,
    component_type_id_to_component_id: HashMap<TypeId, ComponentId>,
    entity_id_to_archetype_id: HashMap<EntityId, ArchetypeId>,
    archetype_id_to_archetype: HashMap<ArchetypeId, Archetype>,
    // columns: Archetype,
}

impl Storage {
    // fn new() -> Self {
    //     // Self {
    //     //     archetype_id: 0,
    //     //     columns: Archetype::new(),
    //     //     edges_added: HashMap::new(),
    //     //     edges_removed: HashMap::new(),
    //     // }
    // }

    fn add<T: 'static>(&mut self, entity_id: usize, component : T) {
        if let Some(archetype_id) = self.entity_id_to_archetype_id.get(&entity_id) {
            let mut new_archetype_id = archetype_id.clone();
            new_archetype_id.push(TypeId::of::<T>());
            new_archetype_id.sort();
            if let Some(found_archetype) = self.archetype_id_to_archetype.get_mut(&new_archetype_id) {
                found_archetype.insert::<T>(entity_id, component);
            } else {
                //create new archetype
                let mut new_archetype = Archetype::new(new_archetype_id);
                new_archetype.insert::<T>(entity_id, component);
                
            }

            // if let Some(archetype) = self.archetype_id_to_archetype.get_mut(archetype_id) {
            //     archetype.insert(entity_id, component);
            // }
        }
    }

    fn remove<T>(&mut self, entity_id: usize) {

    }

    fn get_ref<T>(&mut self, entity_id: usize) {
        
    }

    fn get_mut<T>(&mut self, entity_id: usize) {
        
    }

}

// type ComponentId = TypeId;
type EntityId = usize;
type ComponentIndex = usize;
// type ComponentColumn = Box<dyn Any>;

pub struct Archetype {
    archetype_id: ArchetypeId,
    // entities: HashSet<usize>,
    entity_to_component_index: HashMap<EntityId, ComponentIndex>,
    // components: HashSet<TypeId>,
    // Box of dynamically typed vectors
    component_id_to_component_vector: HashMap<TypeId, Box<dyn Any>>,
}

impl Archetype {
    fn new(archetype_id: ArchetypeId) -> Self {
        Self {
            archetype_id: archetype_id,
            entity_to_component_index: HashMap::new(),
            component_id_to_component_vector: HashMap::new(),
        }
    }

    fn get_component_vector_mut<T: 'static>(&mut self) -> Option<&mut Vec<T>> {
        let typeid = TypeId::of::<T>();
        if let Some(boxed_vec) = self.component_id_to_component_vector.get_mut(&typeid) {
            if let Some(component_vec) = (**boxed_vec).downcast_mut::<Vec<T>>() {
                return Some(component_vec);
            }
        } 
        return None;
    }

    fn insert<T: 'static>(&mut self, entity_id: usize, component : T) {
        if let Some(boxedVec) = self.component_id_to_component_vector.get_mut(&component.type_id()) {
            if let Some(component_vec) = (**boxedVec).downcast_mut::<Vec<T>>() {
                let index = component_vec.len(); 
                component_vec.push(component);
                self.entity_to_component_index.insert(entity_id, index);
            } else {
                todo!();
            }
        } else {
            self.component_id_to_component_vector.insert(component.type_id(), Box::new(vec![component]));
            self.entity_to_component_index.insert(entity_id, 0);
            self.archetype_id.push(TypeId::of::<T>());
            self.archetype_id.sort();
        }
    }

    fn get_ref<T: 'static>(&self, entity_id: usize) -> Option<&T> {
        if let Some(component_index) = self.entity_to_component_index.get(&entity_id) {
            
            let typeid = TypeId::of::<T>();
            if let Some(boxed_vec) = self.component_id_to_component_vector.get(&typeid) {
                if let Some(component_vec) = (**boxed_vec).downcast_ref::<Vec<T>>() {
                    let a = component_vec.index(*component_index);
                    return Some(a);
                }
            } 
        }
        return None;
    }

    fn get_mut<T: 'static>(&mut self, entity_id: usize) -> Option<&mut T> {
        if let Some(component_index) = self.entity_to_component_index.get(&entity_id) {
            let typeid = TypeId::of::<T>();
            if let Some(boxed_vec) = self.component_id_to_component_vector.get_mut(&typeid) {
                if let Some(component_vec) = (**boxed_vec).downcast_mut::<Vec<T>>() {
                    let a = component_vec.index_mut(*component_index);
                    return Some(a);
                }
            } 
        }
        return None;
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, any::{Any, TypeId}};

    use crate::archetype_storage::ArchetypeId;

    use super::Archetype;
    
    #[test]
    fn func() {
        let mut s = Archetype::new(vec![TypeId::of::<u32>()]);

        s.insert::<u32>(0, 12);
        s.insert::<u32>(1, 12);

        let r = s.get_ref::<u32>(1).unwrap();
        println!("{}", r);
    
    }

    // #[test]
    // fn empty_storage_returns_none() {
    //     // Arrange
    //     let s = Archetype::new();
    //     // Act
    //     let result = s.get_ref::<u32>(23);
    //     // Assert
    //     assert_eq!(result, None);
    // }



    #[test]
    fn storage_returns_some_when_value_present() {
        // Arrange

        let mut s = Archetype::new(vec![TypeId::of::<u32>()]);
        s.insert::<u32>(23, 23);
        // Act
        let result = s.get_ref::<u32>(23);
        // Assert
        assert_eq!(result, Some(&23));
    }

    #[test]
    fn double_insert_results_in_updated_value() {
        // Arrange
        let mut s = Archetype::new(vec![TypeId::of::<f64>()]);
        s.insert::<f64>(12, 15.0);
        // Act
        s.insert::<f64>(12, 20.0);
        let result = s.get_ref::<f64>(12);
        // Assert
        assert_eq!(result, Some(&20.0));
    }

    #[test]
    fn typeid_test() {
        let a = 1;
        let b = 2.0;
        eprintln!("a.type_id() = {:?}", a.type_id());
    }

    // #[test]
    // fn can_handle_multple_component_types() {
    //          // Arrange
    //          let mut s = ArchetypeStorage::new();
    //          s.insert::<f64>(1, 1.0);
    //          s.insert::<f64>(2, 2.0);
    //          s.insert::<f64>(13, 3.0);

    //          s.insert::<u32>(1, 4);
    //          s.insert::<u32>(2, 5);
             
    //          s.insert::<u8>(1, 6);
    //          s.insert::<u8>(13, 7);
             
    //          // Act
    //          let result1 = s.get_ref::<f64>(1);
    //          let result2 = s.get_ref::<f64>(2);
    //          let result3 = s.get_ref::<f64>(13);
             
    //          let result4 = s.get_ref::<u32>(1);
    //          let result5 = s.get_ref::<u32>(2);
             
    //          let result6 = s.get_ref::<u8>(1);
    //          let result7 = s.get_ref::<u8>(13);
             
             
    //          // Assert
    //          assert_eq!(result1, Some(&(1.0 as f64)));
    //          assert_eq!(result2, Some(&(2.0 as f64)));
    //          assert_eq!(result3, Some(&(3.0 as f64)));
    //          assert_eq!(result4, Some(&(4 as u32)));
    //          assert_eq!(result5, Some(&(5 as u32)));
    //          assert_eq!(result6, Some(&(6 as u8)));
    //          assert_eq!(result7, Some(&(7 as u8)));
    // }


}