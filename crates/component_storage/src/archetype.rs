use std::any::{Any,TypeId};
use crate::entity::Entity;

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

struct Storage<'a> {
    archetypes: Vec<Archetype<'a>>,
}

impl<'a> Storage<'a> {
    fn default() -> Self {
        Self {
            archetypes: Vec::new(),
        }
    }

    fn lookup_all_archetypes(archetype_size: usize) {
        todo!()
    }

    fn lookup_archetype(entity: Entity) {
        todo!()
    }
}

struct Component {
    component_type: TypeId,
    //component_id: usize,
}

struct Archetype<'a> {
    a_id: usize,
    //generation: usize,
    entities: Vec<crate::entity::Entity>, 
    columns: Vec<Box<dyn ComponentColumn>>,
    add_edge: Vec<&'a Archetype<'a>>,
    remove_edge: Vec<&'a Archetype<'a>>
}

impl<'a> Archetype<'a> {
    fn add<T: 'static>(entity: Entity, component: Component) -> Archetype<'a>{
        /**
         * 1. remove entity and add component to entity
         * 2. find new e_id
         * 3. lookup if archetype with mathing a_id exist
         *      True: add entity 
         *      False: create archetype and add entity (new_from_add + add entity(tba))
         */
        todo!()
    }

    fn new_from_add<T: 'static>(from_archetype: &'a Archetype) -> Archetype<'a>{
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
            a_id: columns.len(),
            entities: Vec::new(),
            columns,
            add_edge: Vec::new(),
            remove_edge: vec![from_archetype],
        }
    }

    fn new_from_remove<T: 'static>(from_archetype: &'a Archetype) -> Archetype<'a> {
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
            a_id: columns.len(),
            entities: Vec::new(),
            columns,
            add_edge: vec![from_archetype],
            remove_edge: Vec::new(),
        } 
    }

    fn builder() -> ColumnsBuilder {
        ColumnsBuilder(Vec::new())
    }

    fn new_from_columns(columns: ColumnsBuilder) -> Archetype<'a> {
        Archetype {
            a_id: columns.0.len(),
            entities: Vec::new(),
            columns: columns.0,
            add_edge: Vec::new(),
            remove_edge: Vec::new(),
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
            a_id: 0,
            entities: Vec::new(),
            columns: Vec::new(),
            add_edge: Vec::new(),
            remove_edge: Vec::new(),
        };
        let archetype = Archetype::new_from_add::<u32>(&archetype);
        let archetype = Archetype::new_from_add::<u32>(&archetype);
    }

    #[test]
    #[should_panic]
    fn remove_unpresent() {
        let archetype = Archetype {
            a_id: 0,
            entities: Vec::new(),
            columns: Vec::new(),
            add_edge: Vec::new(),
            remove_edge: Vec::new(),
        };
        let archetype = Archetype::new_from_remove::<u32>(&archetype);
    }

    #[test]
    #[should_panic]
    fn remove_unpresent_2() {
        let archetype = Archetype {
            a_id: 0,
            entities: Vec::new(),
            columns: Vec::new(),
            add_edge: Vec::new(),
            remove_edge: Vec::new(),
        };
        let archetype = Archetype::new_from_add::<u64>(&archetype);
        let archetype = Archetype::new_from_remove::<u32>(&archetype);
    }

    #[test]
    fn add_removes() {
        let archetype = Archetype {
            a_id: 0,
            entities: Vec::new(),
            columns: Vec::new(),
            add_edge: Vec::new(),
            remove_edge: Vec::new(),
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
