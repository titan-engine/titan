//! Transport-neutral inspection of a Titan [`App`](crate::App).

use std::collections::BTreeMap;

use titan_protocol::{
    Capabilities, EntityDetails, EntityId, EntityPage, EntitySummary, ErrorCode, Operation,
    ProtocolError, Request, RequestEnvelope, Response, ResponseEnvelope, RunMode, RuntimeStatus,
    SCHEMA_VERSION,
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

/// Executes typed inspection requests at a caller-controlled safe point.
///
/// A transport adapter should enqueue requests and call [`handle`](Self::handle)
/// only when it has exclusive access to the application. Transport threads
/// must never access the ECS world directly.
pub struct Inspector {
    config: InspectionConfig,
    state_revision: u64,
}

impl Inspector {
    pub const fn new(config: InspectionConfig) -> Self {
        Self {
            config,
            state_revision: 0,
        }
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
                app.advance_fixed(*frames);
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
                commands: Vec::new(),
            }),
            Request::Invoke { .. } | Request::InjectInput { .. } | Request::Capture => {
                Err(ProtocolError::new(
                    ErrorCode::Unsupported,
                    "this operation has not been registered by the game",
                ))
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
        app.add_systems(FixedUpdate, |world| {
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
}
