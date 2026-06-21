use crate::hash_name;
use crate::storage::Storage;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

pub trait Edge: Serialize + for<'de> Deserialize<'de> + 'static {
    const NAME: &'static str;

    fn hash_id(&self) -> u64 {
        hash_name(Self::NAME)
    }
}

pub fn edge_type_hash<E: Edge>() -> u64 {
    hash_name(<E as Edge>::NAME)
}

pub fn outgoing_edges<E: Edge>(storage: &Storage, thing: u128) -> Vec<(u128, E)> {
    let hash = edge_type_hash::<E>();
    let prefix = Storage::outgoing_edge_prefix(hash, thing);
    let mut results = Vec::new();
    storage.for_each_with_prefix(&prefix, |key, value| {
        if let Some(tgt) = Storage::parse_edge_target(key) {
            if let Ok(data) = postcard::from_bytes(value) {
                results.push((tgt, data));
            }
        }
    });
    results
}

pub fn incoming_edges<E: Edge>(storage: &Storage, thing: u128) -> Vec<(u128, E)> {
    let hash = edge_type_hash::<E>();
    let prefix = Storage::incoming_edge_prefix(hash, thing);
    let mut results = Vec::new();
    storage.for_each_with_prefix(&prefix, |key, value| {
        if let Some(src) = Storage::parse_reverse_edge_source(key) {
            if let Ok(data) = postcard::from_bytes(value) {
                results.push((src, data));
            }
        }
    });
    results
}

pub struct Traversal {
    storage: Arc<Storage>,
    frontier: Vec<u128>,
}

impl Traversal {
    pub fn new(storage: Arc<Storage>, from: u128) -> Self {
        Self {
            storage,
            frontier: vec![from],
        }
    }

    pub fn outgoing<E: Edge>(mut self) -> Self {
        let hash = edge_type_hash::<E>();
        let mut next: HashSet<u128> = HashSet::new();
        for &thing in &self.frontier {
            let prefix = Storage::outgoing_edge_prefix(hash, thing);
            self.storage.for_each_with_prefix(&prefix, |key, _value| {
                if let Some(tgt) = Storage::parse_edge_target(key) {
                    next.insert(tgt);
                }
            });
        }
        self.frontier = next.into_iter().collect();
        self
    }

    pub fn incoming<E: Edge>(mut self) -> Self {
        let hash = edge_type_hash::<E>();
        let mut next: HashSet<u128> = HashSet::new();
        for &thing in &self.frontier {
            let prefix = Storage::incoming_edge_prefix(hash, thing);
            self.storage.for_each_with_prefix(&prefix, |key, _value| {
                if let Some(src) = Storage::parse_reverse_edge_source(key) {
                    next.insert(src);
                }
            });
        }
        self.frontier = next.into_iter().collect();
        self
    }

