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