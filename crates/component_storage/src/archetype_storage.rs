
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

pub struct Archetype {
    archetype_id: usize,
    columns: ArchetypeStorage,
    edges_added: HashMap<TypeId, Option<Box<Archetype>>>,
    edges_removed: HashMap<TypeId, Option<Box<Archetype>>>,
}

impl Archetype {
    fn new() -> Self {
        Self {
            archetype_id: 0,
            columns: ArchetypeStorage::new(),
            edges_added: HashMap::new(),
            edges_removed: HashMap::new(),
        }
    }

    fn add<T>(&mut self, entity_id: usize) {
        
    }

    fn remove<T>(&mut self, entity_id: usize) {

    }

    fn get_ref<T>(&mut self, entity_id: usize) {
        
    }

    fn get_mut<T>(&mut self, entity_id: usize) {
        
    }


}

pub struct ArchetypeStorage {
    // entities: HashSet<usize>,
    entity_to_component_index: HashMap<usize, usize>,
    // components: HashSet<TypeId>,
    component_id_to_component_vector: HashMap<TypeId, Box<dyn Any>>,
}

pub trait StorageInterface {
    fn new();
    fn insert<T>(&mut self, entity_id: usize, component : T);
}

impl ArchetypeStorage {
    fn new() -> Self {
        Self {
            entity_to_component_index: HashMap::new(),
            component_id_to_component_vector: HashMap::new(),
        }
    }

    fn generate_archetype_id(&self) {
        let keys = self.component_id_to_component_vector.keys();
        // .fold("".to_string(), |acc:String, &type_id| acc. type_id)

        // let concated : String = keys.into_iter().fold(0,
        //                                   |mut i,j| {i + j; i});
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
                // println!("Vec Found!");
            } else {
                todo!();
            }
        } else {
            self.component_id_to_component_vector.insert(component.type_id(), Box::new(vec![component]));
            self.entity_to_component_index.insert(entity_id, 0);
            // println!("New vec created, component inserted!");
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
    use std::collections::HashMap;

    use super::ArchetypeStorage;
    
    #[test]
    fn func() {
        let mut s = ArchetypeStorage {
            entity_to_component_index: HashMap::new(),
            component_id_to_component_vector: HashMap::new(),
        };

        s.insert::<u32>(0, 12);
        s.insert::<u32>(1, 12);

        let r = s.get_ref::<u32>(1).unwrap();
        println!("{}", r);
    
    }

    #[test]
    fn empty_storage_returns_none() {
        // Arrange
        let s = ArchetypeStorage::new();
        // Act
        let result = s.get_ref::<u32>(23);
        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn storage_returns_some_when_value_present() {
        // Arrange
        let mut s = ArchetypeStorage::new();
        s.insert::<u32>(23, 23);
        // Act
        let result = s.get_ref::<u32>(23);
        // Assert
        assert_eq!(result, Some(&23));
    }

    #[test]
    fn double_insert_results_in_updated_value() {
        // Arrange
        let mut s = ArchetypeStorage::new();
        s.insert::<f64>(12, 15.0);
        // Act
        s.insert::<f64>(12, 20.0);
        let result = s.get_ref::<f64>(12);
        // Assert
        assert_eq!(result, Some(&20.0));
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