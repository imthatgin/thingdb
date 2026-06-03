use crate::storage::{KeyEncoder, Storage};
use std::collections::HashSet;
use std::sync::Arc;

pub struct Query<T> {
    storage: Arc<Storage>,
    output_attr_hash: u64,
    with_components: Vec<u64>,
    without_components: Vec<u64>,
    filter_fn: Option<Box<dyn Fn(&T) -> bool + Send + Sync>>,
}

impl<T: crate::Attribute> Query<T>
where
    T: for<'de> serde::Deserialize<'de> + Send + 'static,
{
    pub fn new(storage: Arc<Storage>) -> Self {
        let output_attr_hash = crate::hash_name(<T as crate::Attribute>::NAME);
        Self {
            storage,
            output_attr_hash,
            with_components: Vec::new(),
            without_components: Vec::new(),
            filter_fn: None,
        }
    }

    pub fn with<U: crate::Attribute + Send + 'static>(mut self) -> Self {
        let hash = crate::hash_name(<U as crate::Attribute>::NAME);
        self.with_components.push(hash);
        self
    }

    pub fn without<U: crate::Attribute + Send + 'static>(mut self) -> Self {
        let hash = crate::hash_name(<U as crate::Attribute>::NAME);
        self.without_components.push(hash);
        self
    }

    pub fn filter<F: Fn(&T) -> bool + Send + Sync + 'static>(mut self, f: F) -> Self {
        self.filter_fn = Some(Box::new(f));
        self
    }

    pub async fn run(self) -> Vec<T> {
        let mut results = Vec::new();
        let filter_fn = self.filter_fn;

        // Use reverse index to find candidates — avoids scanning all entities
        let mut candidates: HashSet<u128> = self.storage
            .get_entities_with_attr(self.output_attr_hash)
            .into_iter()
            .collect();

        // Narrow by intersecting with each .with() component
        for &with_hash in &self.with_components {
            let with_entities: HashSet<u128> = self.storage
                .get_entities_with_attr(with_hash)
                .into_iter()
                .collect();
            candidates.retain(|e| with_entities.contains(e));
        }

        // Remove entities that have any .without() component — set-level exclusion
        for &without_hash in &self.without_components {
            let without_entities: HashSet<u128> = self.storage
                .get_entities_with_attr(without_hash)
                .into_iter()
                .collect();
            candidates.retain(|e| !without_entities.contains(e));
        }

        // Use stored archetype from eta: mapping for efficient data key lookup
        for thing_id in &candidates {
            if let Some(archetype_id) = self.storage.get_entity_archetype(*thing_id) {
                let key = KeyEncoder::encode(archetype_id, self.output_attr_hash, *thing_id);
                if let Some(data) = self.storage.get(&key) {
                    if let Ok(item) = postcard::from_bytes::<T>(&data) {
                        match &filter_fn {
                            Some(f) if f(&item) => results.push(item),
                            Some(_) => {}
                            None => results.push(item),
                        }
                    }
                }
            }
        }

        results
    }
}
