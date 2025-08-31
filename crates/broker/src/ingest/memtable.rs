use serde_json::Value;
use std::sync::{Arc, RwLock};

#[derive(Default, Clone)]
pub struct Memtable {
    inner: Arc<RwLock<Vec<Value>>>,
}

impl Memtable {
    pub fn new() -> Self { Self::default() }

    pub fn len(&self) -> usize { self.inner.read().unwrap().len() }

    pub fn clear(&self) {
        self.inner.write().unwrap().clear();
    }

    pub fn push_many(&self, docs: Vec<Value>) {
        let mut g = self.inner.write().unwrap();
        g.extend(docs);
    }

    pub fn snapshot(&self) -> Vec<Value> {
        self.inner.read().unwrap().clone()
    }
}
