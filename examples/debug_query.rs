use serde::{Deserialize, Serialize};
use thingdb::{hash_name, World};

#[derive(Serialize, Deserialize)]
struct Player;

impl thingdb::Attribute for Player {
    const NAME: &'static str = "Player";
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = std::fs::remove_dir_all("/tmp/debug_query_db");

    // First check hash values
    let player_hash = hash_name("Player");
    println!("Player hash via fn: {}", player_hash);

    let world = World::open_file("/tmp/debug_query_db")?;
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(async {
        let tx = world.tx().await;

        let id = tx.spawn().await;
        println!("Spawned entity: {}", id);

        tx.add(id, Player).await.unwrap();
        tx.commit().await.unwrap();

        // Check what hash was computed for Player
        let player_attr_hash = hash_name(<Player as thingdb::Attribute>::NAME);
        println!("Computed Player attr hash via trait: {}", player_attr_hash);

        // Query
        let results: Vec<Player> = world.query::<Player>().run().await;
        println!("Query found {} Players", results.len());
    });

    Ok(())
}
