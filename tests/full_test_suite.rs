use thingdb::World;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Player;

impl thingdb::Attribute for Player {
    const NAME: &'static str = "Player";
}

#[derive(Serialize, Deserialize)]
struct Enemy;

impl thingdb::Attribute for Enemy {
    const NAME: &'static str = "Enemy";
}

#[derive(Serialize, Deserialize)]
struct Position {
    x: f64,
    y: f64,
}

impl thingdb::Attribute for Position {
    const NAME: &'static str = "Position";
}

#[derive(Serialize, Deserialize)]
struct Health(u32);

impl thingdb::Attribute for Health {
    const NAME: &'static str = "Health";
}

use std::sync::atomic::{AtomicU64, Ordering};

static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

fn get_test_world() -> World {
    let counter = DB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = format!("/tmp/test_thingdb_db_{}", counter);
    let _ = std::fs::remove_dir_all(&path);
    World::open_file(&path).unwrap()
}

#[tokio::test]
async fn test_spawn_returns_incrementing_ids() {
    let world = get_test_world();
    
    let tx = world.tx().await;
    let id1 = tx.spawn().await;
    let id2 = tx.spawn().await;
    let id3 = tx.spawn().await;
    
    assert_eq!(id2, id1 + 1);
    assert_eq!(id3, id2 + 1);
}

#[tokio::test]
async fn test_add_single_attribute() {
    let world = get_test_world();
    
    let tx = world.tx().await;
    let thing_id = tx.spawn().await;
    
    tx.add(thing_id, Player).await.unwrap();
    tx.commit().await.unwrap();
    
    // Query should find the player
    let results: Vec<Player> = world.query::<Player>().run().await;
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_add_multiple_attributes_same_entity() {
    let world = get_test_world();
    
    let tx = world.tx().await;
    let thing_id = tx.spawn().await;
    
    tx.add(thing_id, Player).await.unwrap();
    tx.add(thing_id, Position { x: 10.0, y: 20.0 }).await.unwrap();
    tx.commit().await.unwrap();
    
    // Query Players with Position
    let results: Vec<Position> = world.query::<Position>()
        .with::<Player>()
        .run()
        .await;
    
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].x, 10.0);
}

#[tokio::test]
async fn test_query_with_single_component() {
    let world = get_test_world();
    
    // Add some entities
    let tx = world.tx().await;
    
    let p1 = tx.spawn().await;
    tx.add(p1, Player).await.unwrap();
    
    let e1 = tx.spawn().await;
    tx.add(e1, Enemy).await.unwrap();
    
    let p2 = tx.spawn().await;
    tx.add(p2, Player).await.unwrap();
    
    tx.commit().await.unwrap();
    
    // Query all Players
    let results: Vec<Player> = world.query::<Player>().run().await;
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_query_with_multiple_with() {
    let world = get_test_world();
    
    let tx = world.tx().await;
    
    // Player with Position
    let p1 = tx.spawn().await;
    tx.add(p1, Player).await.unwrap();
    tx.add(p1, Position { x: 1.0, y: 2.0 }).await.unwrap();
    
    // Enemy with Position  
    let e1 = tx.spawn().await;
    tx.add(e1, Enemy).await.unwrap();
    tx.add(e1, Position { x: 3.0, y: 4.0 }).await.unwrap();
    
    // Player without Position (different archetype)
    let p2 = tx.spawn().await;
    tx.add(p2, Player).await.unwrap();
    
    tx.commit().await.unwrap();
    
    // Query Players with Position
    let results: Vec<Position> = world.query::<Position>()
        .with::<Player>()
        .run()
        .await;
    
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].x, 1.0);
}

#[tokio::test]
async fn test_query_with_without() {
    let world = get_test_world();
    
    let tx = world.tx().await;
    
    // Player with Health
    let p1 = tx.spawn().await;
    tx.add(p1, Player).await.unwrap();
    tx.add(p1, Health(100)).await.unwrap();
    
    // Enemy with Health  
    let e1 = tx.spawn().await;
    tx.add(e1, Enemy).await.unwrap();
    tx.add(e1, Health(50)).await.unwrap();
    
    tx.commit().await.unwrap();
    
    // Query Health that are NOT on Enemies
    let results: Vec<Health> = world.query::<Health>()
        .without::<Enemy>()
        .run()
        .await;
    
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_query_with_filter() {
    let world = get_test_world();
    
    let tx = world.tx().await;
    
    // Players with different health values
    let p1 = tx.spawn().await;
    tx.add(p1, Player).await.unwrap();
    tx.add(p1, Health(50)).await.unwrap();
    
    let p2 = tx.spawn().await;
    tx.add(p2, Player).await.unwrap();
    tx.add(p2, Health(150)).await.unwrap();
    
    tx.commit().await.unwrap();
    
    // Query Players with Health > 100
    let results: Vec<Health> = world.query::<Health>()
        .with::<Player>()
        .filter(|h| h.0 > 100)
        .run()
        .await;
    
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_complex_archetype_routing() {
    let world = get_test_world();
    
    let tx = world.tx().await;
    
    // Archetype 1: Player + Position (2 components)
    let p1 = tx.spawn().await;
    println!("p1 id: {}", p1);
    tx.add(p1, Player).await.unwrap();
    tx.add(p1, Position { x: 1.0, y: 2.0 }).await.unwrap();
    
    // Archetype 2: Enemy + Health (2 components)  
    let e1 = tx.spawn().await;
    println!("e1 id: {}", e1);
    tx.add(e1, Enemy).await.unwrap();
    tx.add(e1, Health(75)).await.unwrap();
    
    // Archetype 3: Player + Position + Health (3 components)
    let p2 = tx.spawn().await;
    println!("p2 id: {}", p2);
    tx.add(p2, Player).await.unwrap();
    tx.add(p2, Position { x: 3.0, y: 4.0 }).await.unwrap();
    tx.add(p2, Health(200)).await.unwrap();
    
    tx.commit().await.unwrap();
    
    // Query all Entities with Position
    let positions: Vec<Position> = world.query::<Position>().run().await;
    println!("Found {} positions", positions.len());
    assert_eq!(positions.len(), 2);
    
    // Query Players (all have Player component)
    let players: Vec<Player> = world.query::<Player>()
        .with::<Player>()  
        .run()
        .await;
    assert_eq!(players.len(), 2);  // p1 and p2
    
    // Query all Entities with Health
    let healths: Vec<Health> = world.query::<Health>().run().await;
    assert_eq!(healths.len(), 2);
    
    // Query Player + Position (should find both archetypes)
    let pos_with_player: Vec<Position> = world.query::<Position>()
        .with::<Player>()
        .run()
        .await;
    assert_eq!(pos_with_player.len(), 2);  // p1 and p2
}