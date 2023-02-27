use std::{any::{Any, TypeId}};

pub trait Column: Any {
    fn as_any(&self) -> &dyn Any;
    
    fn as_any_mut(&mut self) -> &mut dyn Any;

    fn new_empty_column(&self) -> Box<dyn Column>;

    fn swap_remove(&mut self, index: usize);

    fn add_empty_row(&mut self);

    fn stored_type_id(&self) -> TypeId;
}

impl<T: 'static> Column for Vec<Option<T>> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn new_empty_column(&self) -> Box<dyn Column> {
        Box::new(Vec::<Option<T>>::new())
    }

    fn swap_remove(&mut self, index: usize) {
        Vec::<Option<T>>::swap_remove(self, index);
    }

    fn add_empty_row(&mut self) {
        self.push(None);
    }

    fn stored_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }
}