//! Transport-neutral inspection of a Titan [`App`](crate::App).

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use titan_protocol::{
    Capabilities, CaptureResult, CommandMetadata, EntityDetails, EntityId, EntityPage,
    EntitySummary, ErrorCode, InputValue, Operation, ProtocolError, Request, RequestEnvelope,
    Response, ResponseEnvelope, RunMode, RuntimeStatus, SCHEMA_VERSION,
};

use crate::{App, FixedTime, Name};

/// Runtime settings visible through inspection capabilities.
#[derive(Clone, Debug)]
pub struct InspectionConfig {
    pub instance_id: String,
    pub project: String,
    pub run_mode: RunMode,
    pub controlled: bool,
    pub mutation_enabled: bool,
}

impl InspectionConfig {
    pub fn controlled(instance_id: impl Into<String>, project: impl Into<String>) -> Self {
        Self {
            instance_id: instance_id.into(),
            project: project.into(),
            run_mode: RunMode::Headless,
            controlled: true,
            mutation_enabled: false,
        }
    }
}

type CommandHandler =
    Box<dyn FnMut(&mut App, serde_json::Value) -> Result<(), ProtocolError> + Send>;
type InputHandler = Box<
    dyn FnMut(&mut App, u64, &BTreeMap<String, InputValue>) -> Result<(), ProtocolError> + Send,
>;
type CaptureHandler = Box<dyn FnMut(&App) -> Result<CaptureResult, ProtocolError> + Send>;

struct RegisteredCommand {
    metadata: CommandMetadata,
    handler: CommandHandler,
}

/// Executes typed inspection requests at a caller-controlled safe point.
///
/// A transport adapter should enqueue requests and call [`handle`](Self::handle)
/// only when it has exclusive access to the application. Transport threads
/// must never access the ECS world directly.
pub struct Inspector {
    config: InspectionConfig,
    state_revision: u64,
    commands: BTreeMap<String, RegisteredCommand>,
    input_handler: Option<InputHandler>,
    capture_handler: Option<CaptureHandler>,
}

impl Inspector {
    pub const fn new(config: InspectionConfig) -> Self {
        Self {
            config,
            state_revision: 0,
            commands: BTreeMap::new(),
            input_handler: None,
            capture_handler: None,
        }
    }

