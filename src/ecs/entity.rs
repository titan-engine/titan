use core::fmt;

/// A generational handle to an entity in a [`World`](super::World).
///
/// Reusing an entity index changes its generation, so a handle to a despawned
/// entity never starts referring to a newly spawned entity by accident.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    pub(crate) const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Reconstructs an entity handle from structured external data.
    ///
    /// This does not prove that the entity is alive in any particular world;
    /// use [`World::is_alive`](super::World::is_alive) before accessing it.
    pub const fn from_parts(index: u32, generation: u32) -> Self {
        Self::new(index, generation)
    }

    /// Returns the entity's densely allocated index.
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Returns the generation associated with this handle.
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}v{}", self.index, self.generation)
    }
}
