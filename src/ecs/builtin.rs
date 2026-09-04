use std::error::Error;
use std::fmt;

use super::{Component, Entity, World};

/// An optional human-readable name attached to an important entity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(String);

impl Name {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Component for Name {}

impl fmt::Display for Name {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Failure to resolve exactly one entity by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindNameError {
    NotFound(String),
    Ambiguous { name: String, matches: Vec<Entity> },
}

impl fmt::Display for FindNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(name) => write!(formatter, "no entity is named {name:?}"),
            Self::Ambiguous { name, matches } => {
                write!(
                    formatter,
                    "entity name {name:?} has {} matches",
                    matches.len()
                )
            }
        }
    }
}

impl Error for FindNameError {}

impl World {
    /// Resolves a unique entity with an exactly matching [`Name`].
    pub fn find_named(&self, name: &str) -> Result<Entity, FindNameError> {
        let matches: Vec<_> = self
            .iter::<Name>()
            .filter_map(|(entity, candidate)| (candidate.as_str() == name).then_some(entity))
            .collect();
        match matches.as_slice() {
            [] => Err(FindNameError::NotFound(name.to_owned())),
            [entity] => Ok(*entity),
            _ => Err(FindNameError::Ambiguous {
                name: name.to_owned(),
                matches,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FindNameError, Name};
    use crate::World;

    #[test]
    fn names_are_optional_and_ambiguity_is_explicit() {
        let mut world = World::new();
        let unnamed = world.spawn();
        let first = world.spawn();
        world.insert(first, Name::new("player")).unwrap();

        assert_eq!(world.find_named("player"), Ok(first));
        assert!(matches!(
            world.find_named("missing"),
            Err(FindNameError::NotFound(_))
        ));

        let second = world.spawn();
        world.insert(second, Name::new("player")).unwrap();
        assert_eq!(
            world.find_named("player"),
            Err(FindNameError::Ambiguous {
                name: "player".to_owned(),
                matches: vec![first, second],
            })
        );
        assert!(world.is_alive(unnamed));
    }
}
