use serde::{Deserialize, Serialize};
use std::time::Instant;
use thingdb::World;

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Player;

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Enemy;

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Position {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Health(u32);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Damage(u16);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct VipStatus;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let total: u64 = 5_000_000;
    println!("Inserting {} entities across 5 archetypes...", total);

    let _ = std::fs::remove_dir_all("/tmp/benchmark_db");
    let world = World::open_file("/tmp/benchmark_db")?;
    let rt = tokio::runtime::Runtime::new()?;

    println!("\n=== INSERTING {total} ENTITIES ===");
    let insert_start = Instant::now();

    let _vip_ids: Vec<u128> = rt.block_on(async {
        let mut tx = world.tx().await;
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
        for i in 0..total / 5 {
            let id = tx.spawn().await;
            tx.add(id, Enemy).await.unwrap();
            tx.add(id, Health((i % 100) as u32)).await.unwrap();
            tx.add(id, Damage((i % 50) as u16)).await.unwrap();
        }
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

    println!("\n=== QUERY: all Players ===");
    let q1 = Instant::now();
    let n1 = rt.block_on(async { world.query::<Player>().run().await.len() });
    println!(
        "Query [Player] — {} results in {:.3}s",
        n1,
        q1.elapsed().as_secs_f64()
    );

    println!("\n=== QUERY: Positions with Player ===");
    let q2 = Instant::now();
    let n2 = rt.block_on(async { world.query::<Position>().with::<Player>().run().await.len() });
    println!(
        "Query [Position + Player] — {} results in {:.3}s",
        n2,
        q2.elapsed().as_secs_f64()
    );

    println!("\n=== QUERY: Enemies with Health ===");
    let q3 = Instant::now();
    let n3 = rt.block_on(async { world.query::<Health>().with::<Enemy>().run().await.len() });
    println!(
        "Query [Health + Enemy] — {} results in {:.3}s",
        n3,
        q3.elapsed().as_secs_f64()
    );

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
