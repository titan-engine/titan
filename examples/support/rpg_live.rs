//! RPG policy and snapshot validation around Titan's shared recording primitives.
use super::{self as game, Action, journal, snapshot};
use serde::Deserialize;
use serde_json::{Value, json};
use titan::{
    App, FixedTime, Startup, World,
    input::InputFrame,
    inspection::{Inspector, StepBudget, handle_with_policy},
    replay::{Playback, RecordedButtons, RecordingIdentity, SnapshotRecorder, SnapshotRecording},
};
use titan_protocol::{
    CommandMetadata, ErrorCode, FieldMetadata, ProtocolError, QueryMetadata, Request,
    RequestEnvelope, ResponseEnvelope, ResponseOutcome,
};

pub const MAX_RECORDING_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RECORDING_TICKS: usize = 3600;
const ACTIONS: [(Action, &str); 4] = [
    (Action::Up, "up"),
    (Action::Down, "down"),
    (Action::Left, "left"),
    (Action::Right, "right"),
];
const IDENTITY: RecordingIdentity<'static> = RecordingIdentity {
    game_seed: snapshot::GAME_SEED,
    action_schema: "rpg-buttons-v1:up,down,left,right",
    fixed_step_nanos: 16_666_667,
    max_ticks: MAX_RECORDING_TICKS,
};
#[derive(Default)]
struct Control {
    paused: bool,
    epoch: u64,
    journal_was_paused: bool,
}
struct Expected(Value);
fn invalid(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidValue, message)
}
fn frame(world: &World) -> u64 {
    world.resource::<FixedTime>().map_or(0, |t| t.tick())
}
fn reset_input(app: &mut App) {
    app.world_mut()
        .insert_resource(game::ScheduledInput::default());
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.world_mut().resource_mut::<Control>().unwrap().epoch += 1;
}
pub fn begin_recording(world: &mut World) {
    let snapshot = snapshot::export_world(world).expect("valid RPG initial snapshot");
    world.remove_resource::<Playback>();
    world.insert_resource(SnapshotRecorder::new(
        snapshot.clone(),
        frame(world),
        MAX_RECORDING_TICKS,
    ));
    world.insert_resource(Expected(snapshot));
    if world.resource::<Control>().is_none() {
        world.insert_resource(Control {
            paused: true,
            epoch: 0,
            journal_was_paused: true,
        });
    }
}
pub fn record_consumed(world: &mut World) {
    if let Some(playback) = world.resource_mut::<Playback>() {
        if let Some(input) = playback
            .next_frame()
            .map(|f| f.decode(&ACTIONS).expect("validated frame"))
        {
            world.insert_resource(input);
        }
        return;
    }
    let changed = snapshot::export_world(world).ok().as_ref()
        != Some(&world.resource::<Expected>().unwrap().0);
    let captured =
        RecordedButtons::capture(world.resource::<InputFrame<Action>>().unwrap(), &ACTIONS);
    let recorder = world.resource_mut::<SnapshotRecorder>().unwrap();
    if changed {
        recorder.invalidate("gameplay changed outside consumed input");
    }
    match captured {
        Ok(input) => recorder.push(input),
        Err(error) => recorder.invalidate(error),
    }
}
fn comparable(world: &World) -> Value {
    let quest = world.resource::<game::QuestState>().unwrap();
    json!({"collected_shards":quest.collected_shards,"shrine_active":quest.shrine_active})
}
fn checksum(world: &World) -> Result<String, ProtocolError> {
    Ok(format!(
        "{:016x}",
        game::image_checksum(&game::render_replay_image(world)?)
    ))
}
fn finish_playback(world: &mut World) {
    let Some(playback) = world.resource::<Playback>() else {
        return;
    };
    if !playback.complete() || playback.verified().is_some() {
        return;
    }
    let result = if snapshot::export_world(world).ok().as_ref()
        == Some(playback.expected_snapshot())
        && comparable(world) == playback.recording().final_state
        && checksum(world).ok().as_ref() == Some(&playback.recording().final_checksum)
    {
        Ok(())
    } else {
        Err("RPG final snapshot, state or pixels mismatch".into())
    };
    world
        .resource_mut::<Playback>()
        .unwrap()
        .finish(result)
        .expect("first EOF verification");
    journal::cancel(world);
    let control = world.resource_mut::<Control>().unwrap();
    control.paused = true;
    control.epoch += 1;
}
pub fn finish_tick(world: &mut World) {
    if let Ok(snapshot) = snapshot::export_world(world) {
        world.insert_resource(Expected(snapshot));
    }
    finish_playback(world);
}
fn replay_status(world: &World) -> Value {
    world
        .resource::<Playback>()
        .map_or_else(Playback::inactive_status, Playback::status)
}
pub fn recording(app: &App) -> Result<Value, ProtocolError> {
    if let Some(playback) = app.world().resource::<Playback>() {
        return Ok(serde_json::to_value(playback.recording()).unwrap());
    }
    let snapshot = snapshot::export(app)?;
    let mut recording = app
        .world()
        .resource::<SnapshotRecorder>()
        .unwrap()
        .export(
            IDENTITY,
            comparable(app.world()),
            Some(snapshot.clone()),
            checksum(app.world())?,
        )
        .map_err(invalid)?;
    if snapshot != app.world().resource::<Expected>().unwrap().0 {
        recording.invalid_reason = Some("gameplay changed outside consumed input".into());
    }
    Ok(serde_json::to_value(recording).unwrap())
}
pub fn state(app: &App) -> Value {
    let mut value: Value = serde_json::from_str(&game::status(app)).unwrap();
    value["paused"] = app.world().resource::<Control>().unwrap().paused.into();
    value["replay"] = replay_status(app.world());
    value["journal"] = journal::state(app.world());
    if let Ok(save) = snapshot::export(app) {
        value["player"] = save["player"].clone();
        value["remaining_shards"] = save["shards"].as_array().unwrap().len().into();
    }
    value
}
fn parse(value: Value) -> Result<SnapshotRecording, String> {
    let r = SnapshotRecording::parse(value, IDENTITY, MAX_RECORDING_BYTES, false)?;
    for f in &r.frames {
        f.decode(&ACTIONS)?;
    }
    Ok(r)
}
fn verify_parsed(recording: &SnapshotRecording) -> Result<Value, String> {
    let mut app = game::build_game();
    app.update_schedule(Startup);
    snapshot::load(
        &mut app,
        recording.initial_snapshot.clone().ok_or("missing origin")?,
    )
    .map_err(|e| e.message)?;
    for input in &recording.frames {
        app.world_mut().insert_resource(input.decode(&ACTIONS)?);
        app.advance_fixed(1);
    }
    let save = snapshot::export(&app).map_err(|e| e.message)?;
    let actual_checksum = checksum(app.world()).map_err(|e| e.message)?;
    if Some(&save) != recording.final_snapshot.as_ref()
        || comparable(app.world()) != recording.final_state
        || actual_checksum != recording.final_checksum
    {
        return Err("RPG recording final snapshot, state or pixels mismatch".into());
    }
    Ok(
        json!({"verified":true,"ticks":recording.frames.len(),"state":comparable(app.world()),"save":save,"checksum":actual_checksum}),
    )
}
pub fn verify_recording(value: Value) -> Result<Value, String> {
    verify_parsed(&parse(value)?)
}
fn require_paused(app: &App) -> Result<(), ProtocolError> {
    if app.world().resource::<Control>().unwrap().paused {
        Ok(())
    } else {
        Err(invalid(
            "pause before changing playback or loading a snapshot",
        ))
    }
}
fn load_save(app: &mut App, value: Value) -> Result<(), ProtocolError> {
    require_paused(app)?;
    if app.world().resource::<Playback>().is_some() {
        return Err(invalid("exit replay before loading a save"));
    }
    snapshot::load(app, value)?;
    begin_recording(app.world_mut());
    reset_input(app);
    Ok(())
}
fn load_replay(app: &mut App, value: Value) -> Result<(), ProtocolError> {
    require_paused(app)?;
    let r = parse(value).map_err(invalid)?;
    verify_parsed(&r).map_err(invalid)?;
    let expected = r.final_snapshot.clone().unwrap();
    snapshot::load(app, r.initial_snapshot.clone().unwrap())?;
    app.world_mut().insert_resource(Playback::new(r, expected));
    reset_input(app);
    finish_playback(app.world_mut());
    Ok(())
}
fn restart_replay(app: &mut App) -> Result<(), ProtocolError> {
    require_paused(app)?;
    let r = app
        .world()
        .resource::<Playback>()
        .ok_or_else(|| invalid("no replay loaded"))?
        .recording()
        .clone();
    snapshot::load(app, r.initial_snapshot.clone().unwrap())?;
    let expected = r.final_snapshot.clone().unwrap();
    app.world_mut().insert_resource(Playback::new(r, expected));
    reset_input(app);
    finish_playback(app.world_mut());
    Ok(())
}
fn restart(app: &mut App) -> Result<(), ProtocolError> {
    let mut fresh = game::build_game();
    fresh.update_schedule(Startup);
    snapshot::load(app, snapshot::export(&fresh)?)?;
    begin_recording(app.world_mut());
    reset_input(app);
    Ok(())
}
fn stop_replay(app: &mut App) -> Result<(), ProtocolError> {
    require_paused(app)?;
    if app.world().resource::<Playback>().is_none() {
        return Err(invalid("no replay loaded"));
    }
    restart(app)
}
// Journal presentation is transient. Opening suspends simulation without
// changing a recording; closing restores the previous pause policy.
fn journal_transition(app: &mut App, was_open: bool) {
    let open = journal::is_open(app.world());
    if open != was_open {
        let control = app.world_mut().resource_mut::<Control>().unwrap();
        if open {
            control.journal_was_paused = control.paused;
            control.paused = true;
        } else {
            control.paused = control.journal_was_paused;
        }
        reset_input(app);
    }
    app.refresh_extracted();
}
fn journal_key(app: &mut App, key: &str) -> bool {
    let before = journal::is_open(app.world());
    let consumed = journal::key(app.world_mut(), key);
    journal_transition(app, before);
    consumed
}
fn journal_pointer(
    app: &mut App,
    point: Option<(i32, i32)>,
    pressed: bool,
    physical: bool,
) -> bool {
    let before = journal::is_open(app.world());
    let consumed = journal::pointer(app.world_mut(), point, pressed, physical);
    journal_transition(app, before);
    consumed
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalKeyArgs {
    key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalPointerArgs {
    x: Option<i32>,
    y: Option<i32>,
    pressed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoneArgs {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveArgs {
    save: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayArgs {
    recording: Value,
}
fn metadata(name: &str, arg: Option<&str>) -> CommandMetadata {
    CommandMetadata {
        name: name.into(),
        description: format!("RPG {name}; playback changes require pause."),
        arguments: arg
            .into_iter()
            .map(|arg| {
                (
                    arg.into(),
                    FieldMetadata {
                        type_name: "JSON".into(),
                        description: "Bounded game-owned data".into(),
                        writable: false,
                        minimum: None,
                        maximum: None,
                        unit: None,
                    },
                )
            })
            .collect(),
    }
}
pub fn register(inspector: &mut Inspector) {
    for (name, query) in [
        (
            "save",
            snapshot::export as fn(&App) -> Result<Value, ProtocolError>,
        ),
        ("recording", recording),
    ] {
        inspector
            .register_query(
                QueryMetadata {
                    name: name.into(),
                    description: format!("Export RPG {name}"),
                    arguments: Default::default(),
                },
                move |app, _: NoneArgs| query(app),
            )
            .unwrap();
    }
    inspector
        .register_query(
            QueryMetadata {
                name: "rpg_state".into(),
                description: "Actual quest and playback state".into(),
                arguments: Default::default(),
            },
            |app, _: NoneArgs| Ok(state(app)),
        )
        .unwrap();
}
fn register_controls(inspector: &mut Inspector) {
    let mut key_metadata = metadata("journal_key", None);
    key_metadata.description =
        "Journal edge: toggle, next, previous, activate or close. Does not advance gameplay."
            .into();
    key_metadata.arguments.insert(
        "key".into(),
        FieldMetadata {
            type_name: "String".into(),
            description: "toggle | next | previous | activate | close".into(),
            writable: false,
            minimum: None,
            maximum: None,
            unit: None,
        },
    );
    inspector
        .register_command(key_metadata, |app, args: JournalKeyArgs| {
            if !matches!(
                args.key.as_str(),
                "toggle" | "next" | "previous" | "activate" | "close"
            ) {
                return Err(invalid("unknown journal key"));
            }
            journal_key(app, &args.key);
            Ok(())
        })
        .unwrap();
    let mut pointer_metadata = metadata("journal_pointer", None);
    pointer_metadata.description = "Logical primary pointer sample; omit both coordinates for an outside sample. Separate from physical gestures.".into();
    for (name, ty) in [
        ("x", "Option<i32>"),
        ("y", "Option<i32>"),
        ("pressed", "bool"),
    ] {
        pointer_metadata.arguments.insert(
            name.into(),
            FieldMetadata {
                type_name: ty.into(),
                description: "Logical framebuffer pointer sample".into(),
                writable: false,
                minimum: None,
                maximum: None,
                unit: None,
            },
        );
    }
    inspector
        .register_command(pointer_metadata, |app, args: JournalPointerArgs| {
            if args.x.is_some() != args.y.is_some() {
                return Err(invalid("provide both pointer coordinates or neither"));
            }
            journal_pointer(app, args.x.zip(args.y), args.pressed, false);
            Ok(())
        })
        .unwrap();
    inspector
        .register_command(
            metadata("load_save", Some("save")),
            |app, args: SaveArgs| load_save(app, args.save),
        )
        .unwrap();
    inspector
        .register_command(
            metadata("load_replay", Some("recording")),
            |app, args: ReplayArgs| load_replay(app, args.recording),
        )
        .unwrap();
    for (name, command) in [
        (
            "restart",
            restart as fn(&mut App) -> Result<(), ProtocolError>,
        ),
        ("restart_replay", restart_replay),
        ("stop_replay", stop_replay),
    ] {
        inspector
            .register_command(metadata(name, None), move |app, _: NoneArgs| command(app))
            .unwrap();
    }
    for (name, paused) in [("pause", true), ("resume", false)] {
        inspector
            .register_command(metadata(name, None), move |app, _: NoneArgs| {
                set_paused(app, paused);
                Ok(())
            })
            .unwrap();
    }
}
fn set_paused(app: &mut App, paused: bool) {
    if !paused
        && app
            .world()
            .resource::<Playback>()
            .is_some_and(Playback::complete)
    {
        return;
    }

    if journal::is_open(app.world()) {
        // Explicit host/inspector pause overrides the policy restored on close.
        app.world_mut()
            .resource_mut::<Control>()
            .unwrap()
            .journal_was_paused = paused;
        journal::cancel(app.world_mut());
        reset_input(app);
        app.refresh_extracted();
        return;
    }
    journal::cancel(app.world_mut());
    if app.world().resource::<Control>().unwrap().paused != paused {
        app.world_mut().resource_mut::<Control>().unwrap().paused = paused;
        reset_input(app);
    }
}

pub struct RpgSession {
    app: App,
    inspector: Inspector,
    input: game::InteractiveInput,
    enable_control: bool,
}
impl RpgSession {
    pub fn new(app: App, mut inspector: Inspector, enable_control: bool) -> Self {
        register_controls(&mut inspector);
        Self {
            app,
            inspector,
            input: Default::default(),
            enable_control,
        }
    }
    pub fn app(&self) -> &App {
        &self.app
    }
    pub fn control_enabled(&self) -> bool {
        self.enable_control
    }
    pub fn set_control_enabled(&mut self, enabled: bool) {
        self.enable_control = enabled;
        self.inspector.set_mutation_enabled(enabled);
        self.inspector.note_external_change();
    }
    pub fn replay_reference(&mut self) {
        self.pause();
        self.restart();
        game::replay(&mut self.app, &game::recorded_walk());
        self.inspector.note_external_change();
    }
    pub fn inspector(&self) -> &Inspector {
        &self.inspector
    }
    pub fn paused(&self) -> bool {
        self.app.world().resource::<Control>().unwrap().paused
    }
    pub fn clock_epoch(&self) -> u64 {
        self.app.world().resource::<Control>().unwrap().epoch
    }
    pub fn replay_active(&self) -> bool {
        self.app.world().resource::<Playback>().is_some()
    }
    pub fn replay_status(&self) -> Value {
        replay_status(self.app.world())
    }
    pub fn journal_open(&self) -> bool {
        journal::is_open(self.app.world())
    }
    pub fn journal_key(&mut self, key: &str) -> bool {
        let epoch = self.clock_epoch();
        let consumed = journal_key(&mut self.app, key);
        if epoch != self.clock_epoch() {
            self.clear_input();
        }
        if consumed {
            self.inspector.note_external_change();
        }
        self.inspector.set_controlled(self.paused());
        consumed
    }
    pub fn journal_pointer(&mut self, point: Option<(i32, i32)>, pressed: bool) -> bool {
        let epoch = self.clock_epoch();
        let consumed = journal_pointer(&mut self.app, point, pressed, true);
        if epoch != self.clock_epoch() {
            self.clear_input();
        }
        if consumed {
            self.inspector.note_external_change();
        }
        self.inspector.set_controlled(self.paused());
        consumed
    }
    pub fn cancel_journal_input(&mut self) {
        journal::cancel(self.app.world_mut());
        reset_input(&mut self.app);
        self.clear_input();
        self.app.refresh_extracted();
        self.inspector.note_external_change();
    }
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.app
            .world_mut()
            .insert_resource(InputFrame::<Action>::default());
    }
    pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), String> {
        if self.replay_active() || self.journal_open() || self.paused() {
            return Ok(());
        }
        self.input.set_action(name, pressed)
    }
    pub fn cancel_action(&mut self, name: &str) -> Result<(), String> {
        self.input.cancel_action(name)
    }
    pub fn pause(&mut self) {
        if !self.paused() {
            self.inspector.note_external_change();
        }
        set_paused(&mut self.app, true);
        self.clear_input();
        self.inspector.set_controlled(true);
    }
    pub fn resume(&mut self) {
        let before = self.paused();
        set_paused(&mut self.app, false);
        if before != self.paused() {
            self.inspector.note_external_change();
        }
        self.clear_input();
        self.inspector.set_controlled(self.paused());
    }
    pub fn restart(&mut self) {
        restart(&mut self.app).expect("valid fresh RPG");
        self.clear_input();
        self.inspector.note_external_change();
    }
    pub fn load_replay(&mut self, value: Value) -> Result<(), ProtocolError> {
        load_replay(&mut self.app, value)?;
        self.clear_input();
        self.inspector.note_external_change();
        Ok(())
    }
    pub fn restart_replay(&mut self) -> Result<(), ProtocolError> {
        restart_replay(&mut self.app)?;
        self.clear_input();
        self.inspector.note_external_change();
        Ok(())
    }
    pub fn stop_replay(&mut self) -> Result<(), ProtocolError> {
        stop_replay(&mut self.app)?;
        self.clear_input();
        self.inspector.note_external_change();
        Ok(())
    }
    pub fn step_replay(&mut self) -> Result<(), ProtocolError> {
        require_paused(&self.app)?;
        if self.journal_open() {
            return Err(invalid("close the journal before stepping"));
        }
        if self
            .app
            .world()
            .resource::<Playback>()
            .is_none_or(Playback::complete)
        {
            return Err(invalid("no remaining replay frames"));
        }
        self.app.advance_fixed(1);
        self.inspector.note_external_change();
        Ok(())
    }
    pub fn tick(&mut self) {
        if !self.paused() {
            self.input.tick(&mut self.app);
            self.inspector.note_external_change();
        }
    }
    pub fn handle(&mut self, request: &RequestEnvelope) -> ResponseEnvelope {
        self.inspector.set_controlled(self.paused());
        self.inspector.set_step_budget(StepBudget {
            max_frames: self
                .app
                .world()
                .resource::<Playback>()
                .map_or(StepBudget::DEFAULT.max_frames, |p| p.remaining() as u64),
            ..StepBudget::DEFAULT
        });
        let modal_allowed = !self.journal_open()
            || !matches!(
                &request.request,
                Request::Step { .. } | Request::InjectInput { .. } | Request::SetField { .. }
            );
        let allowed = !self.replay_active()
            || match &request.request {
                Request::SetField { .. } | Request::InjectInput { .. } => false,
                Request::Invoke { name, .. } => matches!(
                    name.as_str(),
                    "pause"
                        | "resume"
                        | "restart"
                        | "load_replay"
                        | "restart_replay"
                        | "stop_replay"
                        | "journal_key"
                        | "journal_pointer"
                ),
                _ => true,
            };
        let epoch = self.clock_epoch();
        let response = handle_with_policy(
            &mut self.app,
            &mut self.inspector,
            self.enable_control && allowed && modal_allowed,
            request,
        );
        if self.clock_epoch() != epoch {
            self.clear_input();
        }
        if matches!(response.outcome, ResponseOutcome::Success { .. })
            && (matches!(&request.request, Request::SetField { .. })
                || matches!(&request.request,Request::Invoke{name,..} if name=="spawn_shard"))
        {
            self.app
                .world_mut()
                .resource_mut::<SnapshotRecorder>()
                .unwrap()
                .invalidate("gameplay changed outside consumed input");
        }
        if matches!(response.outcome, ResponseOutcome::Success { .. }) {
            self.app.refresh_extracted();
        }
        self.inspector.set_controlled(self.paused());
        response
    }
    pub fn handle_json(&mut self, json: &str) -> String {
        match serde_json::from_str(json) {
            Ok(request) => serde_json::to_string(&self.handle(&request)).unwrap(),
            Err(_) => titan::inspection::handle_json_with_policy(
                &mut self.app,
                &mut self.inspector,
                self.enable_control,
                json,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> RpgSession {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        let inspector = game::inspector_with_capture(
            titan::inspection::InspectionConfig::controlled("test", "rpg"),
            |_| Err(invalid("capture unused")),
        );
        RpgSession::new(app, inspector, true)
    }
    #[test]
    fn paused_spawn_refreshes_the_presented_frame_and_invalidates_recording() {
        let mut session = session();
        let response = session.handle(&RequestEnvelope::new(
            "spawn",
            Request::Invoke {
                name: "spawn_shard".into(),
                arguments: [("x".into(), 7.into()), ("y".into(), 6.into())].into(),
            },
        ));
        assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
        let rendered = titan::render::SoftwareRenderer::render(
            session
                .app()
                .extracted::<titan::render::RenderFrame>()
                .unwrap(),
            session
                .app()
                .world()
                .resource::<titan::render::ImageAssets>()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            rendered.pixels(),
            game::render_image(session.app().world()).unwrap().pixels()
        );
        assert!(recording(session.app()).unwrap()["invalid_reason"].is_string());
    }
    #[test]
    fn snapshot_replay_isolated_pause_restart_and_eof() {
        let mut session = session();
        game::replay(&mut session.app, &game::recorded_walk());
        let artifact = recording(session.app()).unwrap();
        assert!(
            verify_recording(artifact.clone()).unwrap()["verified"]
                .as_bool()
                .unwrap()
        );
        session.load_replay(artifact).unwrap();
        session.set_action("left", true).unwrap();
        session.step_replay().unwrap();
        assert_eq!(session.replay_status()["position"], 1);
        session.restart_replay().unwrap();
        assert_eq!(session.replay_status()["position"], 0);
        let epoch = session.clock_epoch();
        session.resume();
        for _ in 0..20 {
            session.tick();
        }
        assert!(session.paused());
        assert!(session.clock_epoch() > epoch + 1);
        assert_eq!(session.replay_status()["verified"], true);
        assert_eq!(session.replay_status()["position"], 11);
        let end_frame = frame(session.app().world());
        session.journal_key("toggle");
        session.resume();
        session.journal_key("close");
        assert!(session.paused());
        session.resume();
        session.tick();
        assert_eq!(frame(session.app().world()), end_frame);
        session.stop_replay().unwrap();
        assert!(!session.replay_active());
        assert_eq!(state(session.app())["collected_shards"], 0);
    }
    #[test]
    fn journal_preserves_pause_cancels_buffered_input_and_keeps_replay_canonical() {
        let mut session = session();
        session.resume();
        session.set_action("right", true).unwrap();
        let before = snapshot::export(session.app()).unwrap();
        assert!(session.journal_key("toggle"));
        assert!(session.journal_open() && session.paused());
        session.tick();
        assert_eq!(snapshot::export(session.app()).unwrap(), before);
        let canonical = checksum(session.app().world()).unwrap();
        assert_ne!(
            canonical,
            format!(
                "{:016x}",
                game::image_checksum(&game::render_image(session.app().world()).unwrap())
            )
        );
        let artifact = recording(session.app()).unwrap();
        assert_eq!(verify_recording(artifact).unwrap()["checksum"], canonical);
        session.set_action("right", true).unwrap();
        session.journal_key("close");
        assert!(!session.paused());
        session.tick();
        assert_eq!(snapshot::export(session.app()).unwrap(), before);
        session.pause();
        session.journal_key("toggle");
        session.journal_key("close");
        assert!(session.paused());
        session.resume();
        session.journal_key("toggle");
        session.pause();
        session.journal_key("close");
        assert!(
            session.paused(),
            "explicit pause while modal must survive close"
        );
    }

    #[test]
    fn journal_blocks_injected_ticks_and_load_rebuilds_transient_state() {
        let mut session = session();
        let save = snapshot::export(session.app()).unwrap();
        // Queued future movement must be cleared by the modal transition.
        let inject = RequestEnvelope::new(
            "input",
            Request::InjectInput {
                frame: 1,
                actions: [("right".into(), titan_protocol::InputValue::Button(true))].into(),
            },
        );
        assert!(matches!(
            session.handle(&inject).outcome,
            ResponseOutcome::Success { .. }
        ));
        session.journal_key("toggle");
        let tick = frame(session.app().world());
        let response = session.handle(&RequestEnvelope::new("step", Request::Step { frames: 1 }));
        assert!(matches!(response.outcome, ResponseOutcome::Failure { .. }));
        assert_eq!(frame(session.app().world()), tick);
        assert!(matches!(
            session.handle(&inject).outcome,
            ResponseOutcome::Failure { .. }
        ));
        let response = session.handle(&RequestEnvelope::new(
            "load",
            Request::Invoke {
                name: "load_save".into(),
                arguments: [("save".into(), save.clone())].into(),
            },
        ));
        assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
        assert!(!session.journal_open());
        session.handle(&RequestEnvelope::new("step", Request::Step { frames: 1 }));
        assert_eq!(snapshot::export(session.app()).unwrap(), save);
    }

    #[test]
    fn journal_pointer_sources_cannot_complete_each_others_gesture() {
        let mut session = session();
        session.journal_pointer(Some((5, 5)), true);
        session.handle(&RequestEnvelope::new(
            "pointer",
            Request::Invoke {
                name: "journal_pointer".into(),
                arguments: [
                    ("x".into(), 5.into()),
                    ("y".into(), 5.into()),
                    ("pressed".into(), false.into()),
                ]
                .into(),
            },
        ));
        assert!(!session.journal_open());
        session.cancel_journal_input();
        session.journal_pointer(Some((5, 5)), false);
        assert!(!session.journal_open());
        session.journal_pointer(Some((5, 5)), true);
        session.pause();
        session.journal_pointer(Some((5, 5)), false);
        assert!(
            !session.journal_open(),
            "pause cancels a pending opener gesture"
        );
        session.journal_pointer(Some((5, 5)), true);
        session.journal_pointer(Some((5, 5)), false);
        assert!(session.journal_open());
    }
}
