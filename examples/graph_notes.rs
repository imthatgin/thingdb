use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use thingdb::edge::*;
use thingdb::world::World;
use thingdb::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Note {
    title: String,
}

impl Attribute for Note {
    const NAME: &'static str = "note";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Folder {
    name: String,
}

impl Attribute for Folder {
    const NAME: &'static str = "folder";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Person {
    name: String,
}

impl Attribute for Person {
    const NAME: &'static str = "person";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Tag {
    label: String,
}

impl Attribute for Tag {
    const NAME: &'static str = "tag";
}

#[derive(Serialize, Deserialize, Debug)]
struct Contains;

impl Edge for Contains {
    const NAME: &'static str = "contains";
}

#[derive(Serialize, Deserialize, Debug)]
struct Authored;

impl Edge for Authored {
    const NAME: &'static str = "authored";
}

#[derive(Serialize, Deserialize, Debug)]
struct Tagged;

impl Edge for Tagged {
    const NAME: &'static str = "tagged";
}

#[derive(Serialize, Deserialize, Debug)]
struct Related;

impl Edge for Related {
    const NAME: &'static str = "related";
}

#[tokio::main]
async fn main() {
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = format!("/tmp/thingdb_example_graph_{}", c);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let world = World::open_file(&format!("{}/db", dir)).unwrap();

    let mut tx = world.tx().await;

    let inbox = tx.spawn().await;
    tx.add(
        inbox,
        Folder {
            name: "Inbox".into(),
        },
    )
    .await
    .unwrap();

    let projects = tx.spawn().await;
    tx.add(
        projects,
        Folder {
            name: "Projects".into(),
        },
    )
    .await
    .unwrap();

    let note_a = tx.spawn().await;
    tx.add(
        note_a,
        Note {
            title: "Buy groceries".into(),
        },
    )
    .await
    .unwrap();

    let note_b = tx.spawn().await;
    tx.add(
        note_b,
        Note {
            title: "Fix bike tire".into(),
        },
    )
    .await
    .unwrap();

    let note_c = tx.spawn().await;
    tx.add(
        note_c,
        Note {
            title: "Plan trip".into(),
        },
    )
    .await
    .unwrap();

    let alice = tx.spawn().await;
    tx.add(
        alice,
        Person {
            name: "Alice".into(),
        },
    )
    .await
    .unwrap();

    let bob = tx.spawn().await;
    tx.add(bob, Person { name: "Bob".into() }).await.unwrap();

    let urgent = tx.spawn().await;
    tx.add(
        urgent,
        Tag {
            label: "urgent".into(),
        },
    )
    .await
    .unwrap();

    let idea = tx.spawn().await;
    tx.add(
        idea,
        Tag {
            label: "idea".into(),
        },
    )
    .await
    .unwrap();

    // Edges (relationships)
    tx.relate(inbox, note_a, Contains).await.unwrap();
    tx.relate(inbox, note_b, Contains).await.unwrap();
    tx.relate(projects, note_c, Contains).await.unwrap();
    tx.relate(alice, note_a, Authored).await.unwrap();
    tx.relate(alice, note_c, Authored).await.unwrap();
    tx.relate(bob, note_b, Authored).await.unwrap();
    tx.relate(note_a, urgent, Tagged).await.unwrap();
    tx.relate(note_c, idea, Tagged).await.unwrap();
    tx.relate(note_a, note_b, Related).await.unwrap();
    tx.commit().await.unwrap();

    println!();

    // Direct outgoing edges
    println!("── Notes in Inbox ──");
    for (note_id, _) in world.outgoing_edges::<Contains>(inbox) {
        if let Some(note) = world.get_component::<Note>(note_id) {
            println!("  {}", note.title);
        }
    }
    println!();

    // Direct incoming edges
    println!("── Who authored note_a? ──");
    for (author_id, _) in world.incoming_edges::<Authored>(note_a) {
        if let Some(person) = world.get_component::<Person>(author_id) {
            println!("  authored by: {}", person.name);
        }
    }
    println!();

    // Multi-hop (traversal): notes in inbox → tags of those notes
    println!("── Tags of notes in Inbox ──");
    let tag_ids: Vec<u128> = world
        .traverse(inbox)
        .outgoing::<Contains>()
        .outgoing::<Tagged>()
        .targets();
    for tag_id in &tag_ids {
        if let Some(tag) = world.get_component::<Tag>(*tag_id) {
            println!("  #{}", tag.label);
        }
    }
    println!();

    // Multi-hop filter: only items related to a given note
    println!("── Notes related to note_a (in Inbox) ──");
    let related_in_inbox: Vec<u128> = world.traverse(note_a).outgoing::<Related>().targets();
    for related_id in &related_in_inbox {
        if let Some(note) = world.get_component::<Note>(*related_id) {
            println!("  related: {}", note.title);
        }
    }
    println!();

    // Combined: who authored which notes in inbox?
    println!("── Authors of notes in Inbox ──");
    let notes_in_inbox: Vec<u128> = world.traverse(inbox).outgoing::<Contains>().targets();
    for note_id in &notes_in_inbox {
        for (author_id, _) in world.incoming_edges::<Authored>(*note_id) {
            if let Some(note) = world.get_component::<Note>(*note_id) {
                if let Some(person) = world.get_component::<Person>(author_id) {
                    println!("  \"{}\" by {}", note.title, person.name);
                }
            }
        }
    }
    println!();

    // Reverse traversal: which folder contains something tagged "urgent"?
    println!("── Folders containing urgent items ──");
    let folders: Vec<u128> = world
        .traverse(urgent)
        .incoming::<Tagged>()
        .incoming::<Contains>()
        .targets();
    for folder_id in &folders {
        if let Some(folder) = world.get_component::<Folder>(*folder_id) {
            println!("  /{}", folder.name);
        }
    }
    println!();

    // Multi-hop with multiple edge types
    println!("── Tags reachable via notes authored by Alice ──");
    let tags: Vec<u128> = world
        .traverse(alice)
        .outgoing::<Authored>()
        .outgoing::<Tagged>()
        .targets();
    for tag_id in &tags {
        if let Some(tag) = world.get_component::<Tag>(*tag_id) {
            println!("  #{}", tag.label);
        }
    }
    println!();

    // Full graph dump via known IDs + edges
    println!("── Full folder tree ──");
    for (folder_id, folder_name) in &[(inbox, "Inbox"), (projects, "Projects")] {
        println!("  /{}", folder_name);
        for (note_id, _) in world.outgoing_edges::<Contains>(*folder_id) {
            if let Some(note) = world.get_component::<Note>(note_id) {
                let authors: Vec<String> = world
                    .incoming_edges::<Authored>(note_id)
                    .iter()
                    .filter_map(|(aid, _)| {
                        world.get_component::<Person>(*aid).map(|p| p.name.clone())
                    })
                    .collect();
                let note_tags: Vec<String> = world
                    .outgoing_edges::<Tagged>(note_id)
                    .iter()
                    .filter_map(|(tid, _)| world.get_component::<Tag>(*tid).map(|t| t.label))
                    .collect();
                println!(
                    "    {} (by {}, tags: {})",
                    note.title,
                    authors.join(", "),
                    if note_tags.is_empty() {
                        "none".into()
                    } else {
                        note_tags.join(", ")
                    }
                );
            }
        }
    }
}
