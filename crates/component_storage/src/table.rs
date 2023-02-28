use std::{
    any::{Any, TypeId},
    collections::HashMap,
    hash::Hash,
    ops::IndexMut,
    result::{self, Iter},
};

use crate::column::{self, Column};

// A table stores data in columns
// The same row/index of each column contains data for the same entity.
// The table keeps track of what entity_id maps to which column index.
struct Table {
    columns: HashMap<TypeId, Box<dyn Column>>,
    entity_id_to_index: HashMap<usize, usize>,
    index_to_entity_id: HashMap<usize, usize>,
}

impl Table {
    fn new() -> Self {
        Self {
            columns: HashMap::new(),
            index_to_entity_id: HashMap::new(),
            entity_id_to_index: HashMap::new(),
        }
    }

    fn insert<T: 'static>(&mut self, entity_id: usize, element: T) {

        if let index = self.entity_id_to_index.contains_key(&entity_id) {

        }

        // add new entity and component
        if let Some(column) = self.get_column_as_vec_mut::<T>() {
            let index = column.len();
            column.push(Some(element));
            self.entity_id_to_index.insert(entity_id, index);
            self.index_to_entity_id.insert(index, entity_id);
        } else {
            self.columns
                .insert(TypeId::of::<T>(), Box::new(vec![Some(element)]));
            let index = 0;
            self.entity_id_to_index.insert(entity_id, index);
            self.index_to_entity_id.insert(index, entity_id);
        }
    }

    fn insert_empty_row(&mut self, entity_id: usize) {
        if self.entity_id_to_index.contains_key(&entity_id) {
            todo!("Cannot add new empty row as entity id is already present in table")
        }

        let index = self.entity_id_to_index.len();

        self.columns
            .values_mut()
            .into_iter()
            .for_each(|v| v.add_empty_cell());

        self.entity_id_to_index.insert(entity_id, index);
        self.index_to_entity_id.insert(index, entity_id);
    }

    fn remove_entity(&mut self, entity_id: usize) {
        if let Some(index) = self.entity_id_to_index.get(&entity_id) {
            
            self.columns
                .values_mut()
                .into_iter()
                .for_each(|v| v.swap_remove(entity_id));
    
            self.index_to_entity_id.remove(index);
            self.entity_id_to_index.remove(&entity_id);
        }
    }

    fn remove_component<T: 'static>(&mut self, entity_id: usize) {
        if let Some(&index) = self.entity_id_to_index.get(&entity_id) {
            
            if let Some(column) = self.get_column_as_vec_mut::<T>() {
                *column.index_mut(index) = None;
            }
    
            self.index_to_entity_id.remove(&index);
            self.entity_id_to_index.remove(&entity_id);
        }
    }

    fn get_column_ref<T: 'static>(&self) -> Option<&Box<dyn Column>> {
        self.columns.get(&TypeId::of::<T>())
    }

    fn get_column_mut<T: 'static>(&mut self) -> Option<&mut Box<dyn Column>> {
        self.columns.get_mut(&TypeId::of::<T>())
    }

    fn get_column_as_vec<T: 'static>(&self) -> Option<&Vec<Option<T>>> {
        match self.get_column_ref::<T>() {
            Some(column) => column.as_any().downcast_ref::<Vec<Option<T>>>(),
            _ => None,
        }
    }

    fn get_column_as_vec_mut<T: 'static>(&mut self) -> Option<&mut Vec<Option<T>>> {
        match self.get_column_mut::<T>() {
            Some(column) => column.as_any_mut().downcast_mut::<Vec<Option<T>>>(),
            _ => None,
        }
    }

    fn new_empty_table(&self) -> Self {
        let empty_columns: Vec<Box<dyn Column>> = self
            .columns
            .values()
            .into_iter()
            .map(|v| v.new_empty_column(0))
            .collect();

        let mut columns: HashMap<TypeId, Box<dyn Column>> = HashMap::new();

        for column in empty_columns {
            columns.insert(column.stored_type_id(), column);
        }

        Self {
            columns,
            entity_id_to_index: HashMap::new(),
            index_to_entity_id: HashMap::new(),
        }
    }

    fn get_all_type_ids(&self) -> Vec<TypeId> {
        self.columns
            .values()
            .into_iter()
            .map(|v| v.stored_type_id())
            .collect()
    }
}

//////////  TESTS //////////