    pub fn targets(self) -> Vec<u128> {
        self.frontier
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::Tx;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_storage() -> Arc<Storage> {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = format!("/tmp/test_thingdb_edge_{}", counter);
        let _ = std::fs::remove_dir_all(&path);
        Arc::new(Storage::open(&path).unwrap())
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Owns;

    impl Edge for Owns {
        const NAME: &'static str = "owns";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct BelongsTo;

    impl Edge for BelongsTo {
        const NAME: &'static str = "belongs_to";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct MemberOf {
        role: String,
        since: u64,
    }

    impl Edge for MemberOf {
        const NAME: &'static str = "member_of";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Knows {
        strength: u8,
    }

    impl Edge for Knows {
        const NAME: &'static str = "knows";
    }

    #[tokio::test]
    async fn test_relate_and_outgoing() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);

        let alice = tx.spawn().await;
        let bob = tx.spawn().await;
        tx.relate(alice, bob, Knows { strength: 5 }).await.unwrap();
        tx.commit().await.unwrap();

        let edges: Vec<(u128, Knows)> = crate::outgoing_edges::<Knows>(&storage, alice);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].0, bob);
        assert_eq!(edges[0].1, Knows { strength: 5 });
    }

    #[tokio::test]
    async fn test_incoming() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);

        let alice = tx.spawn().await;
        let bob = tx.spawn().await;
        tx.relate(alice, bob, Knows { strength: 3 }).await.unwrap();
        tx.commit().await.unwrap();

        let incoming: Vec<(u128, Knows)> = crate::incoming_edges::<Knows>(&storage, bob);
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].0, alice);
    }

    #[tokio::test]
    async fn test_unrelate() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);

        let a = tx.spawn().await;
        let b = tx.spawn().await;
        tx.relate(a, b, Owns).await.unwrap();
        tx.commit().await.unwrap();

        assert_eq!(crate::outgoing_edges::<Owns>(&storage, a).len(), 1);

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.unrelate::<Owns>(a, b).await.unwrap();
        tx2.commit().await.unwrap();

        assert_eq!(crate::outgoing_edges::<Owns>(&storage, a).len(), 0);
        assert_eq!(crate::incoming_edges::<Owns>(&storage, b).len(), 0);
    }

    #[tokio::test]
    async fn test_unrelate_all_from() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);

        let a = tx.spawn().await;
        let b = tx.spawn().await;
        let c = tx.spawn().await;
        tx.relate(a, b, Owns).await.unwrap();
        tx.relate(a, c, Owns).await.unwrap();
        tx.commit().await.unwrap();

        assert_eq!(crate::outgoing_edges::<Owns>(&storage, a).len(), 2);

        let mut tx2 = Tx::new(storage.clone(), None);
        tx2.unrelate_all_from::<Owns>(a).await.unwrap();
        tx2.commit().await.unwrap();

        assert_eq!(crate::outgoing_edges::<Owns>(&storage, a).len(), 0);
    }

    #[tokio::test]
    async fn test_edge_with_data() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);

        let user = tx.spawn().await;
        let workspace = tx.spawn().await;
        tx.relate(user, workspace, MemberOf {
            role: "admin".into(),
            since: 2024,
        }).await.unwrap();
        tx.commit().await.unwrap();

        let edges: Vec<(u128, MemberOf)> = crate::outgoing_edges::<MemberOf>(&storage, user);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].0, workspace);
        assert_eq!(edges[0].1.role, "admin");
        assert_eq!(edges[0].1.since, 2024);
    }

    #[tokio::test]
    async fn test_traversal_multi_hop() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);

        let alice = tx.spawn().await;
        let obj = tx.spawn().await;
        let ws = tx.spawn().await;

        tx.relate(alice, obj, Owns).await.unwrap();
        tx.relate(obj, ws, BelongsTo).await.unwrap();
        tx.commit().await.unwrap();

        let result: Vec<u128> = Traversal::new(storage.clone(), alice)
            .outgoing::<Owns>()
            .outgoing::<BelongsTo>()
            .targets();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], ws);
    }

    #[tokio::test]
    async fn test_traversal_incoming() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);

        let alice = tx.spawn().await;
        let obj = tx.spawn().await;
        let ws = tx.spawn().await;

        tx.relate(alice, obj, Owns).await.unwrap();
        tx.relate(obj, ws, BelongsTo).await.unwrap();
        tx.commit().await.unwrap();

        let result: Vec<u128> = Traversal::new(storage.clone(), ws)
            .incoming::<BelongsTo>()
            .incoming::<Owns>()
            .targets();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], alice);
    }

    #[tokio::test]
    async fn test_traversal_deduplicates() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);

        let alice = tx.spawn().await;
        let obj1 = tx.spawn().await;
        let obj2 = tx.spawn().await;
        let ws = tx.spawn().await;

        tx.relate(alice, obj1, Owns).await.unwrap();
        tx.relate(alice, obj2, Owns).await.unwrap();
        tx.relate(obj1, ws, BelongsTo).await.unwrap();
        tx.relate(obj2, ws, BelongsTo).await.unwrap();
        tx.commit().await.unwrap();

        let result: Vec<u128> = Traversal::new(storage.clone(), alice)
            .outgoing::<Owns>()
            .outgoing::<BelongsTo>()
            .targets();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], ws);
    }

    #[tokio::test]
    async fn test_traversal_chain_with_data() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);

        let alice = tx.spawn().await;
        let obj = tx.spawn().await;
        let team = tx.spawn().await;

        tx.relate(alice, obj, Owns).await.unwrap();
        tx.relate(alice, team, MemberOf {
            role: "lead".into(),
            since: 2023,
        }).await.unwrap();
        tx.relate(obj, team, BelongsTo).await.unwrap();
        tx.commit().await.unwrap();

        let teams_from_objects: Vec<u128> = Traversal::new(storage.clone(), alice)
            .outgoing::<Owns>()
            .outgoing::<BelongsTo>()
            .targets();

        assert_eq!(teams_from_objects.len(), 1);
        assert_eq!(teams_from_objects[0], team);
    }

    #[tokio::test]
    async fn test_no_edges_returns_empty() {
        let storage = test_storage();
        let tx = Tx::new(storage.clone(), None);
        let orphan = tx.spawn().await;

        let outgoing = crate::outgoing_edges::<Owns>(&storage, orphan);
        assert!(outgoing.is_empty());

        let incoming = crate::incoming_edges::<Owns>(&storage, orphan);
        assert!(incoming.is_empty());

        let result: Vec<u128> = Traversal::new(storage.clone(), orphan)
            .outgoing::<Owns>()
            .targets();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_edge_types_isolated() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);

        let a = tx.spawn().await;
        let b = tx.spawn().await;

        tx.relate(a, b, Owns).await.unwrap();
        tx.relate(a, b, Knows { strength: 10 }).await.unwrap();
        tx.commit().await.unwrap();

        let owns_edges = crate::outgoing_edges::<Owns>(&storage, a);
        assert_eq!(owns_edges.len(), 1);

        let knows_edges = crate::outgoing_edges::<Knows>(&storage, a);
        assert_eq!(knows_edges.len(), 1);
        assert_eq!(knows_edges[0].1.strength, 10);
    }
}
