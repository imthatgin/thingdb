use crate::archetype::Registry;
use crate::storage::{KeyEncoder, Storage};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub struct Query<T> {
    storage: Arc<Storage>,
    registry: Option<Arc<Mutex<Registry>>>,
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
            registry: None,
            output_attr_hash,
            with_components: Vec::new(),
            without_components: Vec::new(),
            filter_fn: None,
        }
    }

    pub fn with_registry(mut self, registry: Option<Arc<Mutex<Registry>>>) -> Self {
        self.registry = registry;
        self
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
        // Archetype-aware path: use in-memory registry for dense, cache-friendly iteration.
        // Only active when the cache is warm (has at least one archetype).
        if let Some(registry) = &self.registry {
            let registry = registry.lock().unwrap();
            if registry.archetype_count() > 0 {
                let matching_arch_ids = registry.find_matching_archetypes(
                    self.output_attr_hash,
                    &self.with_components,
                    &self.without_components,
                );

                if matching_arch_ids.is_empty() {
                    return Vec::new();
                }

                let filter_fn = &self.filter_fn;
                let mut results = Vec::new();

                for &arch_id in &matching_arch_ids {
                    if let Some(archetype) = registry.get_archetype(arch_id) {
                        for data in archetype.iter_component(self.output_attr_hash) {
                            if let Ok(item) = postcard::from_bytes::<T>(data) {
                                if filter_fn.as_ref().map_or(true, |f| f(&item)) {
                                    results.push(item);
                                }
                            }
                        }
                    }
                }

                return results;
            }
            // Cold cache — fall through to RocksDB
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archetype::Registry;
    use crate::tx::Tx;
    use crate::Attribute;
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_storage() -> Arc<Storage> {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = format!("/tmp/test_thingdb_query_{}", counter);
        let _ = std::fs::remove_dir_all(&path);
        Arc::new(Storage::open(&path).unwrap())
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Player;

    impl Attribute for Player {
        const NAME: &'static str = "Player";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Enemy;

    impl Attribute for Enemy {
        const NAME: &'static str = "Enemy";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Position {
        x: f64,
        y: f64,
    }

    impl Attribute for Position {
        const NAME: &'static str = "Position";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Health(u32);

    impl Attribute for Health {
        const NAME: &'static str = "Health";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Score(i64);

    impl Attribute for Score {
        const NAME: &'static str = "Score";
    }

    /// Helper: populate storage with data using a transaction, returns the registry
    /// (which will be warmed by commit). Also returns a fresh storage clone you can
    /// pass to Query.
    async fn setup_data() -> (Arc<Storage>, Arc<Mutex<Registry>>) {
        let storage = test_storage();
        let registry = Arc::new(Mutex::new(Registry::default()));
        let mut tx = Tx::new(storage.clone(), Some(registry.clone()));

        let p1 = tx.spawn().await;
        tx.add(p1, Player).await.unwrap();
        tx.add(p1, Position { x: 1.0, y: 2.0 }).await.unwrap();
        tx.add(p1, Health(100)).await.unwrap();

        let p2 = tx.spawn().await;
        tx.add(p2, Player).await.unwrap();
        tx.add(p2, Position { x: 3.0, y: 4.0 }).await.unwrap();

        let e1 = tx.spawn().await;
        tx.add(e1, Enemy).await.unwrap();
        tx.add(e1, Health(50)).await.unwrap();

        tx.commit().await.unwrap();

        (storage, registry)
    }

    #[tokio::test]
    async fn test_query_cache_single_component() {
        let (storage, registry) = setup_data().await;
        let results: Vec<Player> = Query::new(storage)
            .with_registry(Some(registry))
            .run()
            .await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_query_cache_with_filter() {
        let (storage, registry) = setup_data().await;
        let results: Vec<Health> = Query::new(storage)
            .with_registry(Some(registry))
            .filter(|h: &Health| h.0 > 60)
            .run()
            .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 100);
    }

    #[tokio::test]
    async fn test_query_cache_with_clause() {
        let (storage, registry) = setup_data().await;
        let results: Vec<Position> = Query::new(storage)
            .with_registry(Some(registry))
            .with::<Player>()
            .run()
            .await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_query_cache_without_clause() {
        let (storage, registry) = setup_data().await;
        let results: Vec<Health> = Query::new(storage)
            .with_registry(Some(registry))
            .without::<Enemy>()
            .run()
            .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 100);
    }

    #[tokio::test]
    async fn test_query_cache_with_and_without() {
        let (storage, registry) = setup_data().await;
        let results: Vec<Health> = Query::new(storage)
            .with_registry(Some(registry))
            .with::<Player>()
            .without::<Enemy>()
            .run()
            .await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_query_cache_filter_rejects_all() {
        let (storage, registry) = setup_data().await;
        let results: Vec<Health> = Query::new(storage)
            .with_registry(Some(registry))
            .filter(|h: &Health| h.0 > 999)
            .run()
            .await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_query_cache_with_matches_nothing() {
        let (storage, registry) = setup_data().await;
        let results: Vec<Player> = Query::new(storage)
            .with_registry(Some(registry))
            .with::<Score>()
            .run()
            .await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_query_cache_empty_world() {
        let storage = test_storage();
        let registry = Arc::new(Mutex::new(Registry::default()));
        let results: Vec<Player> = Query::new(storage)
            .with_registry(Some(registry))
            .run()
            .await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_query_rocksdb_path_single_component() {
        let (storage, _) = setup_data().await;
        let cold_registry = Arc::new(Mutex::new(Registry::default()));
        let results: Vec<Player> = Query::new(storage)
            .with_registry(Some(cold_registry))
            .run()
            .await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_query_rocksdb_path_with_filter() {
        let (storage, _) = setup_data().await;
        let results: Vec<Health> = Query::new(storage)
            .filter(|h: &Health| h.0 > 60)
            .run()
            .await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_query_rocksdb_path_with_clause() {
        let (storage, _) = setup_data().await;
        let results: Vec<Position> = Query::new(storage).with::<Player>().run().await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_query_rocksdb_path_without_clause() {
        let (storage, _) = setup_data().await;
        let results: Vec<Health> = Query::new(storage).without::<Enemy>().run().await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_query_rocksdb_path_with_and_without() {
        let (storage, _) = setup_data().await;
        let results: Vec<Health> = Query::new(storage)
            .with::<Player>()
            .without::<Enemy>()
            .run()
            .await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_query_rocksdb_path_empty_world() {
        let storage = test_storage();
        let results: Vec<Player> = Query::new(storage).run().await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_query_rocksdb_path_filter_rejects_all() {
        let (storage, _) = setup_data().await;
        let results: Vec<Health> = Query::new(storage)
            .filter(|h: &Health| h.0 > 999)
            .run()
            .await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_query_rocksdb_path_no_registry() {
        let (storage, _) = setup_data().await;
        let results: Vec<Enemy> = Query::new(storage).run().await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_query_cache_and_rocksdb_return_same_results() {
        let storage = test_storage();
        let registry = Arc::new(Mutex::new(Registry::default()));
        let mut tx = Tx::new(storage.clone(), Some(registry.clone()));
        let id = tx.spawn().await;
        tx.add(id, Score(42)).await.unwrap();
        tx.commit().await.unwrap();

        let cache_results: Vec<Score> = Query::new(storage.clone())
            .with_registry(Some(registry.clone()))
            .run()
            .await;

        let cold_registry = Arc::new(Mutex::new(Registry::default()));
        let rocks_results: Vec<Score> = Query::new(storage)
            .with_registry(Some(cold_registry))
            .run()
            .await;

        assert_eq!(cache_results.len(), rocks_results.len());
        assert_eq!(cache_results[0].0, rocks_results[0].0);
    }

    #[tokio::test]
    async fn test_query_batched_fetch_path() {
        let storage = test_storage();
        let registry = Arc::new(Mutex::new(Registry::default()));
        let mut tx = Tx::new(storage.clone(), Some(registry.clone()));

        let mut ids = Vec::new();
        for i in 0..500 {
            let id = tx.spawn().await;
            tx.add(id, Score(i as i64)).await.unwrap();
            ids.push(id);
        }
        tx.commit().await.unwrap();

        let results: Vec<Score> = Query::new(storage).run().await;

        assert_eq!(results.len(), 500);

        let mut values: Vec<i64> = results.into_iter().map(|s| s.0).collect();
        values.sort();
        assert_eq!(values[0], 0);
        assert_eq!(values[499], 499);
    }

    #[tokio::test]
    async fn test_query_batched_fetch_path_with_filter() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let mut ids = Vec::new();
        for i in 0..500 {
            let id = tx.spawn().await;
            tx.add(id, Score(i as i64)).await.unwrap();
            ids.push(id);
        }
        tx.commit().await.unwrap();

        let results: Vec<Score> = Query::new(storage)
            .filter(|s: &Score| s.0 >= 100 && s.0 < 200)
            .run()
            .await;

        assert_eq!(results.len(), 100);
    }

    #[tokio::test]
    async fn test_query_batched_fetch_path_with_clause() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let mut ids = Vec::new();
        for i in 0..500 {
            let id = tx.spawn().await;
            tx.add(id, Score(i as i64)).await.unwrap();
            if i % 2 == 0 {
                tx.add(id, Player).await.unwrap();
            }
            ids.push(id);
        }
        tx.commit().await.unwrap();

        let results: Vec<Score> = Query::new(storage).with::<Player>().run().await;

        assert_eq!(results.len(), 250);
    }

    #[tokio::test]
    async fn test_query_builder_chaining() {
        let (storage, registry) = setup_data().await;
        let q = Query::new(storage)
            .with_registry(Some(registry))
            .with::<Player>()
            .without::<Enemy>()
            .filter(|_: &Health| true);

        let _results: Vec<Health> = q.run().await;
    }

    #[tokio::test]
    async fn test_query_with_multiple_with_hashes() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Player).await.unwrap();
        tx.add(id, Position { x: 0.0, y: 0.0 }).await.unwrap();
        tx.add(id, Health(50)).await.unwrap();
        tx.commit().await.unwrap();

        let results: Vec<Position> = Query::new(storage)
            .with::<Player>()
            .with::<Health>()
            .run()
            .await;
        assert_eq!(results.len(), 1);
    }
}
