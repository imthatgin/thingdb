use serde::{Deserialize, Serialize};
use std::time::Instant;
use thingdb::World;

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
    z: f64,
}

impl thingdb::Attribute for Position {
    const NAME: &'static str = "Position";
}

#[derive(Serialize, Deserialize)]
struct Health(u32);

impl thingdb::Attribute for Health {
    const NAME: &'static str = "Health";
}

#[derive(Serialize, Deserialize)]
struct Damage(u16);

impl thingdb::Attribute for Damage {
    const NAME: &'static str = "Damage";
}

#[derive(Serialize, Deserialize)]
struct VipStatus;

impl thingdb::Attribute for VipStatus {
    const NAME: &'static str = "VipStatus";
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let total: u64 = 5_000_000;
    println!("Inserting {} entities across 5 archetypes...", total);

    let _ = std::fs::remove_dir_all("/tmp/benchmark_db");
    let world = World::open_file("/tmp/benchmark_db")?;
    let rt = tokio::runtime::Runtime::new()?;

    // ── INSERT ──────────────────────────────────────────────────────
    println!("\n=== INSERTING {total} ENTITIES ===");
    let insert_start = Instant::now();

    let vip_ids: Vec<u128> = rt.block_on(async {
        let mut tx = world.tx().await;
        // Archetype 1: Player + Position + Health  (1M)
        for i in 0..total / 5 {
            let id = tx.spawn().await;
            tx.add(id, Player).await.unwrap();
            tx.add(
                id,
                Position {
                    x: i as f64,
                    y: i as f64 * 2.0,
                    z: 0.0,
                },
            )
            .await
            .unwrap();
            tx.add(id, Health(i as u32)).await.unwrap();
        }
        // Archetype 2: Enemy + Position + Health + Damage  (1M)
        for i in 0..total / 5 {
            let id = tx.spawn().await;
            tx.add(id, Enemy).await.unwrap();
            tx.add(
                id,
                Position {
                    x: i as f64,
                    y: i as f64 * 2.0,
                    z: 0.0,
                },
            )
            .await
            .unwrap();
            tx.add(id, Health((i % 100) as u32)).await.unwrap();
            tx.add(id, Damage((i % 50) as u16)).await.unwrap();
        }
        // Archetype 3: Player + Position  (1M)
        for i in 0..total / 5 {
            let id = tx.spawn().await;
            tx.add(id, Player).await.unwrap();
            tx.add(
                id,
                Position {
                    x: i as f64,
                    y: i as f64 * 2.0,
                    z: 0.0,
                },
            )
            .await
            .unwrap();
        }
        // Archetype 4: Enemy + Health + Damage  (1M)
        for i in 0..total / 5 {
            let id = tx.spawn().await;
            tx.add(id, Enemy).await.unwrap();
            tx.add(id, Health((i % 100) as u32)).await.unwrap();
            tx.add(id, Damage((i % 50) as u16)).await.unwrap();
        }
        // Archetype 5: Player + Position + Health + Damage  (1M)
        // The last 300 of these get VipStatus (test sparse query)
        let mut vip = Vec::new();
        for i in 0..total / 5 {
            let id = tx.spawn().await;
            tx.add(id, Player).await.unwrap();
            tx.add(
                id,
                Position {
                    x: i as f64,
                    y: i as f64 * 2.0,
                    z: 0.0,
                },
            )
            .await
            .unwrap();
            tx.add(id, Health(i as u32)).await.unwrap();
            tx.add(id, Damage((i % 50) as u16)).await.unwrap();
            if i >= total / 5 - 300 {
                tx.add(id, VipStatus).await.unwrap();
                vip.push(id);
            }
        }
        tx.commit().await.unwrap();
        vip
    });

    let insert_time = insert_start.elapsed();
    println!(
        "Insert time: {:.1}s  ({:.0} inserts/sec)",
        insert_time.as_secs_f64(),
        total as f64 / insert_time.as_secs_f64()
    );

    // ── SPARSE QUERY ────────────────────────────────────────────────
    println!("\n=== SPARSE QUERY (small needle, huge haystack) ===");
    let q_sparse = Instant::now();
    let n = rt.block_on(async { world.query::<VipStatus>().run().await.len() });
    let sparse_time = q_sparse.elapsed();
    println!(
        "Query [VipStatus] — {} results in {:.3}s  ({:.0} results/sec)",
        n,
        sparse_time.as_secs_f64(),
        n as f64 / sparse_time.as_secs_f64()
    );

    // ── ALL PLAYERS ─────────────────────────────────────────────────
    println!("\n=== QUERY: all Players ===");
    let q1 = Instant::now();
    let n1 = rt.block_on(async { world.query::<Player>().run().await.len() });
    println!(
        "Query [Player] — {} results in {:.3}s",
        n1,
        q1.elapsed().as_secs_f64()
    );

    // ── ALL POSITIONS WITH PLAYER ───────────────────────────────────
    println!("\n=== QUERY: Positions with Player ===");
    let q2 = Instant::now();
    let n2 = rt.block_on(async { world.query::<Position>().with::<Player>().run().await.len() });
    println!(
        "Query [Position + Player] — {} results in {:.3}s",
        n2,
        q2.elapsed().as_secs_f64()
    );

    // ── ENEMIES WITH HEALTH ─────────────────────────────────────────
    println!("\n=== QUERY: Enemies with Health ===");
    let q3 = Instant::now();
    let n3 = rt.block_on(async { world.query::<Health>().with::<Enemy>().run().await.len() });
    println!(
        "Query [Health + Enemy] — {} results in {:.3}s",
        n3,
        q3.elapsed().as_secs_f64()
    );

    // ── PLAYERS WITH HEALTH (> 50000) ───────────────────────────────
    println!("\n=== QUERY: Players with Health > 50000 ===");
    let q4 = Instant::now();
    let n4 = rt.block_on(async {
        world
            .query::<Health>()
            .with::<Player>()
            .filter(|h| h.0 > 50000)
            .run()
            .await
            .len()
    });
    println!(
        "Query [Health + Player, filtered] — {} results in {:.3}s",
        n4,
        q4.elapsed().as_secs_f64()
    );

    // ── PLAYERS WITHOUT HEALTH ──────────────────────────────────────
    println!("\n=== QUERY: Players without Health ===");
    let q5 = Instant::now();
    let n5 = rt.block_on(async {
        world
            .query::<Player>()
            .without::<Health>()
            .run()
            .await
            .len()
    });
    println!(
        "Query [Player without Health] — {} results in {:.3}s",
        n5,
        q5.elapsed().as_secs_f64()
    );

    Ok(())
}
