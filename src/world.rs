use crate::archetype::Registry;
use crate::query::Query;
use crate::storage::Storage;
use crate::tx::Tx;
use std::sync::{Arc, Mutex};

pub struct World {
    storage: Arc<Storage>,
    registry: Arc<Mutex<Registry>>,
}

impl World {
    pub fn open_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            storage: Arc::new(Storage::open(path)?),
            registry: Arc::new(Mutex::new(Registry::default())),
        })
    }

    pub async fn tx(&self) -> Tx {
        Tx::new(self.storage.clone(), Some(self.registry.clone()))
    }

    pub fn query<T: crate::Attribute + for<'de> serde::Deserialize<'de> + Send + 'static>(
        &self,
    ) -> Query<T> {
        Query::new(self.storage.clone()).with_registry(Some(self.registry.clone()))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Attribute, World};
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_path() -> String {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = format!("/tmp/test_thingdb_world_{}", counter);
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[derive(Serialize, Deserialize)]
    struct Marker;

    impl Attribute for Marker {
        const NAME: &'static str = "Marker";
    }

    #[test]
    fn test_open_file_creates_database() {
        let path = test_path();
        let result = World::open_file(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_open_file_reopens_existing_database() {
        let path = test_path();
        World::open_file(&path).unwrap();
        let result = World::open_file(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_open_file_errors_on_invalid_path() {
        let result = World::open_file("/sys/thingdb_test_invalid");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tx_returns_valid_transaction() {
        let path = test_path();
        let world = World::open_file(&path).unwrap();
        let tx = world.tx().await;
        let id = tx.spawn().await;
        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_query_returns_valid_query() {
        let path = test_path();
        let world = World::open_file(&path).unwrap();
        let results: Vec<Marker> = world.query::<Marker>().run().await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_world_lifecycle_add_and_query() {
        let path = test_path();
        let world = World::open_file(&path).unwrap();

        let mut tx = world.tx().await;
        let id = tx.spawn().await;
        tx.add(id, Marker).await.unwrap();
        tx.commit().await.unwrap();

        let results: Vec<Marker> = world.query::<Marker>().run().await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_world_multiple_transactions() {
        let path = test_path();
        let world = World::open_file(&path).unwrap();

        let mut tx1 = world.tx().await;
        let id1 = tx1.spawn().await;
        tx1.add(id1, Marker).await.unwrap();
        tx1.commit().await.unwrap();

        let mut tx2 = world.tx().await;
        let id2 = tx2.spawn().await;
        tx2.add(id2, Marker).await.unwrap();
        tx2.commit().await.unwrap();

        let results: Vec<Marker> = world.query::<Marker>().run().await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_world_closes_and_reopens() {
        let path = test_path();
        {
            let world = World::open_file(&path).unwrap();
            let mut tx = world.tx().await;
            let id = tx.spawn().await;
            tx.add(id, Marker).await.unwrap();
            tx.commit().await.unwrap();
        }

        {
            let world = World::open_file(&path).unwrap();
            let results: Vec<Marker> = world.query::<Marker>().run().await;
            assert_eq!(results.len(), 1);
        }
    }
}
