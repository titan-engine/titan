use super::Entity;

#[derive(Clone, Copy, Debug)]
struct EntitySlot {
    generation: u32,
    alive: bool,
    reserved: bool,
}

#[derive(Default)]
pub(crate) struct EntityAllocator {
    entities: Vec<EntitySlot>,
    free_entities: Vec<u32>,
    live_entity_count: usize,
}
impl EntityAllocator {
    pub(crate) fn reserve_entity(&mut self) -> Entity {
        self.allocate_entity(true)
    }

    pub(crate) fn allocate_entity(&mut self, reserved: bool) -> Entity {
        if let Some(index) = self.free_entities.pop() {
            let slot = &mut self.entities[index as usize];
            debug_assert!(!slot.alive && !slot.reserved);
            slot.alive = !reserved;
            slot.reserved = reserved;
            self.live_entity_count += usize::from(!reserved);
            return Entity::new(index, slot.generation);
        }

        let index = u32::try_from(self.entities.len()).expect("entity capacity exceeded");
        self.entities.push(EntitySlot {
            generation: 0,
            alive: !reserved,
            reserved,
        });
        self.live_entity_count += usize::from(!reserved);
        Entity::new(index, 0)
    }

    pub(crate) fn activate_reserved(&mut self, entity: Entity) -> bool {
        let Some(slot) = self.entities.get_mut(entity.index() as usize) else {
            return false;
        };
        if slot.generation != entity.generation() || !slot.reserved || slot.alive {
            return false;
        }
        slot.reserved = false;
        slot.alive = true;
        self.live_entity_count += 1;
        true
    }

    /// Returns whether an entity handle currently refers to a live entity.
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.entities
            .get(entity.index() as usize)
            .is_some_and(|slot| slot.alive && slot.generation == entity.generation())
    }

    /// Returns the number of live entities.
    pub const fn entity_count(&self) -> usize {
        self.live_entity_count
    }

    /// Iterates over live entities in ascending allocator-index order.
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.entities
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.alive)
            .map(|(index, slot)| Entity::new(index as u32, slot.generation))
    }

    pub(crate) fn release(&mut self, entity: Entity) {
        let slot = &mut self.entities[entity.index() as usize];
        slot.alive = false;
        slot.reserved = false;
        self.live_entity_count -= 1;
        if let Some(next_generation) = slot.generation.checked_add(1) {
            slot.generation = next_generation;
            self.free_entities.push(entity.index());
        }
    }
}
