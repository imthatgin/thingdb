use crate::storage::{KeyEncoder, Storage};
use rust_rocksdb::WriteBatch;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct Tx {
    storage: Arc<Storage>,
    puts: Vec<(Vec<u8>, Vec<u8>)>,
    deletes: Vec<Vec<u8>>,
    pending_attrs: HashMap<u128, HashSet<u64>>,
    pending_data: HashMap<Vec<u8>, Option<Vec<u8>>>,
}

impl Tx {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self {
            storage,
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

    // ── buffered read helpers ───────────────────────────────────────

    fn get_entity_attrs(&self, thing: u128) -> HashSet<u64> {
        if let Some(attrs) = self.pending_attrs.get(&thing) {
            return attrs.clone();
        }
        self.storage.get_entity_attrs(thing).into_iter().collect()
    }

    fn read_data(&self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(entry) = self.pending_data.get(key) {
            return entry.clone();
        }
        self.storage.get(key)
    }

    // ── buffered write helpers ──────────────────────────────────────

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

    // ── public API ──────────────────────────────────────────────────

    pub async fn add<T: crate::Attribute + 'static>(
        &mut self,
        thing: u128,
        attr: T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hash = crate::hash_name(<T as crate::Attribute>::NAME);

        let mut attrs = self.get_entity_attrs(thing);
        let old_archetype = if !attrs.is_empty() {
            Some(compute_archetype_id(&attrs))
        } else {
            None
        };

        if !attrs.insert(hash) {
            return Ok(());
        }

        let new_archetype = compute_archetype_id(&attrs);

        // Migrate existing data from old archetype to new archetype
        if let Some(old_arch) = old_archetype {
            for &old_hash in &attrs.iter().copied().filter(|&h| h != hash).collect::<Vec<u64>>() {
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
            let archetype_id = compute_archetype_id(&attrs);
            let key = KeyEncoder::encode(archetype_id, hash, thing);
            let bytes = postcard::to_allocvec(&attr)?;
            self.buf_put(key, bytes);
        } else {
            // New component — full add logic
            let old_archetype = if !attrs.is_empty() {
                Some(compute_archetype_id(&attrs))
            } else {
                None
            };
            let new_attrs: HashSet<u64> = attrs.iter().copied().chain(std::iter::once(hash)).collect();
            let new_archetype = compute_archetype_id(&new_attrs);

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

        let old_archetype = compute_archetype_id(
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
            let new_archetype = compute_archetype_id(&attrs);
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
        let archetype_id = compute_archetype_id(&attr_set);

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
        // the put wins (RocksDB applies ops in order within a batch).
        for key in &self.deletes {
            batch.delete(key);
        }
        for (key, value) in &self.puts {
            batch.put(key, value);
        }

        self.storage.write_batch(&batch)?;
        Ok(())
    }
}

fn compute_archetype_id(attrs: &HashSet<u64>) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut sorted: Vec<u64> = attrs.iter().cloned().collect();
    sorted.sort();

    let mut hasher = DefaultHasher::new();
    for &h in &sorted {
        h.hash(&mut hasher);
    }
    hasher.finish()
}
