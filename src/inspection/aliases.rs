use super::*;

impl Inspector {
    /// Exposes a legacy component name at this inspector's protocol boundary.
    ///
    /// Rust/ECS type identity is unchanged. Register at most one alias per type;
    /// aliases must not collide with another registered component's identity or
    /// alias. Components first encountered in a world are checked when inspecting
    /// or mutating that world. The original name is not accepted in requests once
    /// an alias is registered.
    pub fn register_component_alias<T: Component>(
        &mut self,
        legacy_name: impl Into<String>,
    ) -> Result<&mut Self, ProtocolError> {
        let identity = std::any::type_name::<T>();
        let alias = legacy_name.into();
        self.validate_component_identity(identity)?;
        if alias.trim().is_empty()
            || alias == identity
            || self.component_aliases.contains_key(identity)
            || self.component_aliases.contains_key(&alias)
            || self.component_aliases.values().any(|name| name == &alias)
            || self.fields.contains_key(&alias)
        {
            return Err(alias_collision());
        }
        self.component_aliases.insert(identity.into(), alias);
        Ok(self)
    }

    /// Maps an exact Rust component identity to its exposed inspection name.
    /// Call [`Self::validate_component_aliases`] before describing a world.
    pub fn component_name<'a>(&'a self, identity: &'a str) -> &'a str {
        self.component_aliases
            .get(identity)
            .map_or(identity, String::as_str)
    }

    pub(super) fn component_identity<'a>(&'a self, exposed: &'a str) -> Option<&'a str> {
        self.component_aliases
            .iter()
            .find_map(|(identity, alias)| (alias == exposed).then_some(identity.as_str()))
            .or_else(|| (!self.component_aliases.contains_key(exposed)).then_some(exposed))
    }

    pub(super) fn validate_component_identity(&self, identity: &str) -> Result<(), ProtocolError> {
        if self
            .component_aliases
            .values()
            .any(|alias| alias == identity)
        {
            return Err(alias_collision());
        }
        Ok(())
    }

    /// Rejects aliases which would hide another component registered in a world.
    /// Diagnostic adapters must check this before mapping world metadata; on error
    /// they should omit the ambiguous description rather than merge components.
    pub fn validate_component_aliases(&self, world: &World) -> Result<(), ProtocolError> {
        if self.component_aliases.is_empty() {
            return Ok(());
        }
        for metadata in world.component_metadata() {
            self.validate_component_identity(metadata.type_name)?;
        }
        Ok(())
    }
}

