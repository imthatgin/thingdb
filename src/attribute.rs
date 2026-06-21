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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_name_deterministic() {
        let a = hash_name("Player");
        let b = hash_name("Player");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_name_unique_per_name() {
        let names = ["Player", "Enemy", "Position", "Health", "VipStatus"];
        let mut hashes: Vec<u64> = names.iter().map(|n| hash_name(n)).collect();
        hashes.sort();
        hashes.dedup();
        assert_eq!(hashes.len(), names.len());
    }

    #[test]
    fn hash_name_empty_string() {
        let h = hash_name("");
        let h2 = hash_name("");
        assert_eq!(h, h2);
    }

    #[test]
    fn hash_name_case_sensitive() {
        let a = hash_name("Player");
        let b = hash_name("player");
        assert_ne!(a, b);
    }
}
