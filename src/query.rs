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
        let filter_fn = self.filter_fn;

        let mut candidates: HashSet<u128> = self
            .storage
            .get_entities_with_attr(self.output_attr_hash)
            .into_iter()
            .collect();

        for &with_hash in &self.with_components {
            let with_entities: HashSet<u128> = self
                .storage
                .get_entities_with_attr(with_hash)
                .into_iter()
                .collect();
            candidates.retain(|e| with_entities.contains(e));
        }

        for &without_hash in &self.without_components {
            let without_entities: HashSet<u128> = self
                .storage
                .get_entities_with_attr(without_hash)
                .into_iter()
                .collect();
            candidates.retain(|e| !without_entities.contains(e));
        }

        if candidates.is_empty() {
            return Vec::new();
        }

        let mut sorted: Vec<u128> = candidates.into_iter().collect();
        sorted.sort();

        let n = sorted.len();

        // Use direct point lookups for small sets (avoids multi_get overhead),
        // batch via multi_get for large sets (reduces FFI / I/O overhead).
        let results = if n < 500 {
            Self::fetch_point_lookups(&self.storage, &sorted, self.output_attr_hash, &filter_fn)
        } else {
            Self::fetch_batched(&self.storage, &sorted, self.output_attr_hash, &filter_fn).await
        };

        results
    }

    fn fetch_point_lookups(
        storage: &Arc<Storage>,
        ids: &[u128],
        output_hash: u64,
        filter_fn: &Option<Box<dyn Fn(&T) -> bool + Send + Sync>>,
    ) -> Vec<T> {
        let mut results = Vec::new();
        for &id in ids {
            if let Some(arch_val) = storage.get_entity_archetype(id) {
                let key = KeyEncoder::encode(arch_val, output_hash, id);
                if let Some(data) = storage.get(&key) {
                    if let Ok(item) = postcard::from_bytes::<T>(&data) {
                        match filter_fn {
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

    async fn fetch_batched(
        storage: &Arc<Storage>,
        ids: &[u128],
        output_hash: u64,
        filter_fn: &Option<Box<dyn Fn(&T) -> bool + Send + Sync>>,
    ) -> Vec<T> {
        // Archetype lookup (batch)
        let arch_keys: Vec<Vec<u8>> = ids
            .iter()
            .map(|id| Storage::entity_to_archetype_key(*id))
            .collect();
        let arch_values = storage.get_many(&arch_keys);

        // Build data keys
        let mut data_keys = Vec::new();
        for (id, arch_val) in ids.iter().zip(&arch_values) {
            if let Some(bytes) = arch_val {
                if bytes.len() >= 8 {
                    let archetype_id = u64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                        bytes[7],
                    ]);
                    data_keys.push(KeyEncoder::encode(archetype_id, output_hash, *id));
                }
            }
        }

        // Data fetch (batch)
        let raw_results = storage.get_many(&data_keys);
        let num_items = raw_results.len();
        if num_items == 0 {
            return Vec::new();
        }

        // Deserialize
        let parallelism = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let chunk_size = (num_items + parallelism - 1) / parallelism;
        let mut handles = Vec::new();

        for chunk_idx in 0..parallelism {
            let start = chunk_idx * chunk_size;
            let end = (start + chunk_size).min(num_items);
            if start >= end {
                break;
            }
            let chunk: Vec<Option<Vec<u8>>> =
                raw_results[start..end].iter().map(|o| o.clone()).collect();

            handles.push(tokio::task::spawn_blocking(move || {
                let mut local = Vec::new();
                for raw in &chunk {
                    if let Some(bytes) = raw {
                        if let Ok(item) = postcard::from_bytes::<T>(bytes) {
                            local.push(item);
                        }
                    }
                }
                local
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(chunk) = handle.await {
                results.extend(chunk);
            }
        }

        // Apply filter
        if let Some(f) = filter_fn {
            results.retain(|item| f(item));
        }

        results
    }
}