#[test]
fn weird_type_shit() {
    let val1 = 1;
    let val2: f64 = 3.0;

    let mut table = Table::new();
    table
        .columns
        .insert(val1.type_id(), Box::new(vec![Some(1), Some(2), Some(3)]));
    table.columns.insert(
        val2.type_id(),
        Box::new(vec![Some(1.0), Some(2.0), Some(3.0)]),
    );

    let r = table.get_column_ref::<i32>().unwrap();

    let rd = r.as_any().downcast_ref::<Vec<Option<i32>>>().unwrap(); // option was added for manual downcast, since option was added to column

    eprintln!("result = {:#?}", rd);
}

#[test]
fn weirder_type_stuff() {
    let val1: i32 = 1;
    let val2: f64 = 2.0;

    let mut table = Table::new();
    table
        .columns
        .insert(val1.type_id(), Box::new(vec![Some(3), Some(4), Some(5)]));
    table.columns.insert(
        val2.type_id(),
        Box::new(vec![Some(6.0), Some(7.0), Some(8.0)]),
    );

    // let r = table.get_column::<i32>().unwrap();

    // let rd = r.as_any().downcast_ref::<Vec<i32>>().unwrap();

    let r1 = table.get_column_as_vec::<i32>();
    let r2 = table.get_column_as_vec::<f64>();

    eprintln!("result = {:#?}", r1.unwrap());
    eprintln!("result = {:#?}", r2.unwrap());
}

#[test]
fn test_get_column() {
    let val1: i32 = 1;
    let val2: f64 = 2.0;

    let mut table = Table::new();
    table
        .columns
        .insert(val1.type_id(), Box::new(vec![Some(3), Some(4), Some(5)]));
    table.columns.insert(
        val2.type_id(),
        Box::new(vec![Some(6.0), Some(7.0), Some(8.0)]),
    );

    if let Some(r) = table.get_column_as_vec_mut::<i32>() {
        r.push(Some(1));
        r.push(Some(2));
        r.push(Some(3));
        r.push(Some(4));
    }

    let r1 = table.get_column_as_vec::<i32>();
    // let r2 = table.get_column_as_vec::<f64>();

    eprintln!("result = {:#?}", r1.unwrap());
    // eprintln!("result = {:#?}", r2.unwrap());
}


#[test]
fn empty_table_produces_clone() {
    let mut table = Table::new();

    table.insert::<u32>(0, 1);
    table.insert::<u32>(1, 2);
    table.insert::<f64>(0, 3.0);
    table.insert::<f64>(1, 4.0);



    let new_empty = table.new_empty_table();

    let a = new_empty.columns.keys();

    eprintln!("a = {:#?}", a);
    let b = table.columns.keys();

    eprintln!("a = {:#?}", b);
}

#[test]
fn remove_component_removes_component() {
    let mut table = Table::new();

    table.insert::<u32>(0, 1);
    table.insert::<u32>(1, 2);
    table.insert::<f64>(0, 3.0);
    table.insert::<f64>(1, 4.0);

    table.remove_component::<u32>(1);

    let a = table.get_column_as_vec::<u32>().unwrap();
    let b = table.get_column_as_vec::<f64>().unwrap();

    assert_eq!(*a, vec![Some(1), None]);
    assert_eq!(*b, vec![Some(3.0), Some(4.0)]);
}


#[test]
fn remove_entity_works() {
    let mut table = Table::new();

    table.insert::<u32>(0, 1);
    table.insert::<u32>(1, 2);
    table.insert::<u32>(2, 3);
    table.insert::<f64>(0, 4.0);
    table.insert::<f64>(1, 5.0);
    table.insert::<f64>(2, 6.0);

    table.remove_entity(1);

    let a = table.get_column_as_vec::<u32>().unwrap();
    let b = table.get_column_as_vec::<f64>().unwrap();

    assert_eq!(*a, vec![Some(1), Some(3)]);
    assert_eq!(*b, vec![Some(4.0), Some(6.0)]);
}

#[test]
fn insert_inserts_row_in_each_column() {

    let mut table = Table::new();

    table.insert::<u32>(0, 1);
    table.insert::<f64>(1, 2.0);
    table.insert::<u32>(2, 3);
    
    let a = table.get_column_as_vec::<u32>().unwrap();
    let b = table.get_column_as_vec::<f64>().unwrap();


    eprintln!("columns = {:?}",table.entity_id_to_index);

    // should be: 0: 0, 1: 1, 2: 2

    // current: 0: 0, 1: 0, 2: 1 // rimligt resultat.

    eprintln!("a = {:?}",a);
    eprintln!("b = {:?}",b);

    // assert_eq!(*a, vec![Some(1), None, Some(3)]);
    // assert_eq!(*b, vec![None, Some(2.0), None]);
}