use std::collections::HashMap;

use sparseset::SparseSet;



struct Table<T> {
    //(a,b,c)
    // archetype_storage: Vec<T>,
    //entity_id_to_index: HashMap<u32, usize>,
    //index_to_entity_id: HashMap<usize, u32>,
    archetype_storage: SparseSet<T>,
    last_index: usize
}

impl<T> Table<T> {

    // pub fn archetype_storage(&self) -> &[T] {
    //     &self.archetype_storage
    // }
    
    // fn insert(&mut self, archetype :T, entity_id: u32) {
    //     // insert archetype into vec
    //     self.archetype_storage[self.last_index] = archetype;
    //     // store index
    //     self.entity_id_to_index.insert(entity_id);
    //     // self.entity_id_to_index.insert(entity_id, self.last_index);
    //     // self.index_to_entity_id.insert(self.last_index, entity_id);
    //     self.last_index+=1;
    // }

    // fn remove(&mut self, entity_id: u32) -> bool {
        
    //     // match self.entity_id_to_index.get(&entity_id) {
    //     //     Some(removed_index) => {
                
    //     //         // swap removed and last
    //     //         // archetypestorage and entity_ids
                
    //     //         let last_archetype_index= self.index_to_entity_id.get(&self.last_index).unwrap();
                


    //     //         // self.entity_id_to_index.insert(, last_archetype_index);
    //     //         self.archetype_storage.swap(entity_id as usize, self.last_index);
    //     //         true
    //     //     }
    //     //     None => false
    //     // }

        
        
}

#[cfg(test)]
mod tests {

    #[test]
    fn it_works() {

    }
}