    /// Registers a game command with typed JSON-object arguments.
    ///
    /// Handlers run with exclusive application access. Validate before mutating:
    /// failed handlers and deferred operations are reported but are not rolled back.
    /// Use `#[serde(deny_unknown_fields)]` on argument types to reject extra fields.
    /// Duplicate or blank names are rejected without replacing an existing command.
    pub fn register_command<A: DeserializeOwned + 'static>(
        &mut self,
        metadata: CommandMetadata,
        mut handler: impl FnMut(&mut App, A) -> Result<(), ProtocolError> + Send + 'static,
    ) -> Result<&mut Self, ProtocolError> {
        if metadata.name.trim().is_empty() || self.commands.contains_key(&metadata.name) {
            return Err(ProtocolError::new(
                ErrorCode::InvalidValue,
                "command name must be nonempty and unique",
            ));
        }
        self.commands.insert(
            metadata.name.clone(),
            RegisteredCommand {
                metadata,
                handler: Box::new(move |app, value| {
                    let arguments = serde_json::from_value(value).map_err(|error| {
                        ProtocolError::new(
                            ErrorCode::InvalidValue,
                            format!("invalid command arguments: {error}"),
                        )
                    })?;
                    handler(app, arguments)
                }),
            },
        );
        Ok(self)
    }

    /// Installs the game's deterministic input adapter, replacing any prior hook.
    /// The adapter validates actions and queues them for the requested future frame.
    /// Deferred writes are applied before and after the hook, including on failure.
    /// Like commands, hooks are not transactional; validate before mutating.
    pub fn register_input_handler(
        &mut self,
        handler: impl FnMut(&mut App, u64, &BTreeMap<String, InputValue>) -> Result<(), ProtocolError>
        + Send
        + 'static,
    ) -> &mut Self {
        self.input_handler = Some(Box::new(handler));
        self
    }

    /// Installs a read-only capture adapter, replacing any prior hook.
    pub fn register_capture_handler(
        &mut self,
        handler: impl FnMut(&App) -> Result<CaptureResult, ProtocolError> + Send + 'static,
    ) -> &mut Self {
        self.capture_handler = Some(Box::new(handler));
        self
    }

    pub fn handle(&mut self, app: &mut App, request: &RequestEnvelope) -> ResponseEnvelope {
        if request.schema_version != SCHEMA_VERSION {
            return self.failure(
                app,
                request,
                ProtocolError::new(
                    ErrorCode::ProtocolMismatch,
                    format!(
                        "runtime schema is {SCHEMA_VERSION}, request schema is {}",
                        request.schema_version
                    ),
                ),
            );
        }
        if request
            .target_instance
            .as_ref()
            .is_some_and(|target| target != &self.config.instance_id)
        {
            return self.failure(
                app,
                request,
                ProtocolError::new(
                    ErrorCode::NotFound,
                    "target runtime instance does not match",
                ),
            );
        }

        let result = self.execute(app, &request.request);
        match result {
            Ok(response) => ResponseEnvelope::success(
                request,
                &self.config.instance_id,
                current_frame(app),
                self.state_revision,
                response,
            ),
            Err(error) => self.failure(app, request, error),
        }
    }

    fn execute(&mut self, app: &mut App, request: &Request) -> Result<Response, ProtocolError> {
        match request {
            Request::Capabilities => Ok(Response::Capabilities(self.capabilities())),
            Request::Status => Ok(Response::Status(RuntimeStatus {
                project: self.config.project.clone(),
                run_mode: self.config.run_mode,
                current_frame: current_frame(app),
                paused: self.config.controlled,
            })),
            Request::Entities { query, page } => {
                if page.limit == 0 {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidValue,
                        "entity page limit must be positive",
                    ));
                }
                let start = match page.cursor.as_deref() {
                    Some(cursor) => cursor.parse::<usize>().map_err(|_| {
                        ProtocolError::new(ErrorCode::InvalidValue, "invalid entity page cursor")
                    })?,
                    None => 0,
                };
                let limit = usize::try_from(page.limit.min(1_000)).unwrap_or(1_000);
                let matches: Vec<_> = app
                    .world()
                    .entities()
                    .filter_map(|entity| {
                        let name = app.world().get::<Name>(entity).map(|name| name.to_string());
                        let components: Vec<_> = app
                            .world()
                            .component_type_names(entity)
                            .into_iter()
                            .map(str::to_owned)
                            .collect();
                        let name_matches = query
                            .name
                            .as_ref()
                            .is_none_or(|query_name| name.as_ref() == Some(query_name));
                        let components_match = query
                            .with_components
                            .iter()
                            .all(|required| components.contains(required));
                        (name_matches && components_match).then_some(EntitySummary {
                            id: to_protocol_entity(entity),
                            name,
                            components,
                        })
                    })
                    .collect();
                let entities = matches.iter().skip(start).take(limit).cloned().collect();
                let consumed = start.saturating_add(limit).min(matches.len());
                let next_cursor = (consumed < matches.len()).then(|| consumed.to_string());
                Ok(Response::Entities(EntityPage {
                    entities,
                    next_cursor,
                }))
            }
            Request::Entity { entity } => {
                let entity = crate::Entity::from_parts(entity.index, entity.generation);
                if !app.world().is_alive(entity) {
                    return Err(ProtocolError::new(
                        ErrorCode::NotFound,
                        "entity is not alive",
                    ));
                }
                let components = app
                    .world()
                    .component_type_names(entity)
                    .into_iter()
                    .map(|name| (name.to_owned(), serde_json::Value::Null))
                    .collect::<BTreeMap<_, _>>();
                Ok(Response::Entity(EntityDetails {
                    id: to_protocol_entity(entity),
                    name: app.world().get::<Name>(entity).map(|name| name.to_string()),
                    components,
                }))
            }
            Request::Step { frames } => {
                if !self.config.controlled {
                    return Err(ProtocolError::new(
                        ErrorCode::NotControlled,
                        "the runtime clock is not controlled by the inspector",
                    ));
                }
                app.try_advance_fixed(*frames).map_err(deferred_failure)?;
                self.state_revision = self.state_revision.wrapping_add(1);
                Ok(Response::Stepped {
                    frames: *frames,
                    current_frame: current_frame(app),
                })
            }
            Request::SetField { .. } if !self.config.mutation_enabled => Err(ProtocolError::new(
                ErrorCode::MutationDisabled,
                "runtime mutation was not explicitly enabled",
            )),
            Request::SetField { .. } => Err(ProtocolError::new(
                ErrorCode::ReadOnly,
                "this component does not expose writable reflected fields",
            )),
            Request::Commands => Ok(Response::Commands {
                commands: self
                    .commands
                    .values()
                    .map(|command| command.metadata.clone())
                    .collect(),
            }),
            Request::Invoke { name, arguments } => {
                let command = self.commands.get_mut(name).ok_or_else(|| {
                    ProtocolError::new(ErrorCode::NotFound, format!("unknown game command: {name}"))
                })?;
                app.apply_deferred().map_err(deferred_failure)?;
                let outcome = (command.handler)(
                    app,
                    serde_json::Value::Object(arguments.clone().into_iter().collect()),
                );
                // Always drain this invocation's deferred writes, even when the
                // handler rejects after enqueueing them. Commands are not transactional.
                let deferred = app.apply_deferred().map_err(deferred_failure);
                deferred?;
                outcome?;
                self.state_revision = self.state_revision.wrapping_add(1);
                Ok(Response::Applied {
                    applied_frame: current_frame(app),
                })
            }
            Request::InjectInput { frame, actions } => {
                let handler = self
                    .input_handler
                    .as_mut()
                    .ok_or_else(unregistered_operation)?;
                if !self.config.controlled {
                    return Err(ProtocolError::new(
                        ErrorCode::NotControlled,
                        "input injection requires a controlled runtime",
                    ));
                }
                if *frame <= current_frame(app) {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidValue,
                        "input must target a future frame",
                    ));
                }
                app.apply_deferred().map_err(deferred_failure)?;
                let outcome = handler(app, *frame, actions);
                app.apply_deferred().map_err(deferred_failure)?;
                outcome?;
                self.state_revision = self.state_revision.wrapping_add(1);
                Ok(Response::Applied {
                    applied_frame: *frame,
                })
            }
            Request::Capture => {
                let handler = self
                    .capture_handler
                    .as_mut()
                    .ok_or_else(unregistered_operation)?;
                handler(app).map(Response::Capture)
            }
        }
    }

    fn capabilities(&self) -> Capabilities {
        let mut operations = vec![Operation::Inspect];
        if self.config.controlled {
            operations.push(Operation::Step);
        }
        if self.config.mutation_enabled {
            operations.push(Operation::Mutate);
        }
        if !self.commands.is_empty() {
            operations.push(Operation::Invoke);
        }
        if self.config.controlled && self.input_handler.is_some() {
            operations.push(Operation::InjectInput);
        }
        if self.capture_handler.is_some() {
            operations.push(Operation::Capture);
        }
        Capabilities {
            schema_version: SCHEMA_VERSION,
            run_mode: self.config.run_mode,
            mutation_enabled: self.config.mutation_enabled,
            controlled: self.config.controlled,
            operations,
        }
    }

    fn failure(
        &self,
        app: &App,
        request: &RequestEnvelope,
        error: ProtocolError,
    ) -> ResponseEnvelope {
        ResponseEnvelope::failure(
            request,
            &self.config.instance_id,
            current_frame(app),
            self.state_revision,
            error,
        )
    }
}

