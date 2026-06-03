use std::collections::HashMap;

pub type ArchetypeId = u64;
pub type ThingId = u128;

#[derive(Default)]
pub struct Registry {
    archetypes: HashMap<ArchetypeId, Archetype>,
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

    fn compute_id(hashes: &[u64]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut sorted = hashes.to_vec();
        sorted.sort();
        let mut hasher = DefaultHasher::new();
        for h in &sorted {
            h.hash(&mut hasher);
        }
        hasher.finish()
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
            for i in 0..self.entities.len() {
                storage.append_empty_slot(i);
            }
            self.components.insert(attr_hash, storage);
        }
    }

    pub fn spawn(&mut self, thing_id: ThingId) -> usize {
        let slot = self.entities.len();
        self.entity_to_slot.insert(thing_id, slot);
        self.entities.push(thing_id);
        for storage in self.components.values_mut() {
            storage.append_empty_slot(slot);
        }
        slot
    }

    pub fn despawn(&mut self, thing_id: ThingId) -> Option<usize> {
        let slot = *self.entity_to_slot.get(&thing_id)?;
        let last_thing = *self.entities.last()?;
        for storage in self.components.values_mut() {
            storage.swap_with_last(slot);
        }
        if slot < self.entities.len() - 1 {
            self.entity_to_slot.insert(last_thing, slot);
        }
        self.entity_to_slot.remove(&thing_id);
        self.entities.pop();
        Some(slot)
    }

    pub fn set_component(&mut self, thing_id: ThingId, attr_hash: u64, data: Vec<u8>) -> Option<()> {
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
}

pub struct ComponentStorage {
    slots: Vec<Option<Vec<u8>>>,
}

impl ComponentStorage {
    fn new() -> Self {
        Self { slots: Vec::new() }
    }

    fn append_empty_slot(&mut self, _slot: usize) {
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