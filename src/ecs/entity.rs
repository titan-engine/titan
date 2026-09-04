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
