use titan::inspection::{FieldMetadata, InspectionConfig, Inspector};
use titan::{App, Component, Inspect};
use titan_protocol::{EntityId, ErrorCode, Request, RequestEnvelope, Response, ResponseOutcome};

const LIMIT: i32 = 14;

#[derive(Component, Inspect)]
struct Coordinates {
    /// Map tile coordinate
    #[inspect(writable, minimum = 0, maximum = LIMIT - 1, unit = "tile")]
    x: i32,
    /// Observed value
    #[inspect(unit = "tile")]
    y: i32,
    // No serde, Clone, or Copy implementation is needed for an opaque field.
    opaque: Opaque,
}

struct Opaque;

#[derive(Component)]
struct Internal(Opaque);

#[derive(Component, Inspect)]
struct Flags {
    /// Whether the feature is enabled
    #[inspect(writable)]
    enabled: bool,
    /// Whether the feature is visible
    #[inspect]
    visible: bool,
}

fn metadata(description: &str, writable: bool, maximum: Option<f64>) -> FieldMetadata {
    FieldMetadata {
        type_name: "i32".into(),
        description: description.into(),
        writable,
        minimum: writable.then_some(0.0),
        maximum,
        unit: Some("tile".into()),
    }
}

#[test]
fn derive_matches_manual_metadata_and_preserves_policy_and_validation() {
    let mut config = InspectionConfig::controlled("derive-test", "test");
    config.mutation_enabled = true;
    let mut inspector = Inspector::new(config.clone());
    inspector.register_inspectable::<Coordinates>().unwrap();
    let mut manual = Inspector::new(config);
    manual
        .register_field::<Coordinates, i32>(
            "x",
            metadata("Map tile coordinate", true, Some(13.0)),
            |p| p.x,
            |_, _| Ok(()),
            |p, value| p.x = value,
        )
        .unwrap();
    manual
        .register_read_only_field::<Coordinates, i32>(
            "y",
            metadata("Observed value", false, None),
            |p| p.y,
        )
        .unwrap();
    assert_eq!(
        inspector.component_field_metadata(),
        manual.component_field_metadata()
    );

    let mut app = App::new();
    let entity = app.world_mut().spawn();
    app.world_mut()
        .insert(
            entity,
            Coordinates {
                x: 2,
                y: 7,
                opaque: Opaque,
            },
        )
        .unwrap();
    app.world_mut().insert(entity, Internal(Opaque)).unwrap();
    let _ = &app.world().get::<Coordinates>(entity).unwrap().opaque;
    let component = std::any::type_name::<Coordinates>().to_owned();
    let id = EntityId {
        index: entity.index(),
        generation: entity.generation(),
    };
    let edit = |field: &str, value| Request::SetField {
        entity: id,
        component: component.clone(),
        field: field.into(),
        value,
    };
    let send = |inspector: &mut Inspector, app: &mut App, request| {
        inspector.handle(app, &RequestEnvelope::new("test", request))
    };
    for value in [0.into(), 13.into()] {
        assert!(matches!(
            send(&mut inspector, &mut app, edit("x", value)).outcome,
            ResponseOutcome::Success { .. }
        ));
    }
    for value in [
        (-1).into(),
        14.into(),
        "3".into(),
        1.5.into(),
        serde_json::Value::Null,
    ] {
        let result = send(&mut inspector, &mut app, edit("x", value));
        assert_eq!(result.state_revision, 2);
        assert!(
            matches!(result.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::InvalidValue)
        );
        assert_eq!(app.world().get::<Coordinates>(entity).unwrap().x, 13);
    }
    let result = send(&mut inspector, &mut app, edit("y", 8.into()));
    assert!(
        matches!(result.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::ReadOnly)
    );
    let result = send(&mut inspector, &mut app, Request::Entity { entity: id });
    let ResponseOutcome::Success {
        response: Response::Entity(details),
    } = result.outcome
    else {
        panic!("expected entity");
    };
    assert_eq!(
        details.components[&component],
        serde_json::json!({"x": 13, "y": 7})
    );
    assert_eq!(details.component_fields[&component].len(), 2);
    assert!(
        !details
            .component_fields
            .contains_key(std::any::type_name::<Internal>())
    );

    let mut disabled = Inspector::new(InspectionConfig::controlled("disabled", "test"));
    disabled.register_inspectable::<Coordinates>().unwrap();
    let result = send(&mut disabled, &mut app, edit("x", 1.into()));
    assert!(
        matches!(result.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::MutationDisabled)
    );
    assert_eq!(app.world().get::<Coordinates>(entity).unwrap().x, 13);
    assert!(inspector.register_inspectable::<Coordinates>().is_err());
}

