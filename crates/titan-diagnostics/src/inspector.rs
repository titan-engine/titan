//! Native safe-point integration. Diagnostic failures never replace game responses.
use crate::{
    ApiComponent, ApiSummary, DiagnosticBundle, DiagnosticLog, DiagnosticPolicy, RequestHistory,
    WrittenBundle, attach_failure_path, write_bundle,
};
use std::{path::PathBuf, time::Instant};
use titan::{App, Name, inspection::Inspector, render::Image};
use titan_protocol::{RequestEnvelope, ResponseEnvelope};

/// The response remains usable even when collecting or persisting diagnostics fails.
pub struct DiagnosticResult {
    pub response: ResponseEnvelope,
    pub written: Option<WrittenBundle>,
    pub errors: Vec<String>,
}

/// Runs an inspector at its normal exclusive safe point and retains recent requests.
/// Entity snapshots are capped at 1,000 entries; history has independent byte/count caps.
pub struct DiagnosticInspector {
    pub policy: DiagnosticPolicy,
    pub history: RequestHistory,
    root: PathBuf,
}
impl DiagnosticInspector {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            policy: DiagnosticPolicy::default(),
            history: RequestHistory::default(),
            root: root.into(),
        }
    }

    /// `collect` is called only when policy requests a bundle. It may enrich the
    /// bundle with game-specific read-only state and return an optional image.
    /// Record recoverable capture errors in `bundle.logs` and return `None`.
    pub fn handle(
        &mut self,
        inspector: &mut Inspector,
        app: &mut App,
        request: &RequestEnvelope,
        collect: impl FnOnce(&App, &mut DiagnosticBundle) -> Option<Image>,
    ) -> DiagnosticResult {
        let started = Instant::now();
        let response = inspector.handle(app, request);
        self.record_response(inspector, app, request, response, micros(started), collect)
    }

    /// Retains diagnostics for a request already executed by a session policy.
    ///
    /// Use this when the session must enforce input isolation or playback limits
    /// before delegating to its inspector. This method never executes the request
    /// again. Pass the session's current inspector and app after handling, and
    /// the elapsed request time in microseconds. Capture failures remain separate
    /// from the response, just as with [`Self::handle`].
    pub fn record_response(
        &mut self,
        inspector: &Inspector,
        app: &App,
        request: &RequestEnvelope,
        response: ResponseEnvelope,
        elapsed_us: u64,
        collect: impl FnOnce(&App, &mut DiagnosticBundle) -> Option<Image>,
    ) -> DiagnosticResult {
        let mut result = DiagnosticResult {
            response,
            written: None,
            errors: Vec::new(),
        };
        if let Err(error) = self.history.record(request, &result.response, elapsed_us) {
            result
                .errors
                .push(format!("recording diagnostic history: {error}"));
        }
        if !self.policy.should_capture(&result.response) {
            return result;
        }
        let collecting = Instant::now();
        let mut bundle = DiagnosticBundle::new(request.clone(), result.response.clone());
        bundle.history = self.history.snapshot();
        bundle.timings_us.insert("request".into(), elapsed_us);
        let world = app.world();
        let entities: Vec<_> = world.entities().take(1000).map(|entity| {
            let names = world.component_type_names(entity);
            serde_json::json!({"id": {"index": entity.index(), "generation": entity.generation()}, "name": world.get::<Name>(entity).map(Name::as_str), "components": names})
        }).collect();
        let count = world.entities().count();
        bundle.world_state = serde_json::json!({"entities": entities, "entity_count": count, "truncated": count > 1000});
        let mut components: std::collections::BTreeMap<_, _> = world
            .component_metadata()
            .iter()
            .map(|metadata| {
                (
                    metadata.type_name.to_owned(),
                    ApiComponent::from_metadata(metadata),
                )
            })
            .collect();
        for (name, fields) in inspector.component_field_metadata() {
            components
                .entry(name.clone())
                .or_insert_with(|| ApiComponent {
                    name,
                    ..Default::default()
                })
                .fields = fields;
        }
        bundle.api_summary = Some(ApiSummary::new(
            components.into_values().collect(),
            inspector.command_metadata(),
        ));
        for error in &result.errors {
            bundle.logs.push(DiagnosticLog {
                level: "warning".into(),
                message: error.clone(),
                frame: Some(result.response.observed_frame),
            });
        }
        let image = collect(app, &mut bundle);
        bundle
            .timings_us
            .insert("collection".into(), micros(collecting));
        match write_bundle(&self.root, &bundle, image.as_ref()) {
            Ok(written) => {
                attach_failure_path(&mut result.response, &written.manifest);
                result.written = Some(written);
            }
            Err(error) => result
                .errors
                .push(format!("writing diagnostic bundle: {error}")),
        }
        result
    }
}
fn micros(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}
