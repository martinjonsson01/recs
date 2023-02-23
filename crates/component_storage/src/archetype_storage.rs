
use std::{ops::{Index, IndexMut}, any::{Any, TypeId}, collections::{HashMap}};

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

pub struct Storage {
    // entities: HashSet<usize>,
    entity_to_component_index: HashMap<usize, usize>,
    // components: HashSet<TypeId>,
    component_id_to_component_vector: HashMap<TypeId, Box<dyn Any>>,
}

pub trait StorageInterface {
    fn insert<T>(&mut self, entity_id: usize, component : T);
}

impl Storage {
    fn insert<T: 'static>(&mut self, entity_id: usize, component : T) {
        if let Some(boxedVec) = self.component_id_to_component_vector.get_mut(&component.type_id()) {
            if let Some(component_vec) = (**boxedVec).downcast_mut::<Vec<T>>() {
                let index = component_vec.len(); 
                component_vec.push(component);
                self.entity_to_component_index.insert(entity_id, index);
                println!("Vec Found!");
            } else {
                todo!();
            }
        } else {
            self.component_id_to_component_vector.insert(component.type_id(), Box::new(vec![component]));
            self.entity_to_component_index.insert(entity_id, 0);
            println!("New vec created, component inserted!");
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

    use super::Storage;
    
    #[test]
    fn func() {
        let mut s = Storage {
            entity_to_component_index: HashMap::new(),
            component_id_to_component_vector: HashMap::new(),
        };

        s.insert::<u32>(0, 12);
        s.insert::<u32>(1, 12);

        let r = s.get_ref::<u32>(1).unwrap();
        println!("{}", r);
    
    }

}