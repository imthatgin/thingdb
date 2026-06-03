use crate::storage::{KeyEncoder, Storage};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct Tx {
    storage: Arc<Storage>,
}

impl Tx {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }

    pub async fn spawn(&self) -> u128 {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        NEXT_ID.fetch_add(1, Ordering::Relaxed) as u128
    }

    #[allow(clippy::await_solo)]
    pub async fn add<T: crate::Attribute + 'static>(
        &self,
        thing: u128,
        attr: T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let attr_hash = <T as crate::Attribute>::NAME;
        let hash = crate::hash_name(attr_hash);

        // Get existing attributes for this entity
        let mut attrs: HashSet<u64> = self.storage.get_entity_attrs(thing).into_iter().collect();
        
        // Compute old archetype before adding new attribute
        let old_archetype;
        let is_first = attrs.is_empty();
        if !is_first {
            old_archetype = Some(compute_archetype_id(&attrs));
        } else {
            old_archetype = None;
        }

        let added = attrs.insert(hash);
        if !added {
            return Ok(());
        }

        // Compute new archetype from all attributes (including new one)
        let new_archetype = compute_archetype_id(&attrs);

        // Migrate existing data from old archetype to new archetype
        if let Some(old_arch) = old_archetype {
            for &old_hash in &attrs.iter().copied().filter(|&h| h != hash).collect::<Vec<u64>>() {
                let old_key = KeyEncoder::encode(old_arch, old_hash, thing);
                if let Some(data) = self.storage.get(&old_key) {
                    let new_key = KeyEncoder::encode(new_archetype, old_hash, thing);
                    self.storage.put(&new_key, &data).await?;
                }
            }
        }

        // Store new attribute at the unified archetype key
        let new_key = KeyEncoder::encode(new_archetype, hash, thing);
        let bytes = postcard::to_allocvec(&attr)?;
        self.storage.put(&new_key, &bytes).await?;

        // Set entity→archetype mapping
        self.storage.set_entity_archetype(thing, new_archetype).await?;
        
        // Track this attribute for the entity
        self.storage.add_entity_attribute(thing, hash).await?;
        
        // Maintain reverse index (attr_hash → thing_id)
        self.storage.add_entity_reverse_index(thing, hash)?;

        Ok(())
    }

    pub async fn set<T: crate::Attribute + 'static>(
        &self,
        thing: u128,
        attr: T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let attr_hash = <T as crate::Attribute>::NAME;
        let hash = crate::hash_name(attr_hash);

        let attrs: HashSet<u64> = self.storage.get_entity_attrs(thing).into_iter().collect();

        if attrs.contains(&hash) {
            // Component already exists — overwrite data in-place (same archetype)
            let archetype_id = compute_archetype_id(&attrs);
            let key = KeyEncoder::encode(archetype_id, hash, thing);
            let bytes = postcard::to_allocvec(&attr)?;
            self.storage.put(&key, &bytes).await?;
        } else {
            // Component is new — full add logic
            let new_attrs: HashSet<u64> = attrs.iter().copied().chain(std::iter::once(hash)).collect();
            let old_archetype = if !attrs.is_empty() {
                Some(compute_archetype_id(&attrs))
            } else {
                None
            };
            let new_archetype = compute_archetype_id(&new_attrs);

            if let Some(old_arch) = old_archetype {
                for &old_hash in &attrs {
                    let old_key = KeyEncoder::encode(old_arch, old_hash, thing);
                    if let Some(data) = self.storage.get(&old_key) {
                        let new_key = KeyEncoder::encode(new_archetype, old_hash, thing);
                        self.storage.put(&new_key, &data).await?;
                    }
                }
            }

            let new_key = KeyEncoder::encode(new_archetype, hash, thing);
            let bytes = postcard::to_allocvec(&attr)?;
            self.storage.put(&new_key, &bytes).await?;
            self.storage.set_entity_archetype(thing, new_archetype).await?;
            self.storage.add_entity_attribute(thing, hash).await?;
            self.storage.add_entity_reverse_index(thing, hash)?;
        }

        Ok(())
    }

    pub async fn remove<T: crate::Attribute + 'static>(
        &self,
        thing: u128,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let attr_hash = <T as crate::Attribute>::NAME;
        let hash = crate::hash_name(attr_hash);

        let mut attrs: HashSet<u64> = self.storage.get_entity_attrs(thing).into_iter().collect();

        if !attrs.remove(&hash) {
            return Ok(());
        }

        let old_archetype = compute_archetype_id(
            &attrs.iter().copied().chain(std::iter::once(hash)).collect()
        );

        // Delete the old data key
        self.storage.delete(&KeyEncoder::encode(old_archetype, hash, thing)).await?;

        // Remove from attribute tracking and reverse index
        self.storage.remove_entity_attribute(thing, hash).await?;
        self.storage.remove_entity_reverse_index(thing, hash)?;

        if attrs.is_empty() {
            // Last component removed — delete entity entirely
            self.storage.delete_entity_archetype(thing).await?;
        } else {
            // Migrate remaining data to the new archetype
            let new_archetype = compute_archetype_id(&attrs);
            for &remaining_hash in &attrs {
                let old_key = KeyEncoder::encode(old_archetype, remaining_hash, thing);
                if let Some(data) = self.storage.get(&old_key) {
                    let new_key = KeyEncoder::encode(new_archetype, remaining_hash, thing);
                    self.storage.put(&new_key, &data).await?;
                }
            }
            self.storage.set_entity_archetype(thing, new_archetype).await?;
        }

        Ok(())
    }

    pub async fn destroy(&self, thing: u128) -> Result<(), Box<dyn std::error::Error>> {
        let attrs: Vec<u64> = self.storage.get_entity_attrs(thing);
        if attrs.is_empty() {
            return Ok(());
        }

        let attr_set: HashSet<u64> = attrs.into_iter().collect();
        let archetype_id = compute_archetype_id(&attr_set);

        for &hash in &attr_set {
            self.storage.delete(&KeyEncoder::encode(archetype_id, hash, thing)).await?;
            self.storage.remove_entity_attribute(thing, hash).await?;
            self.storage.remove_entity_reverse_index(thing, hash)?;
        }

        self.storage.delete_entity_archetype(thing).await?;

        Ok(())
    }

    pub async fn commit(self) -> Result<(), Box<dyn std::error::Error>> {
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