fn unregistered_operation() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unsupported,
        "this operation has not been registered by the game",
    )
}

fn deferred_failure(errors: Vec<crate::AppError>) -> ProtocolError {
    let mut error = ProtocolError::new(
        ErrorCode::Internal,
        "application schedule or deferred commands failed",
    );
    let mut deferred = Vec::new();
    let mut systems = Vec::new();
    for failure in errors {
        match failure {
            crate::AppError::Deferred(failure) => deferred.push(serde_json::json!({
                "entity": to_protocol_entity(failure.entity()),
                "operation": format!("{:?}", failure.operation()).to_lowercase(),
                "message": failure.to_string(),
            })),
            crate::AppError::System { system, error } => {
                let (kind, type_name) = match &error {
                    crate::SystemError::MissingResource { type_name } => {
                        ("missing_resource", *type_name)
                    }
                    crate::SystemError::ConflictingAccess { type_name, .. } => {
                        ("conflicting_access", *type_name)
                    }
                };
                systems.push(serde_json::json!({ "system": system, "kind": kind, "type_name": type_name, "message": error.to_string() }));
            }
        }
    }
    if !deferred.is_empty() {
        error
            .details
            .insert("deferred_errors".into(), deferred.into());
    }
    if !systems.is_empty() {
        error.details.insert("system_errors".into(), systems.into());
    }
    error
}

