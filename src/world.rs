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
