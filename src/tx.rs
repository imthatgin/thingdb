use crate::archetype::Registry;
use crate::storage::{KeyEncoder, Storage};
use rust_rocksdb::WriteBatch;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub struct Tx {
    storage: Arc<Storage>,
    cache: Option<Arc<Mutex<Registry>>>,
    puts: Vec<(Vec<u8>, Vec<u8>)>,
    deletes: Vec<Vec<u8>>,
    pending_attrs: HashMap<u128, HashSet<u64>>,
    pending_data: HashMap<Vec<u8>, Option<Vec<u8>>>,
}

impl Tx {
    pub fn new(storage: Arc<Storage>, cache: Option<Arc<Mutex<Registry>>>) -> Self {
        Self {
            storage,
            cache,
            puts: Vec::new(),
            deletes: Vec::new(),
            pending_attrs: HashMap::new(),
            pending_data: HashMap::new(),
        }
    }

    pub async fn spawn(&self) -> u128 {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        NEXT_ID.fetch_add(1, Ordering::Relaxed) as u128
    }

    fn get_entity_attrs(&mut self, thing: u128) -> HashSet<u64> {
        if let Some(attrs) = self.pending_attrs.get(&thing) {
            return attrs.clone();
        }
        let attrs: HashSet<u64> = self.storage.get_entity_attrs(thing).into_iter().collect();
        self.pending_attrs.insert(thing, attrs.clone());
        attrs
    }

    fn read_data(&self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(entry) = self.pending_data.get(key) {
            return entry.clone();
        }
        self.storage.get(key)
    }

