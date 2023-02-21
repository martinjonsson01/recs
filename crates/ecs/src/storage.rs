use std::any::Any;
use std::fmt::Formatter;
use std::sync::RwLock;

pub type ComponentVecImpl<T> = RwLock<Vec<Option<T>>>;

pub trait ComponentVec: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn push_none(&mut self);
}

impl std::fmt::Debug for dyn ComponentVec + 'static {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "component vector")
    }
}

impl<T: Send + Sync + 'static> ComponentVec for ComponentVecImpl<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn push_none(&mut self) {
        self.write().expect("Lock is poisoned").push(None);
    }
}