fn current_frame(app: &App) -> u64 {
    app.world()
        .resource::<FixedTime>()
        .expect("App always contains FixedTime")
        .tick()
}

fn to_protocol_entity(entity: crate::Entity) -> EntityId {
    EntityId {
        index: entity.index(),
        generation: entity.generation(),
    }
}

#[cfg(test)]
mod tests {
    use titan_protocol::{
        EntityId, EntityQuery, ErrorCode, PageRequest, Request, RequestEnvelope, Response,
        ResponseOutcome,
    };

    use super::{InspectionConfig, Inspector};
    use crate::{App, Component, FixedUpdate, Name};

    #[derive(Component)]
    struct Position(i32);

    fn inspected_app() -> (App, Inspector) {
        let mut app = App::new();
        let player = app.world_mut().spawn();
        app.world_mut().insert(player, Name::new("player")).unwrap();
        app.world_mut().insert(player, Position(0)).unwrap();
        app.add_systems(FixedUpdate, |world: &mut crate::World| {
            for (_, position) in world.iter_mut::<Position>() {
                position.0 += 1;
            }
        });
        let inspector = Inspector::new(InspectionConfig::controlled("instance-1", "test-game"));
        (app, inspector)
    }

    #[test]
    fn controlled_stepping_returns_the_exact_observed_frame() {
        let (mut app, mut inspector) = inspected_app();
        let request = RequestEnvelope::new("step", Request::Step { frames: 10 });

        let response = inspector.handle(&mut app, &request);

        assert_eq!(response.observed_frame, 10);
        assert_eq!(response.state_revision, 1);
        assert!(matches!(
            response.outcome,
            ResponseOutcome::Success {
                response: Response::Stepped {
                    frames: 10,
                    current_frame: 10
                }
            }
        ));
        assert_eq!(app.world().iter::<Position>().next().unwrap().1.0, 10);
    }

    #[test]
    fn entity_pages_expose_names_and_opaque_component_types() {
        let (mut app, mut inspector) = inspected_app();
        let request = RequestEnvelope::new(
            "entities",
            Request::Entities {
                query: EntityQuery {
                    name: Some("player".to_owned()),
                    with_components: Vec::new(),
                },
                page: PageRequest::default(),
            },
        );

        let response = inspector.handle(&mut app, &request);
        let ResponseOutcome::Success {
            response: Response::Entities(page),
        } = response.outcome
        else {
            panic!("expected an entity page")
        };

        assert_eq!(page.entities.len(), 1);
        assert_eq!(page.entities[0].name.as_deref(), Some("player"));
        assert!(
            page.entities[0]
                .components
                .iter()
                .any(|name| name.ends_with("::Position"))
        );
    }

