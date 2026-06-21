use crate::thing::Thing;
use crate::tx::Tx;

pub trait Blueprint {
    fn apply(self, tx: &mut Tx, entity: Thing) -> Result<(), Box<dyn std::error::Error>>;
}

impl Blueprint for () {
    fn apply(self, _tx: &mut Tx, _entity: Thing) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

impl<T: crate::attribute::Attribute> Blueprint for T {
    fn apply(self, tx: &mut Tx, entity: Thing) -> Result<(), Box<dyn std::error::Error>> {
        let hash = crate::hash_name(<T as crate::attribute::Attribute>::NAME);
        let bytes = postcard::to_allocvec(&self)?;
        tx.write_attr(entity, hash, bytes)
    }
}

macro_rules! impl_blueprint_for_tuples {
    (@impl $($T:ident),+) => {
        impl<$($T: Blueprint),+> Blueprint for ($($T,)+) {
            #[allow(non_snake_case)]
            fn apply(self, tx: &mut Tx, entity: Thing) -> Result<(), Box<dyn std::error::Error>> {
                let ($($T,)+) = self;
                $(Blueprint::apply($T, tx, entity)?;)+
                Ok(())
            }
        }
    };
}

impl_blueprint_for_tuples!(@impl A);
impl_blueprint_for_tuples!(@impl A, B);
impl_blueprint_for_tuples!(@impl A, B, C);
impl_blueprint_for_tuples!(@impl A, B, C, D);
impl_blueprint_for_tuples!(@impl A, B, C, D, E);
impl_blueprint_for_tuples!(@impl A, B, C, D, E, F);
impl_blueprint_for_tuples!(@impl A, B, C, D, E, F, G);
impl_blueprint_for_tuples!(@impl A, B, C, D, E, F, G, H);
impl_blueprint_for_tuples!(@impl A, B, C, D, E, F, G, H, I);
impl_blueprint_for_tuples!(@impl A, B, C, D, E, F, G, H, I, J);
impl_blueprint_for_tuples!(@impl A, B, C, D, E, F, G, H, I, J, K);
impl_blueprint_for_tuples!(@impl A, B, C, D, E, F, G, H, I, J, K, L);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archetype::Registry;
    use crate::attribute::Attribute;
    use crate::storage::Storage;
    use crate::tx::Tx;
    use serde::{Deserialize, Serialize};
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_storage() -> Arc<Storage> {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = format!("/tmp/test_thingdb_blueprint_{}", counter);
        let _ = std::fs::remove_dir_all(&path);
        Arc::new(Storage::open(&path).unwrap())
    }

    #[derive(Serialize, Deserialize)]
    struct Player {
        name: String,
        level: u32,
    }

    impl Attribute for Player {
        const NAME: &'static str = "Player";
    }

    #[derive(Serialize, Deserialize)]
    struct Health(u32);

    impl Attribute for Health {
        const NAME: &'static str = "Health";
    }

    #[derive(Serialize, Deserialize)]
    struct Position {
        x: f64,
        y: f64,
    }

    impl Attribute for Position {
        const NAME: &'static str = "Position";
    }

    #[derive(Serialize, Deserialize)]
    struct Vip;

    impl Attribute for Vip {
        const NAME: &'static str = "Vip";
    }

    fn player_blueprint(name: &str, x: f64, y: f64) -> impl Blueprint {
        (
            Player {
                name: name.to_owned(),
                level: 1,
            },
            Health(100),
            Position { x, y },
        )
    }

    #[tokio::test]
    async fn test_blueprint_spawn_with_single_attribute() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let entity = tx.spawn_with(Vip).await.unwrap();
        tx.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(entity);
        assert_eq!(attrs.len(), 1);
        assert!(attrs.contains(&crate::hash_name("Vip")));
    }

    #[tokio::test]
    async fn test_blueprint_spawn_with_tuple() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let entity = tx.spawn_with(player_blueprint("Alice", 10.0, 20.0)).await.unwrap();
        tx.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(entity);
        assert_eq!(attrs.len(), 3);
        assert!(attrs.contains(&crate::hash_name("Player")));
        assert!(attrs.contains(&crate::hash_name("Health")));
        assert!(attrs.contains(&crate::hash_name("Position")));
    }

    #[tokio::test]
    async fn test_blueprint_apply_to_existing_entity() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let entity = tx.spawn().await;
        let blueprint = (Health(50), Vip);
        Blueprint::apply(blueprint, &mut tx, entity).unwrap();
        tx.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(entity);
        assert_eq!(attrs.len(), 2);
        assert!(attrs.contains(&crate::hash_name("Health")));
        assert!(attrs.contains(&crate::hash_name("Vip")));
    }

    #[tokio::test]
    async fn test_blueprint_with_single_element_tuple() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let entity = tx.spawn_with((Health(99),)).await.unwrap();
        tx.commit().await.unwrap();

        let attrs: HashSet<u64> = storage.get_entity_attrs(entity).into_iter().collect();
        let arch_id = Registry::compute_archetype_id(&attrs);
        let key = crate::storage::KeyEncoder::encode(arch_id, crate::hash_name("Health"), entity);
        let data = storage.get(&key).unwrap();
        let health: Health = postcard::from_bytes(&data).unwrap();
        assert_eq!(health.0, 99);
    }

    #[tokio::test]
    async fn test_blueprint_empty_tuple_is_noop() {
        let storage = test_storage();
        let mut tx = Tx::new(storage.clone(), None);
        let entity = tx.spawn_with(()).await.unwrap();
        tx.commit().await.unwrap();

        let attrs = storage.get_entity_attrs(entity);
        assert!(attrs.is_empty());
    }
}
