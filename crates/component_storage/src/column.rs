use std::{any::{Any, TypeId}};

pub trait Column: Any {
    fn as_any(&self) -> &dyn Any;
    
    fn as_any_mut(&mut self) -> &mut dyn Any;

    fn empty_copy(&self, capacity: usize) -> Box<dyn Column>;

    fn swap_remove(&mut self, index: usize);

    fn add_empty_cell(&mut self);

    fn stored_type_id(&self) -> TypeId;
}

impl<T: 'static> Column for Vec<Option<T>> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn empty_copy(&self, capacity: usize) -> Box<dyn Column> {
        Box::new(Vec::<Option<T>>::new())
    }

    fn swap_remove(&mut self, index: usize) {
        Vec::<Option<T>>::swap_remove(self, index);
    }

    fn add_empty_cell(&mut self) {
        self.push(None);
    }

    fn stored_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }
}

pub fn new_empty_column<T: 'static>(capacity: usize) -> Box<dyn Column> {
    let v: Vec<Option<T>> = std::iter::repeat_with(|| None).take(capacity).collect();
    Box::new(v)
}

#[test]
fn new_empty_column_contains_only_option_none() {
    // Act
    let mut column = new_empty_column::<i32>(100);
    
    // Assert
    let downcasted_vector = column.as_any_mut().downcast_mut::<Vec<Option<i32>>>().unwrap();

    downcasted_vector.iter().for_each(|x| assert!(x.is_none()));
}

#[test]
fn elements_are_swaped_correctly() {
    // Arrange
    let mut column: Box<dyn Column> = Box::new(vec![Some(1.0), Some(2.0), Some(3.0)]);
    
    // Act
    column.swap_remove(1);
    
    // Assert
    let result = column.as_any().downcast_ref::<Vec<Option<f64>>>().unwrap();
    assert_eq!(result, &vec![Some(1.0), Some(3.0)]);

}