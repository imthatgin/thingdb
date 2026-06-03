use rust_rocksdb::{IteratorMode, WriteBatch, DB};
use std::error::Error;
use std::sync::Arc;

pub struct Storage {
    db: Arc<DB>,
}

impl Clone for Storage {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
        }
    }
}

impl Storage {
    pub fn open(path: &str) -> Result<Self, Box<dyn Error>> {
        let db = DB::open_default(path)?;
        Ok(Self { db: Arc::new(db) })
    }

    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), Box<dyn Error>> {
        self.db.put(key, value)?;
        Ok(())
    }

    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.db.get(key).ok().flatten()
    }

    pub async fn delete(&self, key: &[u8]) -> Result<(), Box<dyn Error>> {
        self.db.delete(key)?;
        Ok(())
    }

    pub fn for_each_with_prefix<F>(&self, prefix: &[u8], mut f: F)
    where
        F: FnMut(&[u8], &[u8]),
    {
        let mode = IteratorMode::From(prefix, rust_rocksdb::Direction::Forward);
        for item in self.db.iterator(mode) {
            match item {
                Ok((key, value)) => {
                    if !key.starts_with(prefix) {
                        break;
                    }
                    f(&key, &value);
                }
                Err(_) => break,
            }
        }
    }

    pub fn write_batch(&self, batch: &WriteBatch) -> Result<(), Box<dyn Error>> {
        self.db.write(batch)?;
        Ok(())
    }

    pub fn get_many(&self, keys: &[Vec<u8>]) -> Vec<Option<Vec<u8>>> {
        self.db
            .multi_get(keys)
            .into_iter()
            .map(|r| r.ok().flatten())
            .collect()
    }

    #[allow(clippy::await_solo)]
    pub async fn set_entity_archetype(
        &self,
        thing_id: u128,
        archetype_id: u64,
    ) -> Result<(), Box<dyn Error>> {
        let key = Self::entity_to_archetype_key(thing_id);
        self.db.put(&key, &archetype_id.to_le_bytes())?;
        Ok(())
    }

    pub fn get_entity_archetype(&self, thing_id: u128) -> Option<u64> {
        let key = Self::entity_to_archetype_key(thing_id);
        self.get(&key).map(|v| {
            if v.len() >= 8 {
                u64::from_le_bytes([v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]])
            } else {
                0
            }
        })
    }

    #[allow(clippy::await_solo)]
    pub async fn add_entity_attribute(
        &self,
        thing_id: u128,
        attr_hash: u64,
    ) -> Result<(), Box<dyn Error>> {
        let key = Self::entity_attr_key(thing_id, attr_hash);
        self.db.put(&key, &attr_hash.to_le_bytes())?;
        Ok(())
    }

    #[allow(clippy::await_solo)]
    pub async fn remove_entity_attribute(
        &self,
        thing_id: u128,
        attr_hash: u64,
    ) -> Result<(), Box<dyn Error>> {
        let key = Self::entity_attr_key(thing_id, attr_hash);
        self.db.delete(&key)?;
        Ok(())
    }

    #[allow(clippy::await_solo)]
    pub async fn delete_entity_archetype(&self, thing_id: u128) -> Result<(), Box<dyn Error>> {
        let key = Self::entity_to_archetype_key(thing_id);
        self.db.delete(&key)?;
        Ok(())
    }

    pub fn get_entity_attrs(&self, thing_id: u128) -> Vec<u64> {
        let prefix = Self::entity_attrs_prefix(thing_id);
        let mut attrs = Vec::new();
        self.for_each_with_prefix(&prefix, |_key, value| {
            if value.len() >= 8 {
                attrs.push(u64::from_le_bytes([
                    value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
                ]));
            }
        });
        attrs
    }

    pub fn add_entity_reverse_index(
        &self,
        thing_id: u128,
        attr_hash: u64,
    ) -> Result<(), Box<dyn Error>> {
        let key = Self::attr_index_key(attr_hash, thing_id);
        self.db.put(&key, b"")?;
        Ok(())
    }

    pub fn remove_entity_reverse_index(
        &self,
        thing_id: u128,
        attr_hash: u64,
    ) -> Result<(), Box<dyn Error>> {
        let key = Self::attr_index_key(attr_hash, thing_id);
        self.db.delete(&key)?;
        Ok(())
    }

    pub fn get_entities_with_attr(&self, attr_hash: u64) -> Vec<u128> {
        let prefix = Self::attr_index_prefix(attr_hash);
        let mut ids = Vec::new();
        self.for_each_with_prefix(&prefix, |key, _value| {
            if key.len() >= 27 {
                let id_bytes: [u8; 16] = key[11..27].try_into().unwrap();
                ids.push(u128::from_le_bytes(id_bytes));
            }
        });
        ids
    }

    pub(crate) fn attr_index_key(attr_hash: u64, thing_id: u128) -> Vec<u8> {
        let mut key = b"ai:".to_vec();
        key.extend_from_slice(&attr_hash.to_le_bytes());
        key.extend_from_slice(&thing_id.to_le_bytes());
        key
    }

    fn attr_index_prefix(attr_hash: u64) -> Vec<u8> {
        let mut prefix = b"ai:".to_vec();
        prefix.extend_from_slice(&attr_hash.to_le_bytes());
        prefix
    }

    pub(crate) fn entity_to_archetype_key(thing_id: u128) -> Vec<u8> {
        let mut key = b"eta:".to_vec();
        key.extend_from_slice(&thing_id.to_le_bytes());
        key
    }

    pub(crate) fn entity_attr_key(thing_id: u128, attr_hash: u64) -> Vec<u8> {
        let mut key = b"ea:".to_vec();
        key.extend_from_slice(&thing_id.to_le_bytes());
        key.extend_from_slice(&attr_hash.to_le_bytes());
        key
    }

    fn entity_attrs_prefix(thing_id: u128) -> Vec<u8> {
        let mut prefix = b"ea:".to_vec();
        prefix.extend_from_slice(&thing_id.to_le_bytes());
        prefix
    }
}