    fn buf_put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.pending_data.insert(key.clone(), Some(value.clone()));
        self.puts.push((key, value));
    }

    fn buf_delete(&mut self, key: Vec<u8>) {
        self.pending_data.insert(key.clone(), None);
        self.deletes.push(key);
    }

    fn buf_add_attr(&mut self, thing: u128, hash: u64) {
        let key = Storage::entity_attr_key(thing, hash);
        let value = hash.to_le_bytes().to_vec();
        self.pending_attrs
            .entry(thing)
            .or_insert_with(|| self.storage.get_entity_attrs(thing).into_iter().collect())
            .insert(hash);
        self.puts.push((key, value));
    }

    fn buf_remove_attr(&mut self, thing: u128, hash: u64) {
        let key = Storage::entity_attr_key(thing, hash);
        if let Some(attrs) = self.pending_attrs.get_mut(&thing) {
            attrs.remove(&hash);
        }
        self.deletes.push(key);
    }

    fn buf_set_archetype(&mut self, thing: u128, archetype_id: u64) {
        let key = Storage::entity_to_archetype_key(thing);
        let value = archetype_id.to_le_bytes().to_vec();
        self.puts.push((key, value));
    }

    fn buf_delete_archetype(&mut self, thing: u128) {
        let key = Storage::entity_to_archetype_key(thing);
        self.deletes.push(key);
    }

    fn buf_add_reverse_index(&mut self, thing: u128, hash: u64) {
        let key = Storage::attr_index_key(hash, thing);
        self.puts.push((key, vec![]));
    }

    fn buf_remove_reverse_index(&mut self, thing: u128, hash: u64) {
        let key = Storage::attr_index_key(hash, thing);
        self.deletes.push(key);
    }

    pub async fn relate<E: crate::Edge>(
        &mut self,
        from: u128,
        to: u128,
        data: E,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hash = crate::hash_name(<E as crate::Edge>::NAME);
        let fwd_key = Storage::edge_key(hash, from, to);
        let rev_key = Storage::reverse_edge_key(hash, to, from);
        let bytes = postcard::to_allocvec(&data)?;
        self.buf_put(fwd_key, bytes.clone());
        self.buf_put(rev_key, bytes);
        Ok(())
    }

    pub async fn unrelate<E: crate::Edge>(
        &mut self,
        from: u128,
        to: u128,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hash = crate::hash_name(<E as crate::Edge>::NAME);
        let fwd_key = Storage::edge_key(hash, from, to);
        let rev_key = Storage::reverse_edge_key(hash, to, from);
        self.buf_delete(fwd_key);
        self.buf_delete(rev_key);
        Ok(())
    }

    pub async fn unrelate_all_from<E: crate::Edge>(
        &mut self,
        from: u128,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hash = crate::hash_name(<E as crate::Edge>::NAME);
        let prefix = Storage::outgoing_edge_prefix(hash, from);
        let mut keys_to_delete = Vec::new();
        self.storage.for_each_with_prefix(&prefix, |key, _value| {
            keys_to_delete.push(key.to_vec());
        });
        for fwd_key in &keys_to_delete {
            if let Some(tgt) = Storage::parse_edge_target(fwd_key) {
                let rev_key = Storage::reverse_edge_key(hash, tgt, from);
                self.buf_delete(rev_key);
            }
            self.buf_delete(fwd_key.clone());
        }
        Ok(())
    }

    pub async fn add<T: crate::Attribute + 'static>(
        &mut self,
        thing: u128,
        attr: T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hash = crate::hash_name(<T as crate::Attribute>::NAME);

        let mut attrs = self.get_entity_attrs(thing);
        let old_archetype = if !attrs.is_empty() {
            Some(Registry::compute_archetype_id(&attrs))
        } else {
            None
        };

        if !attrs.insert(hash) {
            return Ok(());
        }

        let new_archetype = Registry::compute_archetype_id(&attrs);

        // Migrate existing data from old archetype to new archetype
        if let Some(old_arch) = old_archetype {
            for &old_hash in &attrs
                .iter()
                .copied()
                .filter(|&h| h != hash)
                .collect::<Vec<u64>>()
            {
                let old_key = KeyEncoder::encode(old_arch, old_hash, thing);
                if let Some(data) = self.read_data(&old_key) {
                    let new_key = KeyEncoder::encode(new_archetype, old_hash, thing);
                    self.buf_put(new_key, data);
                }
            }
        }

        // Store new attribute data
        let key = KeyEncoder::encode(new_archetype, hash, thing);
        let bytes = postcard::to_allocvec(&attr)?;
        self.buf_put(key, bytes);
        self.buf_set_archetype(thing, new_archetype);
        self.buf_add_attr(thing, hash);
        self.buf_add_reverse_index(thing, hash);

        Ok(())
    }

    pub async fn set<T: crate::Attribute + 'static>(
        &mut self,
        thing: u128,
        attr: T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hash = crate::hash_name(<T as crate::Attribute>::NAME);
        let attrs = self.get_entity_attrs(thing);

        if attrs.contains(&hash) {
            // Overwrite in-place (same archetype)
            let archetype_id = Registry::compute_archetype_id(&attrs);
            let key = KeyEncoder::encode(archetype_id, hash, thing);
            let bytes = postcard::to_allocvec(&attr)?;
            self.buf_put(key, bytes);
        } else {
            // New component — full add logic
            let old_archetype = if !attrs.is_empty() {
                Some(Registry::compute_archetype_id(&attrs))
            } else {
                None
            };
            let new_attrs: HashSet<u64> =
                attrs.iter().copied().chain(std::iter::once(hash)).collect();
            let new_archetype = Registry::compute_archetype_id(&new_attrs);

            if let Some(old_arch) = old_archetype {
                for &old_hash in &attrs {
                    let old_key = KeyEncoder::encode(old_arch, old_hash, thing);
                    if let Some(data) = self.read_data(&old_key) {
                        let new_key = KeyEncoder::encode(new_archetype, old_hash, thing);
                        self.buf_put(new_key, data);
                    }
                }
            }

            let key = KeyEncoder::encode(new_archetype, hash, thing);
            let bytes = postcard::to_allocvec(&attr)?;
            self.buf_put(key, bytes);
            self.buf_set_archetype(thing, new_archetype);
            self.buf_add_attr(thing, hash);
            self.buf_add_reverse_index(thing, hash);
        }

        Ok(())
    }

    pub async fn remove<T: crate::Attribute + 'static>(
        &mut self,
        thing: u128,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hash = crate::hash_name(<T as crate::Attribute>::NAME);

        let mut attrs = self.get_entity_attrs(thing);
        if !attrs.remove(&hash) {
            return Ok(());
        }

        let old_archetype = Registry::compute_archetype_id(
            &attrs.iter().copied().chain(std::iter::once(hash)).collect(),
        );

        // Delete old data key
        self.buf_delete(KeyEncoder::encode(old_archetype, hash, thing));

        // Remove tracking
        self.buf_remove_attr(thing, hash);
        self.buf_remove_reverse_index(thing, hash);

        if attrs.is_empty() {
            self.buf_delete_archetype(thing);
        } else {
            let new_archetype = Registry::compute_archetype_id(&attrs);
            for &remaining_hash in &attrs {
                let old_key = KeyEncoder::encode(old_archetype, remaining_hash, thing);
                if let Some(data) = self.read_data(&old_key) {
                    let new_key = KeyEncoder::encode(new_archetype, remaining_hash, thing);
                    self.buf_put(new_key, data);
                }
            }
            self.buf_set_archetype(thing, new_archetype);
        }

        Ok(())
    }

    pub async fn destroy(&mut self, thing: u128) -> Result<(), Box<dyn std::error::Error>> {
        let attrs: Vec<u64> = self.get_entity_attrs(thing).into_iter().collect();
        if attrs.is_empty() {
            return Ok(());
        }

        let attr_set: HashSet<u64> = attrs.into_iter().collect();
        let archetype_id = Registry::compute_archetype_id(&attr_set);

        for &hash in &attr_set {
            self.buf_delete(KeyEncoder::encode(archetype_id, hash, thing));
            self.buf_remove_attr(thing, hash);
            self.buf_remove_reverse_index(thing, hash);
        }

        self.buf_delete_archetype(thing);

        Ok(())
    }

    pub async fn commit(self) -> Result<(), Box<dyn std::error::Error>> {
        let mut batch = WriteBatch::default();

        // Apply deletes before puts so that if a key is both deleted and re-put,
        // the put wins (RocksDB applies ops in order within a batch)
        for key in &self.deletes {
            batch.delete(key);
        }
        for (key, value) in &self.puts {
            batch.put(key, value);
        }

        self.storage.write_batch(&batch)?;

        // In memory archetype cache
        if let Some(cache) = &self.cache {
            let mut registry = cache.lock().unwrap();

            for (&thing_id, attrs) in &self.pending_attrs {
                if attrs.is_empty() {
                    registry.remove_entity(thing_id);
                    continue;
                }

                let arch_id = Registry::compute_archetype_id(attrs);
                let mut components: HashMap<u64, Vec<u8>> = HashMap::new();

                for &attr_hash in attrs {
                    let data_key = KeyEncoder::encode(arch_id, attr_hash, thing_id);
                    if let Some(Some(data)) = self.pending_data.get(&data_key) {
                        components.insert(attr_hash, data.clone());
                    } else if let Some(data) = registry.read_component(thing_id, attr_hash) {
                        components.insert(attr_hash, data.clone());
                    }
                }

                registry.set_entity_components(thing_id, components);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archetype::Registry;
    use crate::Attribute;
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_storage() -> Arc<Storage> {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = format!("/tmp/test_thingdb_tx_{}", counter);
        let _ = std::fs::remove_dir_all(&path);
        Arc::new(Storage::open(&path).unwrap())
    }

    fn test_tx_no_cache() -> Tx {
        Tx::new(test_storage(), None)
    }

    #[derive(Serialize, Deserialize)]
    struct Tag;

    impl Attribute for Tag {
        const NAME: &'static str = "Tag";
    }

    #[derive(Serialize, Deserialize)]
    struct Pos {
        x: f64,
        y: f64,
    }

    impl Attribute for Pos {
        const NAME: &'static str = "Pos";
    }

    #[derive(Serialize, Deserialize)]
    struct Health(u32);

    impl Attribute for Health {
        const NAME: &'static str = "Health";
    }

    #[derive(Serialize, Deserialize)]
    struct Score(i64);

    impl Attribute for Score {
        const NAME: &'static str = "Score";
    }

    #[tokio::test]
    async fn test_spawn_returns_incrementing_ids() {
        let tx = test_tx_no_cache();
        let id1 = tx.spawn().await;
        let id2 = tx.spawn().await;
        let id3 = tx.spawn().await;
        assert!(id2 > id1);
        assert!(id3 > id2);
    }

    #[tokio::test]
    async fn test_add_single_component_and_commit() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(id);
        assert_eq!(attrs.len(), 1);
        assert!(attrs.contains(&crate::hash_name("Tag")));
    }

    #[tokio::test]
    async fn test_add_multiple_components() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.add(id, Pos { x: 1.0, y: 2.0 }).await.unwrap();
        tx.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(id);
        assert_eq!(attrs.len(), 2);
    }

    #[tokio::test]
    async fn test_add_duplicate_component_is_noop() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.add(id, Tag).await.unwrap();
        tx.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(id);
        assert_eq!(attrs.len(), 1);
    }

    #[tokio::test]
    async fn test_set_updates_existing_component_value() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Score(10)).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.set(id, Score(99)).await.unwrap();
        tx2.commit().await.unwrap();

        let attrs: HashSet<u64> = storage.get_entity_attrs(id).into_iter().collect();
        let arch_id = Registry::compute_archetype_id(&attrs);
        let key = KeyEncoder::encode(arch_id, crate::hash_name("Score"), id);
        let data = storage.get(&key).unwrap();
        let score: Score = postcard::from_bytes(&data).unwrap();
        assert_eq!(score.0, 99);
    }

    #[tokio::test]
    async fn test_set_adds_new_component_when_missing() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.set(id, Score(42)).await.unwrap();
        tx2.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(id);
        assert_eq!(attrs.len(), 2);
    }

    #[tokio::test]
    async fn test_remove_component() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.add(id, Score(5)).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.remove::<Tag>(id).await.unwrap();
        tx2.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(id);
        assert_eq!(attrs.len(), 1);
        assert!(!attrs.contains(&crate::hash_name("Tag")));
    }

    #[tokio::test]
    async fn test_remove_last_component_clears_entity() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.remove::<Tag>(id).await.unwrap();
        tx2.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(id);
        assert!(attrs.is_empty());
    }

    #[tokio::test]
    async fn test_remove_nonexistent_component_is_noop() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.remove::<Health>(id).await.unwrap();
        tx2.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(id);
        assert_eq!(attrs.len(), 1);
    }

    #[tokio::test]
    async fn test_destroy_entity_removes_all_components() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.add(id, Score(7)).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.destroy(id).await.unwrap();
        tx2.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(id);
        assert!(attrs.is_empty());
    }

    #[tokio::test]
    async fn test_destroy_nonexistent_entity_is_noop() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        tx.destroy(999).await.unwrap();
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn test_commit_populates_in_memory_cache() {
        let storage = test_storage();
        let registry = Arc::new(Mutex::new(Registry::default()));
        let mut tx = Tx::new(storage.clone(), Some(registry.clone()));
        let id = tx.spawn().await;
        tx.add(id, Pos { x: 3.0, y: 4.0 }).await.unwrap();
        tx.add(id, Health(100)).await.unwrap();
        tx.commit().await.unwrap();

        let reg = registry.lock().unwrap();
        let arch_id = reg.entity_archetype_id(id);
        assert!(arch_id.is_some(), "cache should have archetype for entity");
        let data = reg.read_component(id, crate::hash_name("Health"));
        assert!(data.is_some(), "cache should have Health data");
    }

    #[tokio::test]
    async fn test_commit_without_cache_does_not_panic() {
        let mut tx = test_tx_no_cache();
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_entities_in_single_transaction() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let a = tx.spawn().await;
        let b = tx.spawn().await;
        tx.add(a, Tag).await.unwrap();
        tx.add(b, Score(1)).await.unwrap();
        tx.commit().await.unwrap();

        let attrs_a = storage.get_entity_attrs(a);
        let attrs_b = storage.get_entity_attrs(b);
        assert_eq!(attrs_a.len(), 1);
        assert!(attrs_a.contains(&crate::hash_name("Tag")));
        assert_eq!(attrs_b.len(), 1);
        assert!(attrs_b.contains(&crate::hash_name("Score")));
    }

    #[tokio::test]
    async fn test_add_after_remove_in_same_transaction() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.add(id, Score(5)).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.remove::<Tag>(id).await.unwrap();
        tx2.add(id, Health(10)).await.unwrap();
        tx2.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(id);
        assert_eq!(attrs.len(), 2);
        assert!(!attrs.contains(&crate::hash_name("Tag")));
        assert!(attrs.contains(&crate::hash_name("Health")));
    }

    #[tokio::test]
    async fn test_archetype_migration_on_add_through_tx() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.commit().await.unwrap();

        let attrs_1: HashSet<u64> = storage.get_entity_attrs(id).into_iter().collect();
        let arch_1 = Registry::compute_archetype_id(&attrs_1);

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.add(id, Score(10)).await.unwrap();
        tx2.commit().await.unwrap();

        let attrs_2: HashSet<u64> = storage.get_entity_attrs(id).into_iter().collect();
        let arch_2 = Registry::compute_archetype_id(&attrs_2);
        assert_ne!(arch_1, arch_2, "adding a component should change archetype");
    }

    #[tokio::test]
    async fn test_archetype_migration_on_remove_through_tx() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.add(id, Score(10)).await.unwrap();
        tx.commit().await.unwrap();

        let attrs_1: HashSet<u64> = storage.get_entity_attrs(id).into_iter().collect();
        let arch_1 = Registry::compute_archetype_id(&attrs_1);

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.remove::<Tag>(id).await.unwrap();
        tx2.commit().await.unwrap();

        let attrs_2: HashSet<u64> = storage.get_entity_attrs(id).into_iter().collect();
        let arch_2 = Registry::compute_archetype_id(&attrs_2);
        assert_ne!(
            arch_1, arch_2,
            "removing a component should change archetype"
        );
    }

    #[tokio::test]
    async fn test_set_on_fresh_entity_creates_component() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.set(id, Score(100)).await.unwrap();
        tx.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(id);
        assert_eq!(attrs.len(), 1);
    }

    #[tokio::test]
    async fn test_destroy_twice_is_noop() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.destroy(id).await.unwrap();
        tx2.destroy(id).await.unwrap();
        tx2.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(id);
        assert!(attrs.is_empty());
    }

    #[tokio::test]
    async fn test_spawn_with_no_cache_still_works() {
        let tx = Tx::new(test_storage(), None);
        let id = tx.spawn().await;
        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_add_component_preserves_existing_data_across_migration() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Pos { x: 1.0, y: 2.0 }).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.add(id, Tag).await.unwrap();
        tx2.commit().await.unwrap();

        let attrs: HashSet<u64> = storage.get_entity_attrs(id).into_iter().collect();
        let arch_id = Registry::compute_archetype_id(&attrs);
        let key = KeyEncoder::encode(arch_id, crate::hash_name("Pos"), id);
        let data = storage.get(&key).unwrap();
        let pos: Pos = postcard::from_bytes(&data).unwrap();
        assert!((pos.x - 1.0).abs() < 1e-9);
        assert!((pos.y - 2.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn test_reverse_index_updated_on_add() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.commit().await.unwrap();

        let entities = storage.get_entities_with_attr(crate::hash_name("Tag"));
        assert!(entities.contains(&id));
    }

    #[tokio::test]
    async fn test_reverse_index_updated_on_remove() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.remove::<Tag>(id).await.unwrap();
        tx2.commit().await.unwrap();

        let entities = storage.get_entities_with_attr(crate::hash_name("Tag"));
        assert!(!entities.contains(&id));
    }

    #[tokio::test]
    async fn test_reverse_index_updated_on_destroy() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let id = tx.spawn().await;
        tx.add(id, Tag).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.destroy(id).await.unwrap();
        tx2.commit().await.unwrap();

        let entities = storage.get_entities_with_attr(crate::hash_name("Tag"));
        assert!(!entities.contains(&id));
    }
}
