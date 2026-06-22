use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use thingdb::{Entity, World};

static COUNTER: AtomicU64 = AtomicU64::new(0);

// ── Attribute types (how the user is stored in the database) ──

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Username(String);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct DisplayName(String);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct CreatedAt(u64);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Active(bool);

// ── Domain model (what your application works with) ──

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

#[tokio::main]
async fn main() {
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = format!("/tmp/thingdb_example_entity_{}", c);
    let _ = std::fs::remove_dir_all(&dir);
    let world = World::open_file(&dir).unwrap();

    let mut tx = world.tx().await;

    // Persist a domain model directly — spawn_entity handles the conversion
    let alice_id = tx
        .spawn_entity(User {
            username: "alice".into(),
            display_name: "Alice".into(),
            created_at: 1700000000,
            active: true,
        })
        .await
        .unwrap();

    let bob_id = tx
        .spawn_entity(User {
            username: "bob".into(),
            display_name: "Bob".into(),
            created_at: 1800000000,
            active: false,
        })
        .await
        .unwrap();

    tx.commit().await.unwrap();

    // Read back as the domain model — get_entity reconstructs it
    let alice: User = world.get_entity(alice_id).unwrap();
    println!(
        "{} ({}) — created: {}, active: {}",
        alice.display_name, alice.username, alice.created_at, alice.active
    );

    let bob: User = world.get_entity(bob_id).unwrap();
    println!(
        "{} ({}) — created: {}, active: {}",
        bob.display_name, bob.username, bob.created_at, bob.active
    );

    // Query a single attribute across all users
    let usernames: Vec<Username> = world.query::<Username>().run().await;
    println!("\nAll usernames:");
    for username in &usernames {
        println!("  @{}", username.0);
    }
}