pub struct KeyEncoder;

impl KeyEncoder {
    pub fn encode(archetype_id: u64, attr_hash: u64, thing_id: u128) -> Vec<u8> {
        let mut key = Vec::with_capacity(32);
        key.extend_from_slice(&archetype_id.to_le_bytes());
        key.extend_from_slice(&attr_hash.to_le_bytes());
        key.extend_from_slice(&thing_id.to_le_bytes());
        key
    }

    pub fn decode(key: &[u8]) -> Option<(u64, u64, u128)> {
        if key.len() != 32 {
            return None;
        }
        let archetype_bytes: [u8; 8] = key[0..8].try_into().ok()?;
        let attr_bytes: [u8; 8] = key[8..16].try_into().ok()?;
        let thing_bytes: [u8; 16] = key[16..32].try_into().ok()?;
        Some((
            u64::from_le_bytes(archetype_bytes),
            u64::from_le_bytes(attr_bytes),
            u128::from_le_bytes(thing_bytes),
        ))
    }

    pub fn encode_archetype_prefix(archetype_id: u64) -> Vec<u8> {
        archetype_id.to_le_bytes().to_vec()
    }

    pub fn encode_attr_prefix(archetype_id: u64, attr_hash: u64) -> Vec<u8> {
        let mut prefix = Vec::with_capacity(16);
        prefix.extend_from_slice(&archetype_id.to_le_bytes());
        prefix.extend_from_slice(&attr_hash.to_le_bytes());
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_encoding() {
        let key = KeyEncoder::encode(1, 2, 3);
        assert_eq!(key.len(), 32);

        let (a, h, t) = KeyEncoder::decode(&key).unwrap();
        assert_eq!(a, 1);
        assert_eq!(h, 2);
        assert_eq!(t, 3);
    }
}
