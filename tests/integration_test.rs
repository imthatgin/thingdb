use serde::{Deserialize, Serialize};
use thingdb::World;

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Player;

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct SteamId(u64);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Nickname(String);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Position {
    x: i128,
    y: i128,
}

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Health {
    value: u64,
    max: u64,
}

#[tokio::test]
async fn test_full_api() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let unique_dir = temp_dir.join(std::process::id().to_string());
    std::fs::create_dir_all(&unique_dir)?;
    let db = World::open_file(unique_dir.join("test.db").to_string_lossy().as_ref())?;

    let mut tx = db.tx().await;

    let thing_id = tx.spawn().await;
    assert!(thing_id > 0);

    tx.add(thing_id, Player).await?;
    tx.add(thing_id, SteamId(239019320391)).await?;
    tx.add(thing_id, Position { x: 0, y: 0 }).await?;
    tx.add(thing_id, Nickname("MyNickname".to_string())).await?;
    tx.add(
        thing_id,
        Health {
            value: 100,
            max: 100,
        },
    )
    .await?;

    tx.commit().await?;

    let results = db.query::<SteamId>().with::<Player>().run().await;

    println!("Found {} players with SteamId", results.len());

    assert!(results.len() == 1);

    Ok(())
}

/// Verify the proc-macro derive works correctly.
#[derive(serde::Serialize, serde::Deserialize, thingdb::Attribute)]
#[allow(dead_code)]
struct DerivedPlayer {
    name: String,
    level: u32,
}

#[tokio::test]
async fn test_derive_macro_implementation() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let unique_dir = temp_dir.join(format!("derive_test_{}", std::process::id()));
    std::fs::create_dir_all(&unique_dir)?;
    let db = World::open_file(unique_dir.join("derive.db").to_string_lossy().as_ref())?;

    let mut tx = db.tx().await;
    let id = tx.spawn().await;
    tx.add(
        id,
        DerivedPlayer {
            name: "Hero".into(),
            level: 10,
        },
    )
    .await?;
    tx.commit().await?;

    let results: Vec<DerivedPlayer> = db.query::<DerivedPlayer>().run().await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "Hero");
    assert_eq!(results[0].level, 10);

    Ok(())
}
