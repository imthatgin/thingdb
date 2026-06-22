use crate::blueprint::AttributeSet;

pub trait Entity: Sized {
    type Attributes: AttributeSet;

    fn from_attributes(attrs: Self::Attributes) -> Self;
    fn into_attributes(self) -> Self::Attributes;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute::Attribute;
    use crate::blueprint::Blueprint;
    use crate::World;
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_path() -> String {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = format!("/tmp/test_thingdb_entity_{}", counter);
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[derive(Serialize, Deserialize)]
    struct Username(String);

    impl Attribute for Username {
        const NAME: &'static str = "Username";
    }

    #[derive(Serialize, Deserialize)]
    struct DisplayName(String);

    impl Attribute for DisplayName {
        const NAME: &'static str = "DisplayName";
    }

    #[derive(Serialize, Deserialize)]
    struct CreatedAt(u64);

    impl Attribute for CreatedAt {
        const NAME: &'static str = "CreatedAt";
    }

    #[derive(Serialize, Deserialize)]
    struct Active(bool);

    impl Attribute for Active {
        const NAME: &'static str = "Active";
    }

    struct User {
        username: String,
        display_name: String,
        created_at: u64,
        active: bool,
    }

    impl Entity for User {
        type Attributes = (Username, DisplayName, CreatedAt, Active);

        fn from_attributes(attrs: Self::Attributes) -> Self {
            let (username, display_name, created_at, active) = attrs;
            User {
                username: username.0,
                display_name: display_name.0,
                created_at: created_at.0,
                active: active.0,
            }
        }

        fn into_attributes(self) -> Self::Attributes {
            (
                Username(self.username),
                DisplayName(self.display_name),
                CreatedAt(self.created_at),
                Active(self.active),
            )
        }
    }

    #[tokio::test]
    async fn test_entity_round_trip() {
        let path = test_path();
        let world = World::open_file(&path).unwrap();

        let user = User {
            username: "alice".into(),
            display_name: "Alice".into(),
            created_at: 1700000000,
            active: true,
        };

        let mut tx = world.tx().await;
        let id = tx.spawn_entity(user).await.unwrap();
        tx.commit().await.unwrap();

        let fetched: User = world.get_entity(id).unwrap();
        assert_eq!(fetched.username, "alice");
        assert_eq!(fetched.display_name, "Alice");
        assert_eq!(fetched.created_at, 1700000000);
        assert!(fetched.active);
    }

    #[tokio::test]
    async fn test_entity_fetch_returns_none_when_missing_attribute() {
        let path = test_path();
        let world = World::open_file(&path).unwrap();

        let mut tx = world.tx().await;
        let id = tx.spawn().await;
        tx.add(id, Username("bob".into())).await.unwrap();
        tx.commit().await.unwrap();

        let result: Option<User> = world.get_entity(id);
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_entity_into_attributes_produces_blueprint() {
        let path = test_path();
        let world = World::open_file(&path).unwrap();

        let user = User {
            username: "charlie".into(),
            display_name: "Charlie".into(),
            created_at: 1800000000,
            active: false,
        };

        let attrs = user.into_attributes();
        let mut tx = world.tx().await;
        let id = tx.spawn_with(attrs).await.unwrap();
        tx.commit().await.unwrap();

        let fetched: User = world.get_entity(id).unwrap();
        assert_eq!(fetched.username, "charlie");
        assert!(!fetched.active);
    }

    #[tokio::test]
    async fn test_entity_apply_to_existing_entity() {
        let path = test_path();
        let world = World::open_file(&path).unwrap();

        let mut tx = world.tx().await;
        let id = tx.spawn().await;
        let user = User {
            username: "dave".into(),
            display_name: "Dave".into(),
            created_at: 1900000000,
            active: true,
        };
        Blueprint::apply(user.into_attributes(), &mut tx, id).unwrap();
        tx.commit().await.unwrap();

        let fetched: User = world.get_entity(id).unwrap();
        assert_eq!(fetched.username, "dave");
        assert_eq!(fetched.created_at, 1900000000);
    }
}
