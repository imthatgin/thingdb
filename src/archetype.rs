use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

pub type ArchetypeId = u64;
pub type ThingId = u128;

#[derive(Default)]
pub struct Registry {
    archetypes: HashMap<ArchetypeId, Archetype>,
    entity_archetype: HashMap<ThingId, ArchetypeId>,
}

impl Registry {
    pub fn create_archetype(&mut self, component_hashes: &[u64]) -> ArchetypeId {
        let id = Self::compute_id(component_hashes);
        if !self.archetypes.contains_key(&id) {
            self.archetypes.insert(id, Archetype::new());
        }
        id
    }

    pub fn get_archetype(&self, id: ArchetypeId) -> Option<&Archetype> {
        self.archetypes.get(&id)
    }

    pub fn get_archetype_mut(&mut self, id: ArchetypeId) -> Option<&mut Archetype> {
        self.archetypes.get_mut(&id)
    }

    pub fn all_archetypes(&self) -> impl Iterator<Item = (&ArchetypeId, &Archetype)> {
        self.archetypes.iter()
    }

    pub fn archetype_count(&self) -> usize {
        self.archetypes.len()
    }

    /// Compute an archetype ID from a set of attribute hashes.
    fn compute_id(hashes: &[u64]) -> u64 {
        let mut sorted = hashes.to_vec();
        sorted.sort();
        let mut hasher = DefaultHasher::new();
        for h in &sorted {
            h.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Compute an archetype ID from a HashSet of attribute hashes.
    pub fn compute_archetype_id(attrs: &HashSet<u64>) -> ArchetypeId {
        let mut sorted: Vec<u64> = attrs.iter().copied().collect();
        sorted.sort();
        let mut hasher = DefaultHasher::new();
        for &h in &sorted {
            h.hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn entity_archetype_id(&self, thing_id: ThingId) -> Option<ArchetypeId> {
        self.entity_archetype.get(&thing_id).copied()
    }

    /// Remove an entity from its current archetype and all component data.
    pub fn remove_entity(&mut self, thing_id: ThingId) {
        if let Some(old_arch_id) = self.entity_archetype.remove(&thing_id) {
            if let Some(old_arch) = self.archetypes.get_mut(&old_arch_id) {
                old_arch.despawn(thing_id);
            }
        }
    }

    /// Set all components for an entity atomically, handling archetype migration.
    /// `components` must contain the **complete** final set of components for this entity.
    pub fn set_entity_components(&mut self, thing_id: ThingId, components: HashMap<u64, Vec<u8>>) {
        let component_hashes: HashSet<u64> = components.keys().copied().collect();
        let new_arch_id = Self::compute_archetype_id(&component_hashes);

        if let Some(&old_arch_id) = self.entity_archetype.get(&thing_id) {
            if old_arch_id != new_arch_id {
                if let Some(old_arch) = self.archetypes.get_mut(&old_arch_id) {
                    old_arch.despawn(thing_id);
                }
            }
        }

        let sorted: Vec<u64> = {
            let mut v: Vec<u64> = component_hashes.iter().copied().collect();
            v.sort();
            v
        };
        self.create_archetype(&sorted);

        let archetype = self.archetypes.get_mut(&new_arch_id).unwrap();
        for &attr_hash in &component_hashes {
            archetype.register_component(attr_hash);
        }
        archetype.spawn(thing_id);
        for (attr_hash, data) in components {
            archetype.set_component(thing_id, attr_hash, data);
        }

        self.entity_archetype.insert(thing_id, new_arch_id);
    }

    /// Read a component for an entity from the cache.
    pub fn read_component(&self, thing_id: ThingId, attr_hash: u64) -> Option<&Vec<u8>> {
        let arch_id = self.entity_archetype.get(&thing_id)?;
        let archetype = self.archetypes.get(arch_id)?;
        archetype.get_component(thing_id, attr_hash)
    }

    /// Find all archetype IDs whose component set:
    ///  - contains `output_hash` and all `with_hashes`
    ///  - contains none of `without_hashes`
    pub fn find_matching_archetypes(
        &self,
        output_hash: u64,
        with_hashes: &[u64],
        without_hashes: &[u64],
    ) -> Vec<ArchetypeId> {
        self.archetypes
            .iter()
            .filter(|(_, arch)| {
                if !arch.components.contains_key(&output_hash) {
                    return false;
                }
                for &h in with_hashes {
                    if !arch.components.contains_key(&h) {
                        return false;
                    }
                }
                for &h in without_hashes {
                    if arch.components.contains_key(&h) {
                        return false;
                    }
                }
                true
            })
            .map(|(&id, _)| id)
            .collect()
    }
}

pub struct Archetype {
    entities: Vec<ThingId>,
    components: HashMap<u64, ComponentStorage>,
    entity_to_slot: HashMap<ThingId, usize>,
}

impl Archetype {
    fn new() -> Self {
        Self {
            entities: Vec::new(),
            components: HashMap::new(),
            entity_to_slot: HashMap::new(),
        }
    }

    pub fn register_component(&mut self, attr_hash: u64) {
        if !self.components.contains_key(&attr_hash) {
            let mut storage = ComponentStorage::new();
            for _ in 0..self.entities.len() {
                storage.append_empty_slot();
            }
            self.components.insert(attr_hash, storage);
        }
    }

    pub fn spawn(&mut self, thing_id: ThingId) -> usize {
        if let Some(&slot) = self.entity_to_slot.get(&thing_id) {
            return slot;
        }
        let slot = self.entities.len();
        self.entity_to_slot.insert(thing_id, slot);
        self.entities.push(thing_id);
        for storage in self.components.values_mut() {
            storage.append_empty_slot();
        }
        slot
    }

    pub fn despawn(&mut self, thing_id: ThingId) -> Option<usize> {
        let slot = *self.entity_to_slot.get(&thing_id)?;
        let last_idx = self.entities.len() - 1;
        let last_thing = self.entities[last_idx];
        for storage in self.components.values_mut() {
            storage.swap_with_last(slot);
        }
        self.entities.swap(slot, last_idx);
        if slot < self.entities.len() - 1 {
            self.entity_to_slot.insert(last_thing, slot);
        }
        self.entity_to_slot.remove(&thing_id);
        self.entities.pop();
        Some(slot)
    }

    pub fn set_component(
        &mut self,
        thing_id: ThingId,
        attr_hash: u64,
        data: Vec<u8>,
    ) -> Option<()> {
        let slot = *self.entity_to_slot.get(&thing_id)?;
        let storage = self.components.get_mut(&attr_hash)?;
        storage.set(slot, data);
        Some(())
    }

    pub fn get_component(&self, thing_id: ThingId, attr_hash: u64) -> Option<&Vec<u8>> {
        let slot = *self.entity_to_slot.get(&thing_id)?;
        let storage = self.components.get(&attr_hash)?;
        storage.get(slot)
    }

    pub fn entities(&self) -> &[ThingId] {
        &self.entities
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn iter_component(&self, attr_hash: u64) -> impl Iterator<Item = &Vec<u8>> + '_ {
        self.components
            .get(&attr_hash)
            .into_iter()
            .flat_map(|s| s.iter())
    }
}

pub struct ComponentStorage {
    slots: Vec<Option<Vec<u8>>>,
}

impl ComponentStorage {
    fn new() -> Self {
        Self { slots: Vec::new() }
    }

    fn append_empty_slot(&mut self) {
        self.slots.push(None);
    }

    fn set(&mut self, slot: usize, data: Vec<u8>) {
        while self.slots.len() <= slot {
            self.slots.push(None);
        }
        self.slots[slot] = Some(data);
    }

    fn get(&self, slot: usize) -> Option<&Vec<u8>> {
        self.slots.get(slot).and_then(|x| x.as_ref())
    }

    fn swap_with_last(&mut self, slot: usize) {
        let len = self.slots.len();
        if slot < len - 1 {
            self.slots.swap(slot, len - 1);
        }
        self.slots.pop();
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Vec<u8>> + '_ {
        self.slots.iter().filter_map(|x| x.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Archetype basics ────────────────────────────────────────────

    #[test]
    fn test_archetype_spawn_and_entity_count() {
        let mut arch = Archetype::new();
        assert_eq!(arch.entity_count(), 0);

        arch.spawn(1);
        assert_eq!(arch.entity_count(), 1);
        assert_eq!(arch.entities(), &[1]);

        arch.spawn(2);
        assert_eq!(arch.entity_count(), 2);
        assert_eq!(arch.entities(), &[1, 2]);
    }

    #[test]
    fn test_archetype_spawn_idempotent() {
        let mut arch = Archetype::new();
        arch.register_component(10);
        arch.spawn(1);
        arch.set_component(1, 10, vec![1, 2, 3]);

        let slot = arch.spawn(1);
        assert_eq!(slot, 0);
        assert_eq!(arch.entity_count(), 1);
        assert_eq!(arch.get_component(1, 10).unwrap(), &vec![1, 2, 3]);
    }

    #[test]
    fn test_archetype_despawn_swap_removes_correctly() {
        let mut arch = Archetype::new();
        arch.register_component(10);

        arch.spawn(1);
        arch.set_component(1, 10, vec![10]);
        arch.spawn(2);
        arch.set_component(2, 10, vec![20]);
        arch.spawn(3);
        arch.set_component(3, 10, vec![30]);

        // despawn entity 1 (slot 0) — last (entity 3, slot 2) should swap into slot 0
        let slot = arch.despawn(1).unwrap();
        assert_eq!(slot, 0);
        assert_eq!(arch.entity_count(), 2);
        assert_eq!(arch.entities(), &[3, 2]);

        // entity 3 was swapped to slot 0, its data should now be at slot 0
        assert_eq!(arch.get_component(3, 10).unwrap(), &vec![30]);
        assert_eq!(arch.get_component(2, 10).unwrap(), &vec![20]);
        assert!(arch.get_component(1, 10).is_none());
    }

    #[test]
    fn test_archetype_despawn_last_entity() {
        let mut arch = Archetype::new();
        arch.register_component(10);
        arch.spawn(1);
        arch.set_component(1, 10, vec![10]);

        arch.despawn(1).unwrap();
        assert_eq!(arch.entity_count(), 0);
        assert!(arch.get_component(1, 10).is_none());
    }

    #[test]
    fn test_archetype_despawn_nonexistent_returns_none() {
        let mut arch = Archetype::new();
        arch.spawn(1);
        assert!(arch.despawn(999).is_none());
        assert_eq!(arch.entity_count(), 1);
    }

    #[test]
    fn test_archetype_register_component_after_spawn() {
        let mut arch = Archetype::new();
        arch.spawn(1);
        arch.spawn(2);

        // register after entities exist — must fill slots for existing entities
        arch.register_component(10);
        assert_eq!(arch.components.get(&10).unwrap().len(), 2);
    }

    #[test]
    fn test_archetype_set_and_get_component() {
        let mut arch = Archetype::new();
        arch.register_component(10);
        arch.spawn(1);
        arch.set_component(1, 10, vec![42]).unwrap();
        assert_eq!(arch.get_component(1, 10).unwrap(), &vec![42]);
    }

    #[test]
    fn test_archetype_set_nonexistent_entity_returns_none() {
        let mut arch = Archetype::new();
        arch.register_component(10);
        assert!(arch.set_component(999, 10, vec![42]).is_none());
    }

    #[test]
    fn test_archetype_get_nonexistent_component() {
        let mut arch = Archetype::new();
        arch.register_component(10);
        arch.spawn(1);
        assert!(arch.get_component(1, 999).is_none());
    }

    #[test]
    fn test_archetype_iter_component() {
        let mut arch = Archetype::new();
        arch.register_component(10);
        arch.spawn(1);
        arch.set_component(1, 10, vec![10]);
        arch.spawn(2);
        arch.set_component(2, 10, vec![20]);

        let items: Vec<&Vec<u8>> = arch.iter_component(10).collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], &vec![10]);
        assert_eq!(items[1], &vec![20]);
    }

    #[test]
    fn test_archetype_iter_component_empty() {
        let arch = Archetype::new();
        assert_eq!(arch.iter_component(10).count(), 0);
    }

    #[test]
    fn test_multiple_components_per_entity() {
        let mut arch = Archetype::new();
        arch.register_component(10);
        arch.register_component(20);
        arch.spawn(1);
        arch.set_component(1, 10, vec![100]);
        arch.set_component(1, 20, vec![200]);

        assert_eq!(arch.get_component(1, 10).unwrap(), &vec![100]);
        assert_eq!(arch.get_component(1, 20).unwrap(), &vec![200]);
    }

    #[test]
    fn test_compute_archetype_id_deterministic() {
        let set1: HashSet<u64> = [10, 20, 30].into_iter().collect();
        let set2: HashSet<u64> = [30, 10, 20].into_iter().collect();
        let id1 = Registry::compute_archetype_id(&set1);
        let id2 = Registry::compute_archetype_id(&set2);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_compute_archetype_id_different_sets_differ() {
        let set_a: HashSet<u64> = [10, 20].into_iter().collect();
        let set_b: HashSet<u64> = [10, 30].into_iter().collect();
        assert_ne!(
            Registry::compute_archetype_id(&set_a),
            Registry::compute_archetype_id(&set_b)
        );
    }

    #[test]
    fn test_registry_set_entity_components_creates_archetype() {
        let mut reg = Registry::default();
        let mut components = HashMap::new();
        components.insert(10, vec![1, 2, 3]);
        components.insert(20, vec![4, 5, 6]);

        reg.set_entity_components(1, components);

        let arch_id = reg.entity_archetype_id(1).unwrap();
        let arch = reg.get_archetype(arch_id).unwrap();
        assert_eq!(arch.entity_count(), 1);
        assert_eq!(arch.get_component(1, 10).unwrap(), &vec![1, 2, 3]);
        assert_eq!(arch.get_component(1, 20).unwrap(), &vec![4, 5, 6]);
    }

    #[test]
    fn test_registry_add_component_migrates_archetype() {
        let mut reg = Registry::default();

        // Start with component 10 only
        let mut c1 = HashMap::new();
        c1.insert(10, vec![10]);
        reg.set_entity_components(1, c1);

        let arch_a = reg.entity_archetype_id(1).unwrap();

        // Add component 20 — should migrate to new archetype
        let mut c2 = HashMap::new();
        c2.insert(10, vec![10]);
        c2.insert(20, vec![20]);
        reg.set_entity_components(1, c2);

        let arch_b = reg.entity_archetype_id(1).unwrap();
        assert_ne!(arch_a, arch_b); // archetype changed

        // Old archetype should have no entities
        assert_eq!(reg.get_archetype(arch_a).unwrap().entity_count(), 0);
        // New archetype should have entity
        assert_eq!(reg.get_archetype(arch_b).unwrap().entity_count(), 1);
        // Both components accessible
        assert_eq!(reg.read_component(1, 10).unwrap(), &vec![10]);
        assert_eq!(reg.read_component(1, 20).unwrap(), &vec![20]);
    }

    #[test]
    fn test_registry_remove_component_migrates_archetype() {
        let mut reg = Registry::default();

        let mut c1 = HashMap::new();
        c1.insert(10, vec![10]);
        c1.insert(20, vec![20]);
        reg.set_entity_components(1, c1);

        let arch_a = reg.entity_archetype_id(1).unwrap();

        // Remove component 20
        let mut c2 = HashMap::new();
        c2.insert(10, vec![10]);
        reg.set_entity_components(1, c2);

        let arch_b = reg.entity_archetype_id(1).unwrap();
        assert_ne!(arch_a, arch_b);

        assert!(reg.read_component(1, 20).is_none());
        assert_eq!(reg.read_component(1, 10).unwrap(), &vec![10]);
    }

    #[test]
    fn test_registry_remove_entity_cleans_up() {
        let mut reg = Registry::default();

        let mut c1 = HashMap::new();
        c1.insert(10, vec![10]);
        reg.set_entity_components(1, c1);

        let arch_id = reg.entity_archetype_id(1).unwrap();
        assert_eq!(reg.get_archetype(arch_id).unwrap().entity_count(), 1);

        reg.remove_entity(1);
        assert!(reg.entity_archetype_id(1).is_none());
        assert_eq!(reg.get_archetype(arch_id).unwrap().entity_count(), 0);
    }

    #[test]
    fn test_registry_remove_entity_nonexistent_is_noop() {
        let mut reg = Registry::default();
        reg.remove_entity(999);
        // no panic
    }

    #[test]
    fn test_registry_read_component_nonexistent() {
        let reg = Registry::default();
        assert!(reg.read_component(1, 10).is_none());
    }

    #[test]
    fn test_find_matching_archetypes_basic() {
        let mut reg = Registry::default();

        // Archetype A: {10, 20}
        let mut c1 = HashMap::new();
        c1.insert(10, vec![]);
        c1.insert(20, vec![]);
        reg.set_entity_components(1, c1);

        // Archetype B: {10, 30}
        let mut c2 = HashMap::new();
        c2.insert(10, vec![]);
        c2.insert(30, vec![]);
        reg.set_entity_components(2, c2);

        // Find all archetypes with component 10
        let matching = reg.find_matching_archetypes(10, &[], &[]);
        assert_eq!(matching.len(), 2);

        // Find archetypes with 10 AND 20
        let matching = reg.find_matching_archetypes(10, &[20], &[]);
        assert_eq!(matching.len(), 1);

        // Find archetypes with 10 but NOT 30
        let matching = reg.find_matching_archetypes(10, &[], &[30]);
        assert_eq!(matching.len(), 1);
    }

    #[test]
    fn test_find_matching_archetypes_no_match() {
        let mut reg = Registry::default();
        let mut c = HashMap::new();
        c.insert(10, vec![]);
        reg.set_entity_components(1, c);

        let matching = reg.find_matching_archetypes(999, &[], &[]);
        assert!(matching.is_empty());

        let matching = reg.find_matching_archetypes(10, &[999], &[]);
        assert!(matching.is_empty());
    }

    #[test]
    fn test_find_matching_archetypes_with_and_without() {
        let mut reg = Registry::default();

        // {10, 20}
        let mut c1 = HashMap::new();
        c1.insert(10, vec![]);
        c1.insert(20, vec![]);
        reg.set_entity_components(1, c1);

        // {10, 20, 30}
        let mut c2 = HashMap::new();
        c2.insert(10, vec![]);
        c2.insert(20, vec![]);
        c2.insert(30, vec![]);
        reg.set_entity_components(2, c2);

        // Has 10 AND 20, does NOT have 30
        let matching = reg.find_matching_archetypes(10, &[20], &[30]);
        assert_eq!(matching.len(), 1);
    }

    #[test]
    fn test_registry_multiple_entities_same_archetype() {
        let mut reg = Registry::default();

        let mut c = HashMap::new();
        c.insert(10, vec![10]);
        c.insert(20, vec![20]);
        reg.set_entity_components(1, c);

        let mut c2 = HashMap::new();
        c2.insert(10, vec![100]);
        c2.insert(20, vec![200]);
        reg.set_entity_components(2, c2);

        let arch_id = reg.entity_archetype_id(1).unwrap();
        assert_eq!(reg.entity_archetype_id(2).unwrap(), arch_id);

        let arch = reg.get_archetype(arch_id).unwrap();
        assert_eq!(arch.entity_count(), 2);

        // Verify data integrity via ComponentStorage iter
        let values: Vec<&Vec<u8>> = arch.iter_component(10).collect();
        assert_eq!(values.len(), 2);
        assert!(values.contains(&&vec![10]));
        assert!(values.contains(&&vec![100]));
    }

    #[test]
    fn test_registry_update_in_place_same_archetype() {
        let mut reg = Registry::default();

        let mut c = HashMap::new();
        c.insert(10, vec![10]);
        reg.set_entity_components(1, c);

        let arch_a = reg.entity_archetype_id(1).unwrap();

        // Update same component — same archetype
        let mut c2 = HashMap::new();
        c2.insert(10, b"updated".to_vec());
        reg.set_entity_components(1, c2);

        let arch_b = reg.entity_archetype_id(1).unwrap();
        assert_eq!(arch_a, arch_b);
        assert_eq!(reg.read_component(1, 10).unwrap(), &b"updated".to_vec());
    }

    #[test]
    fn test_registry_multiple_entities_different_archetypes() {
        let mut reg = Registry::default();

        // Entity 1: {10}
        let mut c1 = HashMap::new();
        c1.insert(10, vec![10]);
        reg.set_entity_components(1, c1);

        // Entity 2: {20}
        let mut c2 = HashMap::new();
        c2.insert(20, vec![20]);
        reg.set_entity_components(2, c2);

        assert_ne!(
            reg.entity_archetype_id(1).unwrap(),
            reg.entity_archetype_id(2).unwrap()
        );
        assert_eq!(
            reg.get_archetype(reg.entity_archetype_id(1).unwrap())
                .unwrap()
                .entity_count(),
            1
        );
        assert_eq!(
            reg.get_archetype(reg.entity_archetype_id(2).unwrap())
                .unwrap()
                .entity_count(),
            1
        );
    }
}
