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
    
    assert!(id2 > id1);
    assert!(id3 > id2);
}

#[tokio::test]
async fn test_add_single_attribute() {
    let world = get_test_world();
    
    let mut tx = world.tx().await;
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
    
    let mut tx = world.tx().await;
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
    let mut tx = world.tx().await;
    
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
    
    let mut tx = world.tx().await;
    
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
    
    let mut tx = world.tx().await;
    
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
    
    let mut tx = world.tx().await;
    
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
    
    let mut tx = world.tx().await;
    
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

#[tokio::test]
async fn test_set_updates_existing_component() {
    let world = get_test_world();
    let mut tx = world.tx().await;

    let id = tx.spawn().await;
    tx.add(id, Position { x: 1.0, y: 2.0 }).await.unwrap();
    tx.commit().await.unwrap();

    let mut tx2 = world.tx().await;
    tx2.set(id, Position { x: 10.0, y: 20.0 }).await.unwrap();
    tx2.commit().await.unwrap();

    let positions: Vec<Position> = world.query::<Position>().run().await;
    assert_eq!(positions.len(), 1);
    assert!((positions[0].x - 10.0).abs() < 1e-9);
    assert!((positions[0].y - 20.0).abs() < 1e-9);
}

#[tokio::test]
async fn test_set_adds_new_component() {
    let world = get_test_world();
    let mut tx = world.tx().await;

    let id = tx.spawn().await;
    tx.add(id, Player).await.unwrap();
    tx.commit().await.unwrap();

    let mut tx2 = world.tx().await;
    tx2.set(id, Position { x: 5.0, y: 6.0 }).await.unwrap();
    tx2.commit().await.unwrap();

    let positions: Vec<Position> = world.query::<Position>().with::<Player>().run().await;
    assert_eq!(positions.len(), 1);
}

#[tokio::test]
async fn test_remove_component() {
    let world = get_test_world();
    let mut tx = world.tx().await;

    let id = tx.spawn().await;
    tx.add(id, Player).await.unwrap();
    tx.add(id, Position { x: 1.0, y: 2.0 }).await.unwrap();
    tx.add(id, Health(100)).await.unwrap();
    tx.commit().await.unwrap();

    // Remove Health
    let mut tx2 = world.tx().await;
    tx2.remove::<Health>(id).await.unwrap();
    tx2.commit().await.unwrap();

    let healths: Vec<Health> = world.query::<Health>().run().await;
    assert_eq!(healths.len(), 0);

    let players: Vec<Player> = world.query::<Player>().run().await;
    assert_eq!(players.len(), 1);
}

#[tokio::test]
async fn test_remove_last_component() {
    let world = get_test_world();
    let mut tx = world.tx().await;

    let id = tx.spawn().await;
    tx.add(id, Player).await.unwrap();
    tx.commit().await.unwrap();

    let mut tx2 = world.tx().await;
    tx2.remove::<Player>(id).await.unwrap();
    tx2.commit().await.unwrap();

    let players: Vec<Player> = world.query::<Player>().run().await;
    assert_eq!(players.len(), 0);
}

#[tokio::test]
async fn test_remove_updates_archetype() {
    let world = get_test_world();
    let mut tx = world.tx().await;

    // Two entities with Player + Health
    let a = tx.spawn().await;
    tx.add(a, Player).await.unwrap();
    tx.add(a, Health(100)).await.unwrap();

    let b = tx.spawn().await;
    tx.add(b, Player).await.unwrap();
    tx.add(b, Health(200)).await.unwrap();
    tx.commit().await.unwrap();

    // Remove Health from a
    let mut tx2 = world.tx().await;
    tx2.remove::<Health>(a).await.unwrap();
    tx2.commit().await.unwrap();

    // Health query: should only find b (has Health)
    let healths: Vec<Health> = world.query::<Health>().run().await;
    assert_eq!(healths.len(), 1);
    assert_eq!(healths[0].0, 200);

    // Player query: should find both
    let players: Vec<Player> = world.query::<Player>().run().await;
    assert_eq!(players.len(), 2);
}

#[tokio::test]
async fn test_destroy_entity() {
    let world = get_test_world();
    let mut tx = world.tx().await;

    let id = tx.spawn().await;
    tx.add(id, Player).await.unwrap();
    tx.add(id, Position { x: 1.0, y: 2.0 }).await.unwrap();
    tx.add(id, Health(100)).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(world.query::<Player>().run().await.len(), 1);

    let mut tx2 = world.tx().await;
    tx2.destroy(id).await.unwrap();
    tx2.commit().await.unwrap();

    assert_eq!(world.query::<Player>().run().await.len(), 0);
    assert_eq!(world.query::<Position>().run().await.len(), 0);
    assert_eq!(world.query::<Health>().run().await.len(), 0);
}

#[tokio::test]
async fn test_destroy_twice_is_noop() {
    let world = get_test_world();
    let mut tx = world.tx().await;

    let id = tx.spawn().await;
    tx.add(id, Player).await.unwrap();
    tx.commit().await.unwrap();

    let mut tx2 = world.tx().await;
    tx2.destroy(id).await.unwrap();
    tx2.destroy(id).await.unwrap();
    tx2.commit().await.unwrap();

    assert_eq!(world.query::<Player>().run().await.len(), 0);
}

#[tokio::test]
async fn test_destroy_does_not_affect_others() {
    let world = get_test_world();
    let mut tx = world.tx().await;

    let a = tx.spawn().await;
    tx.add(a, Player).await.unwrap();
    tx.add(a, Health(100)).await.unwrap();

    let b = tx.spawn().await;
    tx.add(b, Health(200)).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(world.query::<Player>().run().await.len(), 1);
    assert_eq!(world.query::<Health>().run().await.len(), 2);

    let mut tx2 = world.tx().await;
    tx2.destroy(a).await.unwrap();
    tx2.commit().await.unwrap();

    assert_eq!(world.query::<Player>().run().await.len(), 0);
    let healths: Vec<Health> = world.query::<Health>().run().await;
    assert_eq!(healths.len(), 1);
    assert_eq!(healths[0].0, 200);
}

// ── Error & edge-case paths ─────────────────────────────────────────

#[tokio::test]
async fn test_add_duplicate_component_is_noop() {
    let world = get_test_world();
    let mut tx = world.tx().await;
    let id = tx.spawn().await;
    tx.add(id, Player).await.unwrap();
    tx.add(id, Player).await.unwrap();  // same component again
    tx.commit().await.unwrap();

    let players: Vec<Player> = world.query::<Player>().run().await;
    assert_eq!(players.len(), 1);
}

#[tokio::test]
async fn test_remove_nonexistent_component_is_noop() {
    let world = get_test_world();
    let mut tx = world.tx().await;
    let id = tx.spawn().await;
    tx.add(id, Player).await.unwrap();
    tx.commit().await.unwrap();

    let mut tx2 = world.tx().await;
    tx2.remove::<Health>(id).await.unwrap();  // entity has no Health
    tx2.commit().await.unwrap();

    let players: Vec<Player> = world.query::<Player>().run().await;
    assert_eq!(players.len(), 1);
}

#[tokio::test]
async fn test_destroy_nonexistent_entity_is_noop() {
    let world = get_test_world();
    let mut tx = world.tx().await;
    tx.destroy(999).await.unwrap();
    tx.commit().await.unwrap();
    // no panic
}

#[tokio::test]
async fn test_set_on_fresh_entity_creates_component() {
    let world = get_test_world();
    let mut tx = world.tx().await;
    let id = tx.spawn().await;

    // set() without prior add() should still work
    tx.set(id, Position { x: 10.0, y: 20.0 }).await.unwrap();
    tx.commit().await.unwrap();

    let positions: Vec<Position> = world.query::<Position>().run().await;
    assert_eq!(positions.len(), 1);
    assert!((positions[0].x - 10.0).abs() < 1e-9);
}

#[tokio::test]
async fn test_filter_rejects_all() {
    let world = get_test_world();
    let mut tx = world.tx().await;
    let id = tx.spawn().await;
    tx.add(id, Health(50)).await.unwrap();
    tx.commit().await.unwrap();

    let results: Vec<Health> = world.query::<Health>()
        .filter(|h| h.0 > 100)
        .run()
        .await;
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn test_with_matches_nothing() {
    let world = get_test_world();
    let mut tx = world.tx().await;
    let id = tx.spawn().await;
    tx.add(id, Player).await.unwrap();
    tx.commit().await.unwrap();

    let results: Vec<Player> = world.query::<Player>()
        .with::<Health>()  // no entity has both
        .run()
        .await;
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn test_without_standalone() {
    let world = get_test_world();
    let mut tx = world.tx().await;

    let a = tx.spawn().await;
    tx.add(a, Player).await.unwrap();
    tx.add(a, Health(100)).await.unwrap();

    let b = tx.spawn().await;
    tx.add(b, Player).await.unwrap();

    tx.commit().await.unwrap();

    // Player without Health
    let results: Vec<Player> = world.query::<Player>()
        .without::<Health>()
        .run()
        .await;
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_query_empty_world() {
    let world = get_test_world();
    let results: Vec<Player> = world.query::<Player>().run().await;
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn test_add_component_to_existing_entity_updates_archetype() {
    let world = get_test_world();
    let mut tx = world.tx().await;

    let id = tx.spawn().await;
    tx.add(id, Player).await.unwrap();
    tx.add(id, Position { x: 1.0, y: 2.0 }).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(world.query::<Player>().run().await.len(), 1);

    // Now add Health in a second transaction
    let mut tx2 = world.tx().await;
    tx2.add(id, Health(500)).await.unwrap();
    tx2.commit().await.unwrap();

    let healths: Vec<Health> = world.query::<Health>().with::<Player>().run().await;
    assert_eq!(healths.len(), 1);
    assert_eq!(healths[0].0, 500);

    let positions: Vec<Position> = world.query::<Position>().with::<Player>().run().await;
    assert_eq!(positions.len(), 1);
}

// ── RocksDB fallback (cold cache) ───────────────────────────────────

#[tokio::test]
async fn test_rocksdb_fallback_reopens_database() {
    let dir = format!("/tmp/test_rocksdb_fallback_{}", std::process::id());
    let _ = std::fs::remove_dir_all(&dir);

    // First session — write through cache
    {
        let world = World::open_file(&dir).unwrap();
        let mut tx = world.tx().await;
        let id = tx.spawn().await;
        tx.add(id, Player).await.unwrap();
        tx.add(id, Position { x: 42.0, y: 99.0 }).await.unwrap();
        tx.add(id, Health(100)).await.unwrap();
        tx.commit().await.unwrap();
    }

    // Second session — open same path, cache is cold, must fall back to RocksDB
    {
        let world2 = World::open_file(&dir).unwrap();
        let players: Vec<Player> = world2.query::<Player>().run().await;
        assert_eq!(players.len(), 1);

        let positions: Vec<Position> = world2.query::<Position>().with::<Player>().run().await;
        assert_eq!(positions.len(), 1);
        assert!((positions[0].x - 42.0).abs() < 1e-9);

        let healths: Vec<Health> = world2.query::<Health>().without::<Enemy>().run().await;
        assert_eq!(healths.len(), 1);
        assert_eq!(healths[0].0, 100);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_rocksdb_fallback_empty_database() {
    let dir = format!("/tmp/test_rocksdb_fallback_empty_{}", std::process::id());
    let _ = std::fs::remove_dir_all(&dir);

    {
        let _world = World::open_file(&dir).unwrap();
    }

    {
        let world2 = World::open_file(&dir).unwrap();
        let results: Vec<Player> = world2.query::<Player>().run().await;
        assert_eq!(results.len(), 0);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

// ── Large-scale archetype migration ─────────────────────────────────

#[tokio::test]
async fn test_large_scale_archetype_migration() {
    let world = get_test_world();

    // Create 100 entities spread across archetypes, then migrate them
    let mut tx = world.tx().await;
    let mut ids = Vec::new();
    for _ in 0..100 {
        ids.push(tx.spawn().await);
    }

    for &id in &ids[0..30] {
        tx.add(id, Player).await.unwrap();
        tx.add(id, Position { x: 1.0, y: 1.0 }).await.unwrap();
    }
    for &id in &ids[30..60] {
        tx.add(id, Player).await.unwrap();
        tx.add(id, Health(50)).await.unwrap();
    }
    for &id in &ids[60..100] {
        tx.add(id, Enemy).await.unwrap();
        tx.add(id, Position { x: 2.0, y: 2.0 }).await.unwrap();
        tx.add(id, Health(100)).await.unwrap();
    }
    tx.commit().await.unwrap();

    assert_eq!(world.query::<Player>().run().await.len(), 60);
    assert_eq!(world.query::<Enemy>().run().await.len(), 40);
    assert_eq!(world.query::<Position>().run().await.len(), 70);
    assert_eq!(world.query::<Health>().run().await.len(), 70);

    // Migrate: add Health to first 30 Player+Position entities
    let mut tx2 = world.tx().await;
    for &id in &ids[0..30] {
        tx2.add(id, Health(200)).await.unwrap();
    }
    // Migrate: remove Health from 30..60 Player+Health entities
    for &id in &ids[30..60] {
        tx2.remove::<Health>(id).await.unwrap();
    }
    tx2.commit().await.unwrap();

    // After migration:
    // ids[0..30]: Player + Position + Health  (3 components, was 2)
    // ids[30..60]: Player (1 component, was 2)
    // ids[60..100]: Enemy + Position + Health (unchanged, 3 components)

    assert_eq!(world.query::<Player>().run().await.len(), 60);
    assert_eq!(world.query::<Enemy>().run().await.len(), 40);

    // Health: ids[0..30] + ids[60..100] = 30 + 40 = 70
    let healths: Vec<Health> = world.query::<Health>().run().await;
    assert_eq!(healths.len(), 70);
    let health_200_count = healths.iter().filter(|h| h.0 == 200).count();
    assert_eq!(health_200_count, 30);

    // Player without Health: ids[30..60] = 30
    let players_no_health: Vec<Player> = world.query::<Player>()
        .without::<Health>()
        .run()
        .await;
    assert_eq!(players_no_health.len(), 30);

    // Player with Health: ids[0..30] = 30
    let players_with_health: Vec<Player> = world.query::<Player>()
        .with::<Health>()
        .run()
        .await;
    assert_eq!(players_with_health.len(), 30);
}

// ── Complex component types ─────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct Inventory {
    items: Vec<String>,
    gold: u64,
}

impl thingdb::Attribute for Inventory {
    const NAME: &'static str = "Inventory";
}

#[derive(Serialize, Deserialize)]
struct Buff {
    name: String,
    duration: Option<f64>,
    stacks: u32,
}

impl thingdb::Attribute for Buff {
    const NAME: &'static str = "Buff";
}

#[tokio::test]
async fn test_component_with_complex_types() {
    let world = get_test_world();
    let mut tx = world.tx().await;

    let id = tx.spawn().await;
    tx.add(id, Inventory {
        items: vec!["sword".into(), "shield".into()],
        gold: 1000,
    }).await.unwrap();

    tx.add(id, Buff {
        name: "Strength".into(),
        duration: Some(30.0),
        stacks: 3,
    }).await.unwrap();

    tx.commit().await.unwrap();

    let inv: Vec<Inventory> = world.query::<Inventory>().run().await;
    assert_eq!(inv.len(), 1);
    assert_eq!(inv[0].items.len(), 2);
    assert_eq!(inv[0].gold, 1000);

    let buffs: Vec<Buff> = world.query::<Buff>().with::<Inventory>().run().await;
    assert_eq!(buffs.len(), 1);
    assert_eq!(buffs[0].name, "Strength");
    assert!(buffs[0].duration.is_some());
    assert!((buffs[0].duration.unwrap() - 30.0).abs() < 1e-9);
    assert_eq!(buffs[0].stacks, 3);
}