use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use thingdb::world::World;

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize, Debug, thingdb::Attribute)]
struct Metadata {
    value: u64,
}

#[derive(Serialize, Deserialize, Debug, thingdb::Edge)]
struct Connected;

#[derive(Serialize, Deserialize, Debug, thingdb::Edge)]
struct Linked;

fn print_separator(label: &str, elapsed: std::time::Duration, count: usize) {
    let per_sec = if elapsed.as_secs_f64() > 0.0 {
        count as f64 / elapsed.as_secs_f64()
    } else {
        count as f64
    };
    let padded = if label.len() > 50 {
        format!("{}:", &label[..47])
    } else {
        format!("{}:", label)
    };
    println!(
        "  {:50} {:>8} ops in {:>6.2?}  ({:>9.0} ops/s)",
        padded, count, elapsed, per_sec
    );
}

#[tokio::main]
async fn main() {
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = format!("/tmp/thingdb_example_bench_{}", c);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let world = World::open_file(&format!("{}/db", dir)).unwrap();

    const N: usize = 10_000;

    println!("\n── Entity Insert ──");
    let start = Instant::now();
    let mut entity_ids = Vec::with_capacity(N);
    {
        let mut tx = world.tx().await;
        for i in 0..N {
            let id = tx.spawn().await;
            tx.add(id, Metadata { value: i as u64 }).await.unwrap();
            entity_ids.push(id);
        }
        tx.commit().await.unwrap();
    }
    print_separator("Entity insert (spawn + add Metadata)", start.elapsed(), N);

    println!("\n── Edge Insert ──");
    let start = Instant::now();
    {
        let mut tx = world.tx().await;
        for i in 0..N - 1 {
            tx.relate(entity_ids[i], entity_ids[i + 1], Connected)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }
    print_separator("Edge insert (chain)", start.elapsed(), N - 1);

    println!("\n── Outgoing Query ──");
    let start = Instant::now();
    let mut count = 0;
    for _ in 0..100 {
        count += world.outgoing_edges::<Connected>(entity_ids[0]).len();
    }
    print_separator("Outgoing edge query (x100)", start.elapsed(), count);

    println!("\n── Incoming Query ──");
    let start = Instant::now();
    let mut count = 0;
    for _ in 0..100 {
        count += world.incoming_edges::<Connected>(entity_ids[N / 2]).len();
    }
    print_separator("Incoming edge query (x100)", start.elapsed(), count);

    println!("\n── Multi-hop Traversal ──");
    let start = Instant::now();
    let mut count = 0;
    for _ in 0..100 {
        let mut t = world.traverse(entity_ids[0]);
        for _ in 0..10 {
            t = t.outgoing::<Connected>();
        }
        count += t.targets().len();
    }
    print_separator("10-hop traversal (x100)", start.elapsed(), count);

    println!("\n── Edge + Component Fetch ──");
    let start = Instant::now();
    let mut total = 0usize;
    for _ in 0..100 {
        for src in 0..100 {
            let edges = world.outgoing_edges::<Connected>(entity_ids[src]);
            for (tgt_id, _) in &edges {
                if let Some(_meta) = world.get_component::<Metadata>(*tgt_id) {
                    total += 1;
                }
            }
        }
    }
    print_separator("Edge+component batch (x100)", start.elapsed(), total);

    println!("\n── Full Graph Query ──");
    let start = Instant::now();
    for _ in 0..10 {
        for &id in &entity_ids {
            let _incoming = world.incoming_edges::<Connected>(id);
        }
    }
    print_separator("Full scan + incoming edges (x10)", start.elapsed(), N * 10);

    println!("\n── Unrelate ──");
    let start = Instant::now();
    for i in 0..100 {
        let mut tx = world.tx().await;
        tx.relate(entity_ids[i], entity_ids[(i + 1) % N], Connected)
            .await
            .unwrap();
        tx.unrelate_all_from::<Connected>(entity_ids[i])
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    print_separator("Relate+unrelate_all (x100)", start.elapsed(), 100);

    println!();
}
