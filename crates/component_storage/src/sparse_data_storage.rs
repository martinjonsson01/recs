use std::ops::{Index, IndexMut};

// 

struct SparseDataSet<T> {
    sparse: Vec<usize>,
    dense: Vec<usize>,
    data: Vec<T>,
}

impl<T> SparseDataSet<T> {
    fn new(capacity: usize) -> Self {
        Self {
            sparse: vec![0; capacity],
            dense: Vec::new(),
            data: Vec::new(),
        }
    }

    fn insert(&mut self, element: usize, data: T) {
        if self.contains(element) {
            let old_data = self.get_mut(element).unwrap();
            *old_data = data;
        } else {
            let index = self.dense.len();
            self.dense.push(element);
            self.data.push(data);
            // self.sparse.insert(element, index);
            *self.sparse.get_mut(element).unwrap() = index;
        }
    }

    fn remove(&mut self, element: usize) {
        if self.contains(element) {
            let &index = self.sparse.index(element);
            self.dense.swap_remove(index);
            self.data.swap_remove(index);
            self.sparse.insert(index, 0);
        }
    }

    fn get_index(&self, element: usize) -> Option<usize> {
        let &index = self.sparse.index(element);
        if index >= self.dense.len() { // changed from >=, might cause errors
            return None;
        }
        return Some(index);
    }

    fn contains(&self, element: usize) -> bool {
        match self.get_index(element) {
            Some(index) => *self.dense.index(index) == element,
            None => false,
        }
    }

    fn get(&self, element: usize) -> Option<&T> {
        match self.get_index(element) {
            Some(index) => Some(self.data.index(index)),
            None => None,
        }
    }

    fn get_mut(&mut self, element: usize) -> Option<&mut T> {
        match self.get_index(element) {
            Some(index) => Some(self.data.index_mut(index)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sparse_data_storage;

    use super::*;

    #[test]
    fn double_insert_should_be_valid() {
        // Arrange
        let mut s: SparseDataSet<u8> = SparseDataSet::new(10);
        // Act
        s.insert(3, 10);
        s.insert(3, 10);
        // Assert
        assert_eq!(s.dense, vec![3]);
    }

    #[test]
    fn insert_overrides_old_data() {
        // Arrange
        let mut s: SparseDataSet<u32> = SparseDataSet::new(10);
        // Act
        s.insert(3, 10);
        s.insert(3, 12);
        // Assert
        assert_eq!(s.data, vec![12]);
    }

    #[test]
    fn multiple_insert_adds_data() {
        // Arrange
        let mut s: SparseDataSet<u32> = SparseDataSet::new(10);
        // Act
        s.insert(4, 12);
        s.insert(0, 18);
        s.insert(1, 99);
        // Assert
        assert_eq!(s.data, vec![12, 18, 99]);
    }

    #[test]
    fn double_remove_should_be_valid() {
        // Arrange
        let mut s: SparseDataSet<u32> = SparseDataSet::new(10);
        // Act
        s.remove(0);
        s.remove(0);
        // Assert
        assert_eq!(s.data, vec![]);
    }

    #[test]
    fn contains_nothing() {
        // Arrange
        let s: SparseDataSet<u32> = SparseDataSet::new(10);
        // Act
        let result = s.contains(1);
        // Assert
        assert_eq!(result, false);
    }

    #[test]
    fn contains_one_element() {
        // Arrange
        let mut s: SparseDataSet<u32> = SparseDataSet::new(10);
        s.insert(1, 12);
        // Act
        let result = s.contains(1);
        // Assert
        assert_eq!(result, true);
    }

    #[test]
    fn contains_one_of_multiple() {
        // Arrange
        let mut s: SparseDataSet<u32> = SparseDataSet::new(10);
        s.insert(7, 6);
        s.insert(1, 15);
        s.insert(0, 1);
        // Act
        let result = s.contains(1);
        // Assert
        assert_eq!(result, true);
    }
}