    #[test]
    fn mutation_requires_explicit_enablement() {
        let (mut app, mut inspector) = inspected_app();
        let request = RequestEnvelope::new(
            "set",
            Request::SetField {
                entity: EntityId {
                    index: 0,
                    generation: 0,
                },
                component: "Position".to_owned(),
                field: "x".to_owned(),
                value: 2.into(),
            },
        );

        let response = inspector.handle(&mut app, &request);
        assert!(matches!(
            response.outcome,
            ResponseOutcome::Failure {
                error: titan_protocol::ProtocolError {
                    code: ErrorCode::MutationDisabled,
                    ..
                }
            }
        ));
    }
    fn metadata(name: &str) -> titan_protocol::CommandMetadata {
        titan_protocol::CommandMetadata {
            name: name.to_owned(),
            description: String::new(),
            arguments: Default::default(),
        }
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct MoveArgs {
        amount: i32,
    }

    #[test]
    fn commands_are_typed_sorted_and_only_success_advances_revision() {
        let (mut app, mut inspector) = inspected_app();
        inspector
            .register_command::<MoveArgs>(metadata("move"), |app, args| {
                if args.amount < 0 {
                    return Err(titan_protocol::ProtocolError::new(
                        ErrorCode::InvalidValue,
                        "amount must be positive",
                    ));
                }
                app.world_mut().iter_mut::<Position>().next().unwrap().1.0 += args.amount;
                Ok(())
            })
            .unwrap();
        inspector
            .register_command::<MoveArgs>(metadata("alpha"), |_, _| Ok(()))
            .unwrap();
        assert!(
            inspector
                .register_command::<MoveArgs>(metadata("move"), |_, _| Ok(()))
                .is_err()
        );
        let response = inspector.handle(&mut app, &RequestEnvelope::new("list", Request::Commands));
        let ResponseOutcome::Success {
            response: Response::Commands { commands },
        } = response.outcome
        else {
            panic!("expected command list")
        };
        assert_eq!(
            commands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "move"]
        );
        for arguments in [
            serde_json::json!({}),
            serde_json::json!({"amount": "bad"}),
            serde_json::json!({"amount": -1}),
            serde_json::json!({"amount": 2, "extra": true}),
        ] {
            let response = inspector.handle(
                &mut app,
                &RequestEnvelope::new(
                    "bad",
                    Request::Invoke {
                        name: "move".into(),
                        arguments: serde_json::from_value(arguments).unwrap(),
                    },
                ),
            );
            assert_eq!(response.state_revision, 0);
            assert!(matches!(
                response.outcome,
                ResponseOutcome::Failure {
                    error: titan_protocol::ProtocolError {
                        code: ErrorCode::InvalidValue,
                        ..
                    }
                }
            ));
            assert_eq!(app.world().iter::<Position>().next().unwrap().1.0, 0);
        }
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "valid",
                Request::Invoke {
                    name: "move".into(),
                    arguments: [("amount".into(), 5.into())].into(),
                },
            ),
        );
        assert_eq!(response.state_revision, 1);
        assert!(matches!(
            response.outcome,
            ResponseOutcome::Success {
                response: Response::Applied { applied_frame: 0 }
            }
        ));
        assert_eq!(app.world().iter::<Position>().next().unwrap().1.0, 5);
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "missing",
                Request::Invoke {
                    name: "missing".into(),
                    arguments: Default::default(),
                },
            ),
        );
        assert_eq!(response.state_revision, 1);
        assert!(matches!(
            response.outcome,
            ResponseOutcome::Failure {
                error: titan_protocol::ProtocolError {
                    code: ErrorCode::NotFound,
                    ..
                }
            }
        ));
    }

    #[test]
    fn hooks_advertise_capabilities_and_capture_does_not_mutate_revision() {
        let (mut app, mut inspector) = inspected_app();
        assert_eq!(
            inspector.capabilities().operations,
            [
                titan_protocol::Operation::Inspect,
                titan_protocol::Operation::Step
            ]
        );
        let unsupported =
            inspector.handle(&mut app, &RequestEnvelope::new("capture", Request::Capture));
        assert!(matches!(
            unsupported.outcome,
            ResponseOutcome::Failure {
                error: titan_protocol::ProtocolError {
                    code: ErrorCode::Unsupported,
                    ..
                }
            }
        ));
        inspector.register_input_handler(|app, frame, actions| {
            app.world_mut().insert_resource((frame, actions.clone()));
            Ok(())
        });
        inspector.register_capture_handler(|_| {
            Ok(titan_protocol::CaptureResult {
                width: 1,
                height: 1,
                format: "ppm".into(),
                artifact: "capture.ppm".into(),
                checksum: "abc".into(),
            })
        });
        assert!(
            inspector
                .capabilities()
                .operations
                .contains(&titan_protocol::Operation::InjectInput)
        );
        assert!(
            inspector
                .capabilities()
                .operations
                .contains(&titan_protocol::Operation::Capture)
        );
        let stale = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "stale",
                Request::InjectInput {
                    frame: 0,
                    actions: Default::default(),
                },
            ),
        );
        assert_eq!(stale.state_revision, 0);
        assert!(matches!(stale.outcome, ResponseOutcome::Failure { .. }));
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "input",
                Request::InjectInput {
                    frame: 3,
                    actions: [("jump".into(), titan_protocol::InputValue::Button(true))].into(),
                },
            ),
        );
        assert_eq!(response.state_revision, 1);
        assert_eq!(response.observed_frame, 0);
        assert!(matches!(
            response.outcome,
            ResponseOutcome::Success {
                response: Response::Applied { applied_frame: 3 }
            }
        ));
        let response =
            inspector.handle(&mut app, &RequestEnvelope::new("capture", Request::Capture));
        assert_eq!(response.state_revision, 1);
        assert!(matches!(
            response.outcome,
            ResponseOutcome::Success {
                response: Response::Capture(_)
            }
        ));
    }

    #[test]
    fn deferred_command_failures_are_structured_and_do_not_advance_revision() {
        let (mut app, mut inspector) = inspected_app();
        inspector
            .register_command::<MoveArgs>(metadata("remove_twice"), |app, _| {
                let entity = app.world().entities().next().unwrap();
                let mut commands = app.world_mut().commands();
                commands.despawn(entity);
                commands.despawn(entity);
                Ok(())
            })
            .unwrap();
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "bad",
                Request::Invoke {
                    name: "remove_twice".into(),
                    arguments: [("amount".into(), 1.into())].into(),
                },
            ),
        );
        assert_eq!(response.state_revision, 0);
        let ResponseOutcome::Failure { error } = response.outcome else {
            panic!("expected failure")
        };
        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(error.details["deferred_errors"][0]["operation"], "despawn");
        assert_eq!(app.world().entity_count(), 0);
        assert!(app.take_deferred_errors().is_empty());
    }
    #[test]
    fn failed_handlers_drain_their_deferred_writes() {
        let (mut app, mut inspector) = inspected_app();
        inspector
            .register_command::<MoveArgs>(metadata("reject"), |app, _| {
                app.world_mut().commands().spawn_with(Position(20));
                Err(titan_protocol::ProtocolError::new(
                    ErrorCode::InvalidValue,
                    "rejected after queueing",
                ))
            })
            .unwrap();
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "reject",
                Request::Invoke {
                    name: "reject".into(),
                    arguments: [("amount".into(), 1.into())].into(),
                },
            ),
        );
        assert!(matches!(response.outcome, ResponseOutcome::Failure { .. }));
        assert_eq!(response.state_revision, 0);
        assert_eq!(app.world().entity_count(), 2);
        assert!(app.apply_deferred().is_ok());
        assert_eq!(app.world().entity_count(), 2);
    }

    #[test]
    fn failed_step_reports_partial_progress_without_success_revision() {
        let (mut app, mut inspector) = inspected_app();
        app.add_systems(FixedUpdate, |world: &mut crate::World| {
            let entity = world.entities().next().unwrap();
            let mut commands = world.commands();
            commands.despawn(entity);
            commands.despawn(entity);
        });
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new("step", Request::Step { frames: 3 }),
        );
        assert_eq!(response.observed_frame, 1);
        assert_eq!(response.state_revision, 0);
        assert!(matches!(
            response.outcome,
            ResponseOutcome::Failure {
                error: titan_protocol::ProtocolError {
                    code: ErrorCode::Internal,
                    ..
                }
            }
        ));
    }
    #[test]
    fn zero_page_limit_is_rejected_instead_of_repeating_cursor() {
        let (mut app, mut inspector) = inspected_app();
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "page",
                Request::Entities {
                    query: EntityQuery::default(),
                    page: PageRequest {
                        cursor: None,
                        limit: 0,
                    },
                },
            ),
        );
        assert_eq!(response.state_revision, 0);
        assert!(matches!(
            response.outcome,
            ResponseOutcome::Failure {
                error: titan_protocol::ProtocolError {
                    code: ErrorCode::InvalidValue,
                    ..
                }
            }
        ));
    }

    #[test]
    fn input_hooks_flush_deferred_failures_before_reporting_success() {
        let (mut app, mut inspector) = inspected_app();
        inspector.register_input_handler(|app, _, _| {
            let entity = app.world().entities().next().unwrap();
            let mut commands = app.world_mut().commands();
            commands.despawn(entity);
            commands.despawn(entity);
            Ok(())
        });
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "input",
                Request::InjectInput {
                    frame: 1,
                    actions: Default::default(),
                },
            ),
        );
        assert_eq!(response.state_revision, 0);
        let ResponseOutcome::Failure { error } = response.outcome else {
            panic!("expected deferred failure")
        };
        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(error.details["deferred_errors"][0]["operation"], "despawn");
        assert_eq!(app.world().entity_count(), 0);
        assert!(app.apply_deferred().is_ok());
    }

    #[test]
    fn rejected_input_hooks_drain_deferred_writes() {
        let (mut app, mut inspector) = inspected_app();
        inspector.register_input_handler(|app, _, _| {
            app.world_mut().commands().spawn_with(Position(20));
            Err(titan_protocol::ProtocolError::new(
                ErrorCode::InvalidValue,
                "rejected after queueing",
            ))
        });
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "input",
                Request::InjectInput {
                    frame: 1,
                    actions: Default::default(),
                },
            ),
        );
        assert_eq!(response.state_revision, 0);
        assert!(matches!(
            response.outcome,
            ResponseOutcome::Failure {
                error: titan_protocol::ProtocolError {
                    code: ErrorCode::InvalidValue,
                    ..
                }
            }
        ));
        assert_eq!(app.world().entity_count(), 2);
        assert!(app.apply_deferred().is_ok());
        assert_eq!(app.world().entity_count(), 2);
    }

    #[test]
    fn input_hooks_do_not_run_with_outstanding_deferred_errors() {
        let (mut app, mut inspector) = inspected_app();
        let entity = app.world().entities().next().unwrap();
        let mut commands = app.world_mut().commands();
        commands.despawn(entity);
        commands.despawn(entity);
        inspector
            .register_input_handler(|_, _, _| panic!("hook ran before old failure was reported"));
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "input",
                Request::InjectInput {
                    frame: 1,
                    actions: Default::default(),
                },
            ),
        );
        assert_eq!(response.state_revision, 0);
        assert!(matches!(
            response.outcome,
            ResponseOutcome::Failure {
                error: titan_protocol::ProtocolError {
                    code: ErrorCode::Internal,
                    ..
                }
            }
        ));
    }
    #[test]
    fn missing_typed_resources_are_structured_protocol_failures() {
        struct Missing;
        let (mut app, mut inspector) = inspected_app();
        app.add_systems(FixedUpdate, |_: crate::Res<Missing>| {
            panic!("missing resource system ran")
        });
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new("missing-resource", Request::Step { frames: 3 }),
        );
        assert_eq!(response.observed_frame, 1);
        assert_eq!(response.state_revision, 0);
        let ResponseOutcome::Failure { error } = response.outcome else {
            panic!("expected system failure")
        };
        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(
            error.details["system_errors"][0]["kind"],
            "missing_resource"
        );
        assert!(
            error.details["system_errors"][0]["type_name"]
                .as_str()
                .unwrap()
                .ends_with("::Missing")
        );
    }
}
