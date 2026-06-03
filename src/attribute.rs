use serde::{Deserialize, Serialize};

pub trait Attribute: Serialize + for<'de> Deserialize<'de> + 'static {
    const NAME: &'static str;

    fn hash_id(&self) -> u64 {
        hash_name(Self::NAME)
    }
}

pub fn hash_name(name: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    hasher.finish()
}