#[test]
fn derived_bounds_use_existing_registration_validation() {
    #[derive(Component, Inspect)]
    struct Invalid {
        #[inspect(minimum = 10, maximum = 1)]
        x: i32,
    }
    let mut inspector = Inspector::new(InspectionConfig::controlled("invalid", "test"));
    assert!(inspector.register_inspectable::<Invalid>().is_err());
    assert!(inspector.component_field_metadata().is_empty());
}

#[test]
fn derived_booleans_match_manual_metadata_and_preserve_mutation_semantics() {
    let mut config = InspectionConfig::controlled("derive-bool-test", "test");
    config.mutation_enabled = true;
    let mut inspector = Inspector::new(config.clone());
    inspector.register_inspectable::<Flags>().unwrap();
    let mut manual = Inspector::new(config);
    manual
        .register_field::<Flags, bool>(
            "enabled",
            FieldMetadata {
                type_name: "bool".into(),
                description: "Whether the feature is enabled".into(),
                writable: true,
                minimum: None,
                maximum: None,
                unit: None,
            },
            |flags| flags.enabled,
            |_, _| Ok(()),
            |flags, value| flags.enabled = value,
        )
        .unwrap();
    manual
        .register_read_only_field::<Flags, bool>(
            "visible",
            FieldMetadata {
                type_name: "bool".into(),
                description: "Whether the feature is visible".into(),
                writable: false,
                minimum: None,
                maximum: None,
                unit: None,
            },
            |flags| flags.visible,
        )
        .unwrap();
    assert_eq!(
        inspector.component_field_metadata(),
        manual.component_field_metadata()
    );

    let mut app = App::new();
    let entity = app.world_mut().spawn();
    app.world_mut()
        .insert(
            entity,
            Flags {
                enabled: false,
                visible: true,
            },
        )
        .unwrap();
    let id = EntityId {
        index: entity.index(),
        generation: entity.generation(),
    };
    let component = std::any::type_name::<Flags>().to_owned();
    let edit = |field: &str, value| Request::SetField {
        entity: id,
        component: component.clone(),
        field: field.into(),
        value,
    };
    let send = |inspector: &mut Inspector, app: &mut App, request| {
        inspector.handle(app, &RequestEnvelope::new("test", request))
    };

    for value in [true, false] {
        let result = send(&mut inspector, &mut app, edit("enabled", value.into()));
        assert!(matches!(result.outcome, ResponseOutcome::Success { .. }));
    }
    for value in [
        serde_json::json!(1),
        serde_json::json!("true"),
        serde_json::Value::Null,
    ] {
        let result = send(&mut inspector, &mut app, edit("enabled", value));
        assert_eq!(result.state_revision, 2);
        assert!(
            matches!(result.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::InvalidValue)
        );
        assert!(!app.world().get::<Flags>(entity).unwrap().enabled);
    }
    let denied = send(&mut inspector, &mut app, edit("visible", false.into()));
    assert_eq!(denied.state_revision, 2);
    assert!(
        matches!(denied.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::ReadOnly)
    );
    assert!(app.world().get::<Flags>(entity).unwrap().visible);

    let mut disabled = Inspector::new(InspectionConfig::controlled("disabled", "test"));
    disabled.register_inspectable::<Flags>().unwrap();
    let denied = send(&mut disabled, &mut app, edit("enabled", true.into()));
    assert_eq!(denied.state_revision, 0);
    assert!(
        matches!(denied.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::MutationDisabled)
    );
    assert!(!app.world().get::<Flags>(entity).unwrap().enabled);
}
