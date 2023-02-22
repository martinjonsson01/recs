use std::any::Any; //switch for something else?

trait ComponentColumn: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn new_empty_column(&self) -> Box<dyn ComponentColumn>;
}

impl<T: 'static> ComponentColumn for Vec<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn new_empty_column(&self) -> Box<dyn ComponentColumn> {
        Box::new(Vec::<T>::new())
    }
}

struct Archetype {
    //id: usize,
    //generation: usize,
    entities: Vec<crate::entity::Entity>, 
    columns: Vec<Box<dyn ComponentColumn>>, //find a more efficient storage
}

impl Archetype {
    fn new_from_add<T: 'static>(from_archetype: &Archetype) -> Archetype {
        let mut columns: Vec<_> = from_archetype
                .columns
                .iter()
                .map(|column| column.new_empty_column())
                .collect();

        assert!(columns
                .iter()
                .find(|column| column.as_any().is::<Vec<T>>())
                .is_none()); //assert will panic if it finds the same ComponentColumn
        columns.push(Box::new(Vec::<T>::new()));

        Archetype {
            entities: Vec::new(),
            columns,
        }
    }

    fn new_from_remove<T: 'static>(from_archetype: &Archetype) -> Archetype {
        let mut columns: Vec<_> = from_archetype
                .columns
                .iter()
                .map(|column| column.new_empty_column())
                .collect();

        let index = columns
                .iter()
                .position(|column| column.as_any().is::<Vec<T>>())
                .unwrap();
        columns.remove(index);

        Archetype {
            entities: Vec::new(),
            columns,
        } 
    }

    fn builder() -> ColumnsBuilder {
        ColumnsBuilder(Vec::new())
    }

    fn new_from_columns(columns: ColumnsBuilder) -> Archetype {
        Archetype {
            entities: Vec::new(),
            columns: columns.0,
        }
    }
}

struct ColumnsBuilder(Vec<Box<dyn ComponentColumn>>);

impl ColumnsBuilder {
    fn with_column_type<T: 'static>(mut self) -> Self {
        if let Some(_) = self
            .0
            .iter()
            .find(|col| col.as_any().type_id() == std::any::TypeId::of::<Vec<T>>())
        {
            panic!("Attempted to create invalid archetype");
        }

        self.0.push(Box::new(Vec::<T>::new()));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Archetype;


    #[test]
    #[should_panic]
    fn add_preexisting() {
        let archetype = Archetype {
            entities: Vec::new(),
            columns: Vec::new(),
        };
        let archetype = Archetype::new_from_add::<u32>(&archetype);
        let archetype = Archetype::new_from_add::<u32>(&archetype);
    }

    #[test]
    #[should_panic]
    fn remove_unpresent() {
        let archetype = Archetype {
            entities: Vec::new(),
            columns: Vec::new(),
        };
        let archetype = Archetype::new_from_remove::<u32>(&archetype);
    }

    #[test]
    #[should_panic]
    fn remove_unpresent_2() {
        let archetype = Archetype {
            entities: Vec::new(),
            columns: Vec::new(),
        };
        let archetype = Archetype::new_from_add::<u64>(&archetype);
        let archetype = Archetype::new_from_remove::<u32>(&archetype);
    }

    #[test]
    fn add_removes() {
        let archetype = Archetype {
            entities: Vec::new(),
            columns: Vec::new(),
        };

        let archetype = Archetype::new_from_add::<u32>(&archetype);
        assert!(archetype.columns.len() == 1);
        assert!(archetype
            .columns
            .iter()
            .find(|col| col.as_any().is::<Vec<u32>>())
            .is_some());

        let archetype = Archetype::new_from_add::<u64>(&archetype);
        assert!(archetype.columns.len() == 2);
        assert!(archetype
            .columns
            .iter()
            .find(|col| col.as_any().is::<Vec<u32>>())
            .is_some());
        assert!(archetype
            .columns
            .iter()
            .find(|col| col.as_any().is::<Vec<u64>>())
            .is_some());

        let archetype = Archetype::new_from_remove::<u32>(&archetype);
        assert!(archetype.columns.len() == 1);
        assert!(archetype
            .columns
            .iter()
            .find(|col| col.as_any().is::<Vec<u64>>())
            .is_some());
    }

    #[test]
    fn columns_builder() {
        let archetype = Archetype::new_from_columns(
            Archetype::builder()
                .with_column_type::<u32>()
                .with_column_type::<u64>()
                .with_column_type::<bool>(),
        );

        assert!(archetype.columns.len() == 3);
        assert!(archetype
            .columns
            .iter()
            .find(|col| col.as_any().is::<Vec<u32>>())
            .is_some());
        assert!(archetype
            .columns
            .iter()
            .find(|col| col.as_any().is::<Vec<u64>>())
            .is_some());
        assert!(archetype
            .columns
            .iter()
            .find(|col| col.as_any().is::<Vec<bool>>())
            .is_some());
    }

    #[test]
    #[should_panic]
    fn columns_builder_duplicate() {
        let archetype = Archetype::new_from_columns(
            Archetype::builder()
                .with_column_type::<u32>()
                .with_column_type::<u32>(),
        );
    }
 
}
