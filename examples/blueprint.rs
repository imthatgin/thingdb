use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use thingdb::{Blueprint, World};

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Player {
    name: String,
}

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Health(u32);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Position {
    x: f64,
    y: f64,
}

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Tag;

fn player_blueprint(name: &str, x: f64, y: f64) -> impl Blueprint {
    (
        Player {
            name: name.to_owned(),
        },
        Health(100),
        Position { x, y },
    )
}

#[tokio::main]
async fn main() {
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = format!("/tmp/thingdb_example_blueprint_{}", c);
    let _ = std::fs::remove_dir_all(&dir);
    let world = World::open_file(&dir).unwrap();

    let mut tx = world.tx().await;

    // Spawn from a blueprint function
    let alice = tx.spawn_with(player_blueprint("Alice", 0.0, 0.0)).await.unwrap();
    let bob = tx.spawn_with(player_blueprint("Bob", 10.0, 5.0)).await.unwrap();

    // Spawn a single attribute directly (any Attribute is a Blueprint)
    let marker = tx.spawn_with(Tag).await.unwrap();

    // Spawn from an inline tuple
    let _carrot = tx
        .spawn_with((
            Position { x: 3.0, y: 7.0 },
            Health(1),
        ))
        .await
        .unwrap();

    tx.commit().await.unwrap();

    // Read back individual attributes
    if let Some(pos) = world.get_component::<Position>(alice) {
        println!("Alice is at ({}, {})", pos.x, pos.y);
    }
    if let Some(hp) = world.get_component::<Health>(alice) {
        println!("Alice has {} HP", hp.0);
    }
    if let Some(pos) = world.get_component::<Position>(bob) {
        println!("Bob is at ({}, {})", pos.x, pos.y);
    }

    // Query all entities with a Health attribute
    let results: Vec<Health> = world.query::<Health>().run().await;
    println!("{} entities have Health", results.len());

    // The marker entity only has Tag
    let marker_health = world.get_component::<Health>(marker);
    println!("Marker has Health: {}", marker_health.is_some());
}
