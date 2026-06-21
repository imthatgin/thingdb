use std::error::Error;

pub struct Storage {
    db: fjall::Database,
    keyspace: fjall::Keyspace,
}

impl Clone for Storage {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            keyspace: self.keyspace.clone(),
        }
    }
}

impl Storage {
    pub fn open(path: &str) -> Result<Self, Box<dyn Error>> {
        let db = fjall::Database::builder(path).open()?;
        let keyspace = db.keyspace("default", || fjall::KeyspaceCreateOptions::default())?;
        Ok(Self { db, keyspace })
    }

    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), Box<dyn Error>> {
        self.keyspace.insert(key, value)?;
        Ok(())
    }

    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.keyspace.get(key).ok().flatten().map(|v| v.to_vec())
    }

    pub async fn delete(&self, key: &[u8]) -> Result<(), Box<dyn Error>> {
        self.keyspace.remove(key)?;
        Ok(())
    }

    pub fn for_each_with_prefix<F>(&self, prefix: &[u8], mut f: F)
    where
        F: FnMut(&[u8], &[u8]),
    {
        for item in self.keyspace.prefix(prefix) {
            match item.into_inner() {
                Ok((key, value)) => f(&key, &value),
                Err(_) => break,
            }
        }
    }

    pub fn commit_batch(
        &self,
        deletes: &[Vec<u8>],
        puts: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<(), Box<dyn Error>> {
        let mut batch = self.db.batch();
        for key in deletes {
            batch.remove(&self.keyspace, key.clone());
        }
        for (key, value) in puts {
            batch.insert(&self.keyspace, key.clone(), value.clone());
        }
        batch.commit()?;
        Ok(())
    }

    pub fn get_many(&self, keys: &[Vec<u8>]) -> Vec<Option<Vec<u8>>> {
        keys.iter()
            .map(|k| self.keyspace.get(k).ok().flatten().map(|v| v.to_vec()))
            .collect()
    }

    #[allow(clippy::await_solo)]
    pub async fn set_entity_archetype(
        &self,
        thing_id: u128,
        archetype_id: u64,
    ) -> Result<(), Box<dyn Error>> {
        let key = Self::entity_to_archetype_key(thing_id);
        self.keyspace.insert(&key, &archetype_id.to_le_bytes())?;
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
        self.keyspace.insert(&key, &attr_hash.to_le_bytes())?;
        Ok(())
    }

    #[allow(clippy::await_solo)]
    pub async fn remove_entity_attribute(
        &self,
        thing_id: u128,
        attr_hash: u64,
    ) -> Result<(), Box<dyn Error>> {
        let key = Self::entity_attr_key(thing_id, attr_hash);
        self.keyspace.remove(&key)?;
        Ok(())
    }

    #[allow(clippy::await_solo)]
    pub async fn delete_entity_archetype(&self, thing_id: u128) -> Result<(), Box<dyn Error>> {
        let key = Self::entity_to_archetype_key(thing_id);
        self.keyspace.remove(&key)?;
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
        self.keyspace.insert(&key, b"")?;
        Ok(())
    }

    pub fn remove_entity_reverse_index(
        &self,
        thing_id: u128,
        attr_hash: u64,
    ) -> Result<(), Box<dyn Error>> {
        let key = Self::attr_index_key(attr_hash, thing_id);
        self.keyspace.remove(&key)?;
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

    pub(crate) fn edge_key(edge_hash: u64, src: u128, tgt: u128) -> Vec<u8> {
        let mut key = b"ef:".to_vec();
        key.extend_from_slice(&edge_hash.to_le_bytes());
        key.extend_from_slice(&src.to_le_bytes());
        key.extend_from_slice(&tgt.to_le_bytes());
        key
    }

    pub(crate) fn reverse_edge_key(edge_hash: u64, tgt: u128, src: u128) -> Vec<u8> {
        let mut key = b"er:".to_vec();
        key.extend_from_slice(&edge_hash.to_le_bytes());
        key.extend_from_slice(&tgt.to_le_bytes());
        key.extend_from_slice(&src.to_le_bytes());
        key
    }

    pub(crate) fn outgoing_edge_prefix(edge_hash: u64, src: u128) -> Vec<u8> {
        let mut prefix = b"ef:".to_vec();
        prefix.extend_from_slice(&edge_hash.to_le_bytes());
        prefix.extend_from_slice(&src.to_le_bytes());
        prefix
    }

    pub(crate) fn incoming_edge_prefix(edge_hash: u64, tgt: u128) -> Vec<u8> {
        let mut prefix = b"er:".to_vec();
        prefix.extend_from_slice(&edge_hash.to_le_bytes());
        prefix.extend_from_slice(&tgt.to_le_bytes());
        prefix
    }

    pub(crate) fn parse_edge_target(key: &[u8]) -> Option<u128> {
        if key.len() < 43 || !key.starts_with(b"ef:") {
            return None;
        }
        let bytes: [u8; 16] = key[27..43].try_into().ok()?;
        Some(u128::from_le_bytes(bytes))
    }

    pub(crate) fn parse_reverse_edge_source(key: &[u8]) -> Option<u128> {
        if key.len() < 43 || !key.starts_with(b"er:") {
            return None;
        }
        let bytes: [u8; 16] = key[27..43].try_into().ok()?;
        Some(u128::from_le_bytes(bytes))
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_storage() -> Storage {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = format!("/tmp/test_thingdb_storage_{}", counter);
        let _ = std::fs::remove_dir_all(&path);
        Storage::open(&path).unwrap()
    }

    #[test]
    fn test_key_encoding() {
        let key = KeyEncoder::encode(1, 2, 3);
        assert_eq!(key.len(), 32);

        let (a, h, t) = KeyEncoder::decode(&key).unwrap();
        assert_eq!(a, 1);
        assert_eq!(h, 2);
        assert_eq!(t, 3);
    }

    #[test]
    fn test_key_encoding_max_values() {
        let key = KeyEncoder::encode(u64::MAX, u64::MAX, u128::MAX);
        assert_eq!(key.len(), 32);

        let (a, h, t) = KeyEncoder::decode(&key).unwrap();
        assert_eq!(a, u64::MAX);
        assert_eq!(h, u64::MAX);
        assert_eq!(t, u128::MAX);
    }

    #[test]
    fn test_key_encoding_zero_values() {
        let key = KeyEncoder::encode(0, 0, 0);
        assert_eq!(key.len(), 32);

        let (a, h, t) = KeyEncoder::decode(&key).unwrap();
        assert_eq!(a, 0);
        assert_eq!(h, 0);
        assert_eq!(t, 0);
    }

    #[test]
    fn test_key_decode_wrong_length_returns_none() {
        assert!(KeyEncoder::decode(&[]).is_none());
        assert!(KeyEncoder::decode(&[1, 2, 3]).is_none());
        assert!(KeyEncoder::decode(&[0; 31]).is_none());
        assert!(KeyEncoder::decode(&[0; 33]).is_none());
    }

    #[test]
    fn test_encode_archetype_prefix() {
        let prefix = KeyEncoder::encode_archetype_prefix(42);
        assert_eq!(prefix.len(), 8);
        assert_eq!(u64::from_le_bytes(prefix[..8].try_into().unwrap()), 42);
    }

    #[test]
    fn test_encode_attr_prefix() {
        let prefix = KeyEncoder::encode_attr_prefix(7, 13);
        assert_eq!(prefix.len(), 16);
        assert_eq!(u64::from_le_bytes(prefix[0..8].try_into().unwrap()), 7);
        assert_eq!(u64::from_le_bytes(prefix[8..16].try_into().unwrap()), 13);
    }

    #[tokio::test]
    async fn test_entity_attr_roundtrip() {
        let storage = test_storage();
        let thing_id: u128 = 42;

        let attrs = storage.get_entity_attrs(thing_id);
        assert!(attrs.is_empty());

        storage.add_entity_attribute(thing_id, 10).await.unwrap();
        storage.add_entity_attribute(thing_id, 20).await.unwrap();
        storage.add_entity_attribute(thing_id, 30).await.unwrap();

        let attrs = storage.get_entity_attrs(thing_id);
        assert_eq!(attrs.len(), 3);
        assert!(attrs.contains(&10));
        assert!(attrs.contains(&20));
        assert!(attrs.contains(&30));

        storage.remove_entity_attribute(thing_id, 20).await.unwrap();
        let attrs = storage.get_entity_attrs(thing_id);
        assert_eq!(attrs.len(), 2);
        assert!(!attrs.contains(&20));
    }

    #[tokio::test]
    async fn test_entity_attrs_empty_after_all_removed() {
        let storage = test_storage();
        storage.add_entity_attribute(1, 99).await.unwrap();
        storage.remove_entity_attribute(1, 99).await.unwrap();
        let attrs = storage.get_entity_attrs(1);
        assert!(attrs.is_empty());
    }

    #[tokio::test]
    async fn test_entity_attrs_multiple_entities_isolated() {
        let storage = test_storage();
        storage.add_entity_attribute(1, 10).await.unwrap();
        storage.add_entity_attribute(2, 20).await.unwrap();

        let attrs_1 = storage.get_entity_attrs(1);
        let attrs_2 = storage.get_entity_attrs(2);
        assert_eq!(attrs_1, vec![10]);
        assert_eq!(attrs_2, vec![20]);
    }

    #[tokio::test]
    async fn test_entity_archetype_roundtrip() {
        let storage = test_storage();
        assert!(storage.get_entity_archetype(1).is_none());

        storage.set_entity_archetype(1, 100).await.unwrap();
        assert_eq!(storage.get_entity_archetype(1), Some(100));

        storage.delete_entity_archetype(1).await.unwrap();
        assert!(storage.get_entity_archetype(1).is_none());
    }

    #[tokio::test]
    async fn test_entity_archetype_update() {
        let storage = test_storage();
        storage.set_entity_archetype(1, 100).await.unwrap();
        storage.set_entity_archetype(1, 200).await.unwrap();
        assert_eq!(storage.get_entity_archetype(1), Some(200));
    }

    #[tokio::test]
    async fn test_reverse_index_roundtrip() {
        let storage = test_storage();
        let entities = storage.get_entities_with_attr(42);
        assert!(entities.is_empty());

        storage.add_entity_reverse_index(1, 42).unwrap();
        storage.add_entity_reverse_index(2, 42).unwrap();

        let entities = storage.get_entities_with_attr(42);
        assert_eq!(entities.len(), 2);
        assert!(entities.contains(&1));
        assert!(entities.contains(&2));
    }

    #[tokio::test]
    async fn test_reverse_index_remove() {
        let storage = test_storage();
        storage.add_entity_reverse_index(1, 42).unwrap();
        storage.add_entity_reverse_index(2, 42).unwrap();
        storage.remove_entity_reverse_index(1, 42).unwrap();

        let entities = storage.get_entities_with_attr(42);
        assert_eq!(entities.len(), 1);
        assert!(entities.contains(&2));
    }

    #[tokio::test]
    async fn test_reverse_index_multiple_attrs_isolated() {
        let storage = test_storage();
        storage.add_entity_reverse_index(1, 10).unwrap();
        storage.add_entity_reverse_index(1, 20).unwrap();

        assert_eq!(storage.get_entities_with_attr(10), vec![1]);
        assert_eq!(storage.get_entities_with_attr(20), vec![1]);
        assert!(storage.get_entities_with_attr(30).is_empty());
    }

    #[tokio::test]
    async fn test_reverse_index_remove_all() {
        let storage = test_storage();
        storage.add_entity_reverse_index(1, 42).unwrap();
        storage.add_entity_reverse_index(2, 42).unwrap();
        storage.remove_entity_reverse_index(1, 42).unwrap();
        storage.remove_entity_reverse_index(2, 42).unwrap();

        let entities = storage.get_entities_with_attr(42);
        assert!(entities.is_empty());
    }

    #[test]
    fn test_key_format_entity_to_archetype_key() {
        let key = Storage::entity_to_archetype_key(42);
        assert!(key.starts_with(b"eta:"));
        assert_eq!(key.len(), 4 + 16);
    }

    #[test]
    fn test_key_format_entity_attr_key() {
        let key = Storage::entity_attr_key(42, 7);
        assert!(key.starts_with(b"ea:"));
        assert_eq!(key.len(), 3 + 16 + 8);
    }

    #[test]
    fn test_key_format_attr_index_key() {
        let key = Storage::attr_index_key(7, 42);
        assert!(key.starts_with(b"ai:"));
        assert_eq!(key.len(), 3 + 8 + 16);
    }

    #[tokio::test]
    async fn test_storage_put_get_delete() {
        let storage = test_storage();
        storage.put(b"key1", b"value1").await.unwrap();
        assert_eq!(storage.get(b"key1"), Some(b"value1".to_vec()));

        storage.delete(b"key1").await.unwrap();
        assert_eq!(storage.get(b"key1"), None);
    }

    #[tokio::test]
    async fn test_storage_get_nonexistent() {
        let storage = test_storage();
        assert_eq!(storage.get(b"nonexistent"), None);
    }

    #[tokio::test]
    async fn test_storage_overwrite_value() {
        let storage = test_storage();
        storage.put(b"key", b"old").await.unwrap();
        storage.put(b"key", b"new").await.unwrap();
        assert_eq!(storage.get(b"key"), Some(b"new".to_vec()));
    }
}
