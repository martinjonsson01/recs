use std::ops::Index;

struct SparseSet {
    dense: Vec<usize>,
    sparse: Vec<usize>,
}

impl SparseSet {
    fn new(capacity: usize) -> Self {
        Self {
            sparse: vec![0; capacity],
            dense: Vec::new(),
        }
    }

    fn insert(&mut self, element: usize) {
        if self.contains(element) {
            return;
        }
        let index = self.dense.len();
        self.dense.push(element);
        self.sparse.insert(element, index);
    }

    fn remove(&mut self, element: usize) {
        let &index = self.sparse.index(element);
        self.dense.swap_remove(index);
        self.sparse.insert(index, 0);
    }

    fn contains(&self, element: usize) -> bool {
        let &index = self.sparse.index(element);
        if index >= self.dense.len() {
            return false;
        }
        let &value = self.dense.index(index);
        return value == element;
    }
}

#[cfg(test)]
mod tests {
    use super::SparseSet;

    #[test]
    fn element_removed_after_removal() {
        // Arrange
        let mut s = SparseSet::new(10);
        s.insert(1);
        s.insert(2);
        s.insert(3);

        // Act
        s.remove(2);

        // Assert
        assert!(!s.contains(2));
    }

    #[test]
    fn contains_empty() {
        // Arrange
        let s = SparseSet::new(10);

        // Assert
        assert!(!s.contains(2));
    }

    #[test]
    fn double_insert_equals_single_insert() {
        // Arrange
        let mut s = SparseSet::new(10);

        // Act
        s.insert(2);
        s.insert(3);
        s.insert(2);

        // Assert
        assert_eq!(s.dense, vec![2,3]);
    }

    #[test]
    fn it_works() {
        let mut s = SparseSet::new(10);

        s.insert(2);
        s.insert(1);
        s.insert(9);
        s.insert(5);

        eprintln!("Insert: ");
        eprintln!("s.sparse = {:?}", s.sparse);
        eprintln!("s.dense = {:?}", s.dense);

        s.remove(2);
        s.remove(9);
        s.remove(0);

        eprintln!("Remove: ");
        eprintln!("s.sparse = {:?}", s.sparse);
        eprintln!("s.dense = {:?}", s.dense);

        eprintln!("s.contains(1) = {:?}", s.contains(1));
        eprintln!("s.contains(2) = {:?}", s.contains(2));
        eprintln!("s.contains(3) = {:?}", s.contains(3));
        eprintln!("s.contains(100) = {:?}", s.contains(100));
    }
}