fn alias_collision() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::InvalidValue,
        "component alias must be nonempty and must not collide with a component identity or alias",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use titan_protocol::{EntityQuery, PageRequest, ResponseOutcome};

    #[derive(crate::Component)]
    struct Position(i32);
    #[derive(crate::Component)]
    struct Other;

    fn inspector() -> Inspector {
        let mut config = InspectionConfig::controlled("alias-test", "test");
        config.mutation_enabled = true;
        Inspector::new(config)
    }
    fn fields(inspector: &mut Inspector) -> Result<(), ProtocolError> {
        let metadata = FieldMetadata {
            type_name: "i32".into(),
            description: String::new(),
            writable: false,
            minimum: Some(0.0),
            maximum: Some(10.0),
            unit: None,
        };
        inspector.register_field::<Position, i32>(
            "x",
            metadata.clone(),
            |p| p.0,
            |_, _| Ok(()),
            |p, v| p.0 = v,
        )?;
        inspector.register_read_only_field::<Position, i32>("read", metadata, |p| p.0)?;
        Ok(())
    }
    fn send(inspector: &mut Inspector, app: &mut App, request: Request) -> ResponseOutcome {
        inspector
            .handle(app, &RequestEnvelope::new("test", request))
            .outcome
    }
    fn set(entity: Entity, name: &str, field: &str, value: i32) -> Request {
        Request::SetField {
            entity: to_protocol_entity(entity),
            component: name.into(),
            field: field.into(),
            value: value.into(),
        }
    }
    #[test]
    fn aliases_preserve_fields_filters_and_permissions_in_either_registration_order() {
        for alias_first in [false, true] {
            let mut inspector = inspector();
            if alias_first {
                inspector
                    .register_component_alias::<Position>("legacy::Position")
                    .unwrap();
            }
            fields(&mut inspector).unwrap();
            if !alias_first {
                inspector
                    .register_component_alias::<Position>("legacy::Position")
                    .unwrap();
            }
            assert_eq!(
                inspector
                    .component_field_metadata()
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>(),
                ["legacy::Position"]
            );
            let mut app = App::new();
            let entity = app.world_mut().spawn_with((Position(1), Other));
            let ResponseOutcome::Success {
                response: Response::Entity(details),
            } = send(
                &mut inspector,
                &mut app,
                Request::Entity {
                    entity: to_protocol_entity(entity),
                },
            )
            else {
                panic!("entity failed")
            };
            assert_eq!(details.components["legacy::Position"]["x"], 1);
            assert!(details.component_fields.contains_key("legacy::Position"));
            assert!(
                !details
                    .components
                    .contains_key(std::any::type_name::<Position>())
            );
            for (name, count) in [
                ("legacy::Position", 1),
                (std::any::type_name::<Position>(), 0),
            ] {
                let ResponseOutcome::Success {
                    response: Response::Entities(page),
                } = send(
                    &mut inspector,
                    &mut app,
                    Request::Entities {
                        query: EntityQuery {
                            name: None,
                            with_components: vec![name.into()],
                        },
                        page: PageRequest::default(),
                    },
                )
                else {
                    panic!("filter failed")
                };
                assert_eq!(page.entities.len(), count);
            }
            assert!(matches!(
                send(
                    &mut inspector,
                    &mut app,
                    set(entity, "legacy::Position", "x", 5)
                ),
                ResponseOutcome::Success { .. }
            ));
            for (name, field, value, code) in [
                ("legacy::Position", "read", 2, ErrorCode::ReadOnly),
                ("legacy::Position", "x", 11, ErrorCode::InvalidValue),
                (
                    std::any::type_name::<Position>(),
                    "x",
                    2,
                    ErrorCode::NotFound,
                ),
            ] {
                assert!(
                    matches!(send(&mut inspector, &mut app, set(entity, name, field, value)), ResponseOutcome::Failure { error } if error.code == code)
                );
                assert_eq!(app.world().get::<Position>(entity).unwrap().0, 5);
            }
            inspector.set_mutation_enabled(false);
            assert!(matches!(
                send(
                    &mut inspector,
                    &mut app,
                    set(entity, "legacy::Position", "x", 2)
                ),
                ResponseOutcome::Failure { .. }
            ));
            assert_eq!(app.world().get::<Position>(entity).unwrap().0, 5);
        }
    }

    #[test]
    fn aliases_reject_registration_collisions_and_late_world_collisions() {
        let mut inspector = inspector();
        assert!(inspector.register_component_alias::<Position>(" ").is_err());
        assert!(
            inspector
                .register_component_alias::<Position>(std::any::type_name::<Position>())
                .is_err()
        );
        inspector
            .register_component_alias::<Position>("legacy")
            .unwrap();
        assert!(
            inspector
                .register_component_alias::<Position>("different")
                .is_err()
        );
        assert!(
            inspector
                .register_component_alias::<Other>("legacy")
                .is_err()
        );
        assert!(
            inspector
                .register_component_alias::<Other>(std::any::type_name::<Position>())
                .is_err()
        );
        let mut late = self::inspector();
        fields(&mut late).unwrap();
        assert!(
            late.register_component_alias::<Other>(std::any::type_name::<Position>())
                .is_err()
        );
        let mut late = self::inspector();
        late.register_component_alias::<Other>(std::any::type_name::<Position>())
            .unwrap();
        assert!(fields(&mut late).is_err());
        let mut app = App::new();
        let entity = app.world_mut().spawn_with((Position(7), Other));
        assert!(late.validate_component_aliases(app.world()).is_err());
        for request in [
            Request::Entity {
                entity: to_protocol_entity(entity),
            },
            Request::Entities {
                query: EntityQuery::default(),
                page: PageRequest::default(),
            },
            set(entity, std::any::type_name::<Position>(), "x", 3),
        ] {
            assert!(
                matches!(send(&mut late, &mut app, request), ResponseOutcome::Failure { error } if error.code == ErrorCode::InvalidValue)
            );
            assert_eq!(app.world().get::<Position>(entity).unwrap().0, 7);
        }
    }
}
