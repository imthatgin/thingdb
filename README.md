# thingdb

> Small flexible database inspired by the ECS data architecture.

## Concept

There are 3 overall concepts in ThingDB:

- **Entities**: A simple ID (`Thing`, a `u128`) that can have one or more attributes and one or more edges.
- **Attributes**: A piece of data in the shape of a struct, primitive, enum, etc. Each attribute type has a unique name and is stored independently on the entity.
- **Edges**: A relationship between entities. Usually a simple marker struct, but can also contain data.

On top of these, ThingDB provides two higher-level patterns for working with entities:

- **Blueprints**: Describe a collection of attributes to spawn an entity in a single call.
- **Entities**: Map between your domain model (a single struct) and the set of attributes stored in the database.

## Quick Start

```toml
[dependencies]
thingdb = { ... }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
```

## Defining Attributes

Any struct that implements `Serialize`, `Deserialize`, and `Attribute` can be stored. Use the derive macro:

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Username(String);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct DisplayName(String);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct CreatedAt(u64);

#[derive(Serialize, Deserialize, thingdb::Attribute)]
struct Active(bool);
```

## Spawning and Querying

```rust
let world = World::open_file("/path/to/db")?;

let mut tx = world.tx().await;
let id = tx.spawn().await;
tx.add(id, Username("alice".into())).await?;
tx.add(id, Active(true)).await?;
tx.commit().await?;

// Fetch a single attribute
if let Some(active) = world.get_component::<Active>(id) {
    println!("active: {}", active.0);
}

// Query across all entities
let usernames: Vec<Username> = world.query::<Username>().run().await;
```

## Blueprints

A `Blueprint` describes a collection of attributes to apply to an entity. Tuples of attributes implement `Blueprint` automatically (up to 12 elements):

```rust
use thingdb::Blueprint;

fn new_user(name: &str) -> impl Blueprint {
    (
        Username(name.to_owned()),
        DisplayName(name.to_owned()),
        CreatedAt(1700000000),
        Active(true),
    )
}

let mut tx = world.tx().await;
let id = tx.spawn_with(new_user("alice")).await?;
tx.commit().await?;
```

A single `Attribute` is also a `Blueprint`, so `tx.spawn_with(Tag)` works for spawning an entity with one attribute.

See `examples/blueprint.rs` for a runnable example.

## Domain Entities

The `Entity` trait bridges your domain model and the database's attribute-based storage. Implement it on your domain struct, specifying the attribute set and how to convert between the two:

```rust
use thingdb::Entity;

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
```

Then you can persist and fetch domain models directly:

```rust
// Persist
let mut tx = world.tx().await;
let id = tx.spawn_entity(User {
    username: "alice".into(),
    display_name: "Alice".into(),
    created_at: 1700000000,
    active: true,
}).await?;
tx.commit().await?;

// Fetch
let user: User = world.get_entity(id).unwrap();
println!("{}", user.display_name); // "Alice"
```

`get_entity` returns `None` if the entity is missing any of the required attributes.

See `examples/entity.rs` for a runnable example.

## Edges

Edges represent relationships between entities. Define them with the `Edge` derive:

```rust
#[derive(Serialize, Deserialize, thingdb::Edge)]
struct Authored;

#[derive(Serialize, Deserialize, thingdb::Edge)]
struct Contains;
```

Create and traverse edges:

```rust
let mut tx = world.tx().await;
tx.relate(alice_id, note_id, Authored).await?;
tx.commit().await?;

// Who authored note_id?
for (author_id, _) in world.incoming_edges::<Authored>(note_id) {
    // ...
}

// Multi-hop traversal: folders -> notes -> tags
let tag_ids: Vec<u128> = world
    .traverse(folder_id)
    .outgoing::<Contains>()
    .outgoing::<Tagged>()
    .targets();
```

See `examples/graph_notes.rs` for a full graph example.

## Updating and Removing

```rust
let mut tx = world.tx().await;

// Overwrite an existing attribute (or add if missing)
tx.set(id, Health(50)).await?;

// Remove a single attribute
tx.remove::<Health>(id).await?;

// Remove all attributes from an entity
tx.destroy(id).await?;

tx.commit().await?;
```

## Examples

- `examples/blueprint.rs` — Spawning entities with blueprints
- `examples/entity.rs` — Domain model mapping with the Entity trait
- `examples/graph_notes.rs` — Graph traversal with edges
- `examples/benchmark.rs` — Performance benchmark
