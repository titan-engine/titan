//! Deterministic, GPU-independent adventure. Positions are millimeters.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use titan::input::{ActionValue, InputFrame, InputRecording, InputTracker, RecordingHeader};
use titan::inspection::{InspectionConfig, Inspector};
use titan::render::three_d::*;
use titan::render::{Color, ImageAssets, RenderFrame};
use titan::replay::RecordedButtons;
use titan::ui::{BitmapFont, UiNode, UiText, append_ui, register_ui_inspection};
use titan::{App, Component, FixedTime, FixedUpdate, Name, Startup, World};
use titan_protocol::{
    CommandMetadata, ErrorCode, FieldMetadata, InputValue, ProtocolError, QueryMetadata,
};

pub const MAX_RECORDING_TICKS: usize = 4096;
pub const AXIAL_STEP: i32 = 60;
pub const DIAGONAL_STEP: i32 = 42;
pub const FIXTURE: &str = "adventure-v3";
pub mod block;
pub mod movement;
mod presentation;
pub mod puzzle;
use movement::{Movement, SOLIDS};
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
#[derive(Component)]
struct Character {
    index: usize,
}
#[derive(Component)]
struct Hud;
#[derive(Component)]
struct Visual {
    mesh: MeshHandle,
    scale: Vec3,
    height: f32,
    color: BaseColor,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Switch,
    Restart,
    Jump,
    Interact,
    Confirm,
}
pub(crate) const SCHEMA: [(Action, &str); 9] = [
    (Action::Up, "up"),
    (Action::Down, "down"),
    (Action::Left, "left"),
    (Action::Right, "right"),
    (Action::Switch, "switch"),
    (Action::Restart, "restart"),
    (Action::Jump, "jump"),
    (Action::Interact, "interact"),
    (Action::Confirm, "confirm"),
];
#[derive(Default)]
struct ScheduledInput {
    enabled: bool,
    frames: BTreeMap<u64, Vec<(Action, ActionValue)>>,
    tracker: InputTracker<Action>,
    held: BTreeSet<Action>,
    blocked: BTreeSet<Action>,
}
struct PendingSwitch;
struct PendingConfirm;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Start,
    #[default]
    Playing,
    RoomComplete,
    SliceComplete,
}
struct Session {
    room: u8,
    phase: Phase,
    recording_room: u8,
    block: block::BlockState,
    generation: u64,
    puzzle: puzzle::PuzzleState,
    tick: u64,
    recording: InputRecording<Action>,
    truncated: bool,
    active: usize,
    recovery_message_ticks: u32,
    origin: RecordingOrigin,
    blocked: BTreeSet<Action>,
    consumed: InputFrame<Action>,
    effective_tracker: InputTracker<Action>,
}
impl Session {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            room: 1,
            phase: Phase::Playing,
            recording_room: 1,
            block: block::BlockState::default(),
            puzzle: puzzle::PuzzleState::default(),
            tick: 0,
            recording: InputRecording::new(RecordingHeader::new(16_666_667, 81, 0x81)),
            truncated: false,
            active: 0,
            recovery_message_ticks: 0,
            origin: RecordingOrigin::default(),
            blocked: BTreeSet::new(),
            consumed: InputFrame::default(),
            effective_tracker: InputTracker::default(),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingOrigin {
    #[serde(default)]
    pub phase: Phase,
    pub blocked_actions: Vec<String>,
    pub recovery_message_ticks: u32,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    #[serde(default = "default_room", skip_serializing_if = "is_default_room")]
    pub room: u8,
    #[serde(default)]
    pub origin: RecordingOrigin,
    pub format_version: u32,
    pub fixture: String,
    pub frames: Vec<RecordedButtons>,
    pub truncated: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayArgs {
    recording: Recording,
}
fn is_default_room(room: &u8) -> bool {
    *room == 1
}
pub(crate) fn default_room() -> u8 {
    1
}
fn initial_position(index: usize) -> Position {
    Position {
        x: if index == 0 { 1500 } else { 3500 },
        y: 0,
        z: 6500,
    }
}
fn character_name(index: usize) -> &'static str {
    if index == 0 { "jumper" } else { "strong" }
}
pub fn build_game() -> App {
    let mut app = App::new();
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.world_mut().insert_resource(ScheduledInput::default());
    let mut session = Session::new(0);
    session.phase = Phase::Start;
    session.origin.phase = Phase::Start;
    app.world_mut().insert_resource(session);
    app.add_systems(Startup, setup);
    app.add_systems(FixedUpdate, tick);
    app.add_extractor(extract);
    app.add_extractor(extract_overlay);
    app
}
fn setup(world: &mut World) {
    let mut images = ImageAssets::default();
    let font = BitmapFont::tiny(&mut images);
    world.insert_resource(images);
    world.insert_resource(font);
    world.spawn_with((
        Name::new("ui/active-character"),
        Hud,
        UiNode::new(8, 8, 304, 5),
        UiText::new("ACTIVE: JUMPER  [Q] SWITCH  [R] RESTART")
            .with_color(Color::rgb(255, 240, 190)),
    ));
    presentation::setup(world);
    let mut assets = MeshAssets::new();
    let cube = assets.insert(Mesh::cube(1.0).unwrap()).unwrap();
    let floor = assets.insert(Mesh::floor(1.0).unwrap()).unwrap();
    // A triangular top marker makes Jumper identifiable without relying on color.
    let triangle = assets
        .insert(
            Mesh::new(
                vec![
                    Vec3::new(-0.3, 0.0, 0.25),
                    Vec3::new(0.3, 0.0, 0.25),
                    Vec3::new(0.0, 0.0, -0.35),
                ],
                vec![Vec3::new(0.0, 1.0, 0.0); 3],
                vec![0, 1, 2],
            )
            .unwrap(),
        )
        .unwrap();
    world.insert_resource(assets);
    let angle = -(14.0f32).atan2(13.0);
    world.insert_resource(
        PerspectiveCamera::new(
            Vec3::new(6.0, 14.0, 17.0),
            Quaternion::new((angle / 2.0).sin(), 0.0, 0.0, (angle / 2.0).cos()).unwrap(),
            50.0f32.to_radians(),
            16.0 / 9.0,
            0.1,
            50.0,
        )
        .unwrap(),
    );
    world.insert_resource(Lighting3d::new(Vec3::new(-1.0, 3.0, 2.0), 0.3, 0.7).unwrap());
    world.spawn_with((
        Name::new("floor"),
        Position {
            x: 6000,
            y: 0,
            z: 4000,
        },
        Visual {
            mesh: floor,
            scale: Vec3::new(12.0, 1.0, 8.0),
            height: 0.0,
            color: BaseColor::rgb(50, 68, 85),
        },
    ));
    for solid in &SOLIDS[1..] {
        if solid.name == "wall-south" {
            continue;
        }
        let wall = solid.name.starts_with("wall-");
        let height = if wall {
            300
        } else if solid.name.contains("partition") {
            1200
        } else {
            solid.max.y - solid.min.y
        };
        world.spawn_with((
            Name::new(solid.name),
            Position {
                x: (solid.min.x + solid.max.x) / 2,
                y: solid.min.y,
                z: (solid.min.z + solid.max.z) / 2,
            },
            Visual {
                mesh: cube,
                scale: Vec3::new(
                    (solid.max.x - solid.min.x) as f32 / 1000.0,
                    height as f32 / 1000.0,
                    (solid.max.z - solid.min.z) as f32 / 1000.0,
                ),
                height: height as f32 / 2000.0,
                color: if wall {
                    BaseColor::rgb(85, 105, 130)
                } else {
                    BaseColor::rgb(110, 130, 150)
                },
            },
        ));
    }
    for index in 0..2 {
        world.spawn_with((
            Name::new(character_name(index)),
            Character { index },
            initial_position(index),
            Movement::default(),
            Visual {
                mesh: cube,
                scale: Vec3::new(
                    if index == 0 { 0.35 } else { 0.4 },
                    0.9,
                    if index == 0 { 0.35 } else { 0.4 },
                ),
                height: 0.45,
                color: if index == 0 {
                    BaseColor::rgb(65, 215, 235)
                } else {
                    BaseColor::rgb(240, 155, 70)
                },
            },
        ));
    }
    world.insert_resource(Markers { cube, triangle });
    sync_hud(world);
}
struct Markers {
    cube: MeshHandle,
    triangle: MeshHandle,
}
fn reset_world(world: &mut World) {
    let ids: Vec<_> = world
        .iter::<Character>()
        .map(|(id, c)| (id, c.index))
        .collect();
    for (id, index) in ids {
        *world.get_mut::<Position>(id).unwrap() = initial_position(index);
        *world.get_mut::<Movement>(id).unwrap() = Movement::default();
    }
    let generation = world
        .resource::<Session>()
        .unwrap()
        .generation
        .checked_add(1)
        .expect("session generation exhausted");
    let room = world.resource::<Session>().unwrap().room;
    let mut session = Session::new(generation);
    session.room = room;
    session.recording_room = room;
    world.insert_resource(session);
    world.remove_resource::<PendingSwitch>();
    world.remove_resource::<PendingConfirm>();
    clear_scheduled_input(world);
    world.insert_resource(InputFrame::<Action>::default());
}
/// Reset game state and recording; the host FixedTime remains monotonic.
pub fn restart(app: &mut App) {
    app.update_schedule(Startup);
    reset_world(app.world_mut());
    crate::player::reset_input(app.world_mut());
    sync_hud(app.world_mut());
    app.refresh_extracted();
}
/// Development room selector, separate from the future slice progression UI.
pub fn select_room(app: &mut App, room: u8) -> Result<(), ProtocolError> {
    if !matches!(room, 1 | 2) {
        return Err(invalid("room must be 1 or 2"));
    }
    app.update_schedule(Startup);
    app.world_mut().resource_mut::<Session>().unwrap().room = room;
    restart(app);
    sync_room_visual(app.world_mut());
    app.refresh_extracted();
    Ok(())
}
fn sync_room_visual(world: &mut World) {
    let room = world.resource::<Session>().unwrap().room;
    let id = world
        .iter::<Name>()
        .find(|(_, n)| matches!(n.as_str(), "teaching-ledge" | "high-ledge"))
        .unwrap()
        .0;
    let ledge = room_solids(room)[5];
    *world.get_mut::<Name>(id).unwrap() = Name::new(ledge.name);
    *world.get_mut::<Position>(id).unwrap() = Position {
        x: (ledge.min.x + ledge.max.x) / 2,
        y: 0,
        z: 2000,
    };
    let visual = world.get_mut::<Visual>(id).unwrap();
    visual.scale = Vec3::new(
        (ledge.max.x - ledge.min.x) as f32 / 1000.,
        ledge.max.y as f32 / 1000.,
        2.,
    );
    visual.height = ledge.max.y as f32 / 2000.;
}
/// Trigger the visible Start, Continue or Play again action on a recorded tick.
pub fn confirm(app: &mut App) {
    app.update_schedule(Startup);
    app.world_mut().insert_resource(PendingConfirm);
    app.advance_fixed(1);
}
/// Reconstruct a sequence destination, retaining the complete canonical recording.
fn transition(world: &mut World, room: u8, input: InputFrame<Action>) {
    let old = world.remove_resource::<Session>().unwrap();
    // reset_world needs the old room identity to reconstruct destination state.
    let mut placeholder = Session::new(old.generation);
    placeholder.room = room;
    world.insert_resource(placeholder);
    reset_world(world);
    crate::player::reset_input(world);
    let session = world.resource_mut::<Session>().unwrap();
    session.recording = old.recording;
    session.recording_room = old.recording_room;
    session.origin = old.origin;
    session.truncated = old.truncated;
    session
        .blocked
        .extend(input.active_actions().map(|(a, _)| *a));
    record_input(session, input);
    sync_room_visual(world);
    sync_hud(world);
}
fn record_input(session: &mut Session, input: InputFrame<Action>) {
    if session.recording.len() < MAX_RECORDING_TICKS {
        session.recording.push(input);
    } else {
        session.truncated = true;
    }
}
pub fn room_solids(room: u8) -> [movement::Solid; 8] {
    let mut solids = SOLIDS;
    if room == 2 {
        solids[5] = movement::solid("high-ledge", (4000, 0, 1000), (7000, 2000, 3000));
    }
    solids
}
pub(crate) fn clear_scheduled_input(world: &mut World) {
    let scheduled = world.resource_mut::<ScheduledInput>().unwrap();
    scheduled.frames.clear();
    scheduled.enabled = false;
    // Clearing pending work is not a physical release. Keep source-local gates
    // across resets and pauses, including ticks supplied by another input source.
    scheduled.blocked.extend(scheduled.held.iter().copied());
}
fn tick(world: &mut World) {
    crate::player::prepare_tick(world);
    let next = world.resource::<FixedTime>().unwrap().tick() + 1;
    let scheduled = world.resource_mut::<ScheduledInput>().unwrap();
    let frame = if scheduled.enabled {
        let values = scheduled.frames.remove(&next).unwrap_or_default();
        let raw = scheduled.tracker.sample(values);
        scheduled.held = raw.active_actions().map(|(a, _)| *a).collect();
        scheduled.blocked.retain(|a| scheduled.held.contains(a));
        let mut buttons = RecordedButtons::capture(&raw, &SCHEMA).unwrap();
        for (action, name) in SCHEMA {
            if scheduled.blocked.contains(&action) {
                buttons.active.retain(|v| v != name);
                buttons.pressed.retain(|v| v != name);
                buttons.released.retain(|v| v != name);
            }
        }
        Some(buttons.decode(&SCHEMA).unwrap())
    } else {
        None
    };
    if let Some(frame) = frame {
        world.insert_resource(frame);
    }
    let mut input = world.resource::<InputFrame<Action>>().unwrap().clone();
    if world.remove_resource::<PendingSwitch>().is_some() {
        let mut buttons = RecordedButtons::capture(&input, &SCHEMA).unwrap();
        if !buttons.active.iter().any(|a| a == "switch") {
            buttons.active.push("switch".into());
        }
        if !buttons.pressed.iter().any(|a| a == "switch") {
            buttons.pressed.push("switch".into());
        }
        buttons.released.retain(|a| a != "switch");
        input = buttons.decode(&SCHEMA).unwrap();
    }
    if world.remove_resource::<PendingConfirm>().is_some() {
        let mut buttons = RecordedButtons::capture(&input, &SCHEMA).unwrap();
        if !buttons.active.iter().any(|a| a == "confirm") {
            buttons.active.push("confirm".into());
        }
        if !buttons.pressed.iter().any(|a| a == "confirm") {
            buttons.pressed.push("confirm".into());
        }
        buttons.released.retain(|a| a != "confirm");
        input = buttons.decode(&SCHEMA).unwrap();
    }
    let active: BTreeSet<_> = input.active_actions().map(|(a, _)| *a).collect();
    let reset = input.just_pressed(&Action::Restart);
    let switch = input.just_pressed(&Action::Switch);
    if reset {
        reset_world(world);
        let session = world.resource_mut::<Session>().unwrap();
        session.blocked.extend(active);
        // Preserve the reset edge in the canonical-origin recording for replay gating.
        session.recording.push(input);
        sync_hud(world);
        return;
    }
    let session = world.resource_mut::<Session>().unwrap();
    if session.phase != Phase::Playing {
        if input.just_pressed(&Action::Confirm) {
            let room = if session.phase == Phase::RoomComplete {
                2
            } else {
                1
            };
            transition(world, room, input);
            return;
        }
        // Preserve raw frames for deterministic replay, but completion freezes
        // characters, puzzle state, active selection and the room clock.
        session.consumed = InputFrame::default();
        if session.recording.len() < MAX_RECORDING_TICKS {
            session.recording.push(input);
        } else {
            session.truncated = true;
        }
        sync_hud(world);
        return;
    }
    // A fresh press also proves release/repress between fixed ticks. Preserve
    // this edge in the raw recording so replay makes the same decision.
    session
        .blocked
        .retain(|a| active.contains(a) && !input.just_pressed(a));
    if switch {
        session.active = 1 - session.active;
        session.blocked.extend(active.iter().copied());
    }
    let effective: Vec<_> = active
        .iter()
        .filter(|a| {
            !session.blocked.contains(a)
                && !matches!(a, Action::Switch | Action::Restart | Action::Confirm)
        })
        .map(|a| (*a, ActionValue::PRESSED))
        .collect();
    session.consumed = session.effective_tracker.sample(effective);
    // Physical release/repress can occur between fixed ticks. Keep its genuine
    // press edge even when the previous effective snapshot was already held.
    let mut consumed = RecordedButtons::capture(&session.consumed, &SCHEMA).unwrap();
    for (action, name) in SCHEMA {
        if input.just_pressed(&action)
            && session.consumed.is_active(&action)
            && !consumed.pressed.iter().any(|v| v == name)
        {
            consumed.pressed.push(name.into());
        }
    }
    session.consumed = consumed.decode(&SCHEMA).unwrap();
    let dx = i32::from(session.consumed.is_active(&Action::Right))
        - i32::from(session.consumed.is_active(&Action::Left));
    let dz = i32::from(session.consumed.is_active(&Action::Down))
        - i32::from(session.consumed.is_active(&Action::Up));
    let target = session.active;
    let jump = session.consumed.just_pressed(&Action::Jump);
    let push = session.consumed.just_pressed(&Action::Interact);
    let direction_count = usize::from(dx != 0) + usize::from(dz != 0);
    let room = session.room;
    session.recovery_message_ticks = session.recovery_message_ticks.saturating_sub(1);
    session.tick += 1;
    if session.recording.len() < MAX_RECORDING_TICKS {
        session.recording.push(input);
    } else {
        session.truncated = true;
    }
    let step = if dx != 0 && dz != 0 {
        DIAGONAL_STEP
    } else {
        AXIAL_STEP
    };
    // The solids are selected once before either character moves. Plate and
    // obstruction sampling below determines collision for the following tick.
    let mut solids = room_solids(room).to_vec();
    if !session.puzzle.door.open {
        solids.push(puzzle::DOOR);
    }
    let mut bodies = [(initial_position(0), Movement::default()); 2];
    for (id, c) in world.iter::<Character>() {
        bodies[c.index] = (
            *world.get::<Position>(id).unwrap(),
            *world.get::<Movement>(id).unwrap(),
        );
    }
    let session = world.resource_mut::<Session>().unwrap();
    let pushed = room == 2
        && push
        && session
            .block
            .push(target, (dx, dz, direction_count), jump, bodies, &solids);
    if room == 2 {
        solids.push(session.block.solid());
    }
    let mut ids: Vec<_> = world
        .iter::<Character>()
        .map(|(id, c)| (c.index, id))
        .collect();
    ids.sort_by_key(|(index, _)| *index);
    for (index, id) in ids {
        let mut p = *world.get::<Position>(id).unwrap();
        let mut m = *world.get::<Movement>(id).unwrap();
        let controlled = index == target && !pushed;
        movement::advance(
            &mut p,
            &mut m,
            if controlled { dx * step } else { 0 },
            if controlled { dz * step } else { 0 },
            controlled && jump,
            if index == 0 { 180 } else { 100 },
            &solids,
        );
        *world.get_mut::<Position>(id).unwrap() = p;
        *world.get_mut::<Movement>(id).unwrap() = m;
    }
    if world
        .iter::<Character>()
        .any(|(id, _)| world.get::<Position>(id).unwrap().y < -2000)
    {
        reset_world(world);
        let session = world.resource_mut::<Session>().unwrap();
        session.blocked.extend(active);
        session.recovery_message_ticks = 120;
        session.origin = RecordingOrigin {
            phase: Phase::Playing,
            blocked_actions: session
                .blocked
                .iter()
                .map(|a| {
                    SCHEMA
                        .iter()
                        .find(|(known, _)| known == a)
                        .unwrap()
                        .1
                        .into()
                })
                .collect(),
            recovery_message_ticks: 120,
        };
    }
    let mut bodies = [(initial_position(0), Movement::default()); 2];
    for (id, c) in world.iter::<Character>() {
        bodies[c.index] = (
            *world.get::<Position>(id).unwrap(),
            *world.get::<Movement>(id).unwrap(),
        );
    }
    world
        .resource_mut::<Session>()
        .unwrap()
        .puzzle
        .sample_room(bodies, room);
    let session = world.resource_mut::<Session>().unwrap();
    if session.puzzle.complete {
        session.phase = if session.room == 1 {
            Phase::RoomComplete
        } else {
            Phase::SliceComplete
        };
    }
    sync_hud(world);
}
fn sync_hud(world: &mut World) {
    presentation::sync(world);
    let active = world.resource::<Session>().unwrap().active;
    let id = world.iter::<Hud>().next().unwrap().0;
    world.get_mut::<UiText>(id).unwrap().text = format!(
        "ACTIVE: {} [SPACE] JUMP [Q] SWITCH [R] RESET",
        character_name(active).to_uppercase()
    );
    if world.resource::<Session>().unwrap().recovery_message_ticks > 0 {
        world.get_mut::<UiText>(id).unwrap().text =
            "FELL - ROOM RESET  [SPACE] JUMP  [Q] SWITCH".into();
    }
}
pub fn extract_overlay(world: &World) -> RenderFrame {
    let mut frame = RenderFrame::new(320, 180, Color::rgba(0, 0, 0, 0));
    presentation::append_overlay(world, &mut frame);
    append_ui(world, &mut frame);
    frame
}
pub fn extract(world: &World) -> Result<RenderFrame3d, Frame3dError> {
    let mut draws = Vec::new();
    let markers = world.resource::<Markers>().unwrap();
    for (id, visual) in world.iter::<Visual>() {
        let p = world.get::<Position>(id).unwrap();
        let base = (u64::from(id.index()) << 32) | u64::from(id.generation());
        let x = p.x as f32 / 1000.0;
        let z = p.z as f32 / 1000.0;
        draws.push(Draw3d {
            mesh: visual.mesh,
            transform: Transform3d::new(
                Vec3::new(x, p.y as f32 / 1000.0 + visual.height, z),
                Quaternion::IDENTITY,
                visual.scale,
            )
            .unwrap(),
            color: visual.color,
            order: base,
        });
        if let Some(character) = world.get::<Character>(id) {
            draws.push(Draw3d {
                mesh: if character.index == 0 {
                    markers.triangle
                } else {
                    markers.cube
                },
                transform: Transform3d::new(
                    Vec3::new(x, p.y as f32 / 1000.0 + 1.05, z),
                    Quaternion::IDENTITY,
                    if character.index == 0 {
                        Vec3::ONE
                    } else {
                        Vec3::new(0.45, 0.04, 0.45)
                    },
                )
                .unwrap(),
                color: BaseColor::rgb(245, 245, 225),
                order: base + 1,
            });
            if character.index == world.resource::<Session>().unwrap().active {
                for (i, (ox, oz, sx, sz)) in [
                    (-0.48, 0.0, 0.07, 1.03),
                    (0.48, 0.0, 0.07, 1.03),
                    (0.0, -0.48, 0.89, 0.07),
                    (0.0, 0.48, 0.89, 0.07),
                ]
                .into_iter()
                .enumerate()
                {
                    draws.push(Draw3d {
                        mesh: markers.cube,
                        transform: Transform3d::new(
                            Vec3::new(x + ox, p.y as f32 / 1000.0 + 0.045, z + oz),
                            Quaternion::IDENTITY,
                            Vec3::new(sx, 0.04, sz),
                        )
                        .unwrap(),
                        color: BaseColor::rgb(255, 230, 90),
                        order: base + 2 + i as u64,
                    });
                }
            }
        }
    }
    presentation::append(world, &mut draws);
    RenderFrame3d::new(
        *world.resource::<PerspectiveCamera>().unwrap(),
        *world.resource::<Lighting3d>().unwrap(),
        world.resource::<MeshAssets>().unwrap(),
        draws,
        Frame3dLimits {
            max_draws: 96,
            max_geometry_bytes: 1024 * 1024,
        },
    )
}
fn invalid(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidValue, message)
}
pub fn recording(app: &App) -> Result<Recording, ProtocolError> {
    let s = app.world().resource::<Session>().unwrap();
    Ok(Recording {
        room: s.recording_room,
        origin: s.origin.clone(),
        format_version: 1,
        fixture: FIXTURE.into(),
        frames: s
            .recording
            .frames()
            .iter()
            .map(|f| RecordedButtons::capture(f, &SCHEMA).expect("digital schema"))
            .collect(),
        truncated: s.truncated,
    })
}
pub fn replay(app: &mut App, recording: Recording) -> Result<(), ProtocolError> {
    if recording.format_version != 1
        || !matches!(recording.room, 1 | 2)
        || recording.fixture != FIXTURE
        || recording.truncated
        || recording.frames.len() > MAX_RECORDING_TICKS
    {
        return Err(invalid("unsupported, truncated or oversized recording"));
    }
    let frames = recording
        .frames
        .iter()
        .map(|f| {
            f.decode(&SCHEMA)
                .map_err(|_| invalid("invalid recorded buttons"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_origin(&recording.origin)?;
    select_room(app, recording.room)?;
    apply_origin(app, &recording.origin);
    for frame in frames {
        app.world_mut().insert_resource(frame);
        app.advance_fixed(1);
    }
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    Ok(())
}
pub fn status(app: &App) -> serde_json::Value {
    let world = app.world();
    let s = world.resource::<Session>().unwrap();
    let characters: BTreeMap<_, _> = world
        .iter::<Character>()
        .map(|(id, c)| {
            let p = world.get::<Position>(id).unwrap();
            let m = world.get::<Movement>(id).unwrap();
            (character_name(c.index), serde_json::json!({"x":p.x,"y":p.y,"z":p.z,"velocity_y":m.velocity_y,"grounded":m.grounded,"support":m.support,"collisions":m.collisions}))
        })
        .collect();
    serde_json::json!({"phase":s.phase,"room":s.room,"block":if s.room == 2 {Some(&s.block)} else {None},"block_geometry":if s.room == 2 {Some(s.block.solid())} else {None},"puzzle":s.puzzle,"puzzle_geometry":{"plates":puzzle::plates(s.room),"door":puzzle::DOOR,"exit":puzzle::EXIT},"solids":room_solids(s.room),"recovery_message_ticks":s.recovery_message_ticks,"fixture":FIXTURE,"frame":world.resource::<FixedTime>().unwrap().tick(),"session_generation":s.generation,"session_tick":s.tick,"characters":characters,"active_character":character_name(s.active),"consumed_input":RecordedButtons::capture(&s.consumed,&SCHEMA).unwrap(),"blocked_actions":s.blocked.iter().map(|a|SCHEMA.iter().find(|(v,_)|v==a).unwrap().1).collect::<Vec<_>>(),"recorded_ticks":s.recording.len(),"recording_valid":true,"recording_truncated":s.truncated,"pending_inputs":world.resource::<ScheduledInput>().unwrap().frames.len()})
}
fn field(type_name: &str, description: &str) -> FieldMetadata {
    FieldMetadata {
        type_name: type_name.into(),
        description: description.into(),
        writable: false,
        minimum: None,
        maximum: None,
        unit: None,
    }
}
pub fn configured_inspector(config: InspectionConfig) -> Inspector {
    let mut inspector = Inspector::new(config);
    register_ui_inspection(&mut inspector).unwrap();
    inspector
        .register_read_only_field::<Position, _>(
            "x",
            field("i32", "X coordinate in millimeters"),
            |p| p.x,
        )
        .unwrap();
    inspector
        .register_read_only_field::<Position, _>(
            "z",
            field("i32", "Z coordinate in millimeters"),
            |p| p.z,
        )
        .unwrap();
    inspector
        .register_read_only_field::<Position, _>(
            "y",
            field("i32", "Foot height in millimeters"),
            |p| p.y,
        )
        .unwrap();
    inspector
        .register_query(
            QueryMetadata {
                name: "state".into(),
                description: "Bounded room state".into(),
                arguments: BTreeMap::new(),
            },
            |app, _: Empty| Ok(status(app)),
        )
        .unwrap();
    inspector
        .register_query(
            QueryMetadata {
                name: "recording".into(),
                description: "Bounded canonical-origin digital recording".into(),
                arguments: BTreeMap::new(),
            },
            |app, _: Empty| Ok(serde_json::to_value(recording(app)?).unwrap()),
        )
        .unwrap();
    inspector
        .register_command(
            CommandMetadata {
                name: "restart".into(),
                description: "Restore fixture and clear input/replay; increment session generation"
                    .into(),
                arguments: BTreeMap::new(),
            },
            |app, _: Empty| {
                restart(app);
                Ok(())
            },
        )
        .unwrap();
    inspector
        .register_command(
            CommandMetadata {
                name: "confirm".into(),
                description: "Start, continue or play again on one recorded fixed tick".into(),
                arguments: BTreeMap::new(),
            },
            |app, _: Empty| {
                confirm(app);
                Ok(())
            },
        )
        .unwrap();
    inspector
        .register_command(
            CommandMetadata {
                name: "replay".into(),
                description: "Validate and replay at most 4096 recorded ticks from fixture origin"
                    .into(),
                arguments: BTreeMap::from([(
                    "recording".into(),
                    field("object", "Adventure recording object"),
                )]),
            },
            |app, args: ReplayArgs| replay(app, args.recording),
        )
        .unwrap();
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RoomArgs {
        room: u8,
    }
    inspector
        .register_command(
            CommandMetadata {
                name: "select_room".into(),
                description: "Development selector: reconstruct room 1 or 2".into(),
                arguments: BTreeMap::from([("room".into(), field("u8", "Room 1 or 2"))]),
            },
            |app, args: RoomArgs| select_room(app, args.room),
        )
        .unwrap();
    inspector.register_command(CommandMetadata {name:"switch".into(),description:"Switch active character on one recorded fixed tick; suppress held input until release".into(),arguments:BTreeMap::new()}, |app, _:Empty| {
        app.world_mut().insert_resource(PendingSwitch);
        app.advance_fixed(1);
        Ok(())
    }).unwrap();
    inspector.register_input_handler(|app, frame, actions| {
        let now = app.world().resource::<FixedTime>().unwrap().tick();
        if frame <= now || frame - now > MAX_RECORDING_TICKS as u64 {
            return Err(invalid("input frame must be within the next 4096 ticks"));
        }
        let values = actions
            .iter()
            .map(|(name, value)| {
                let action = SCHEMA
                    .iter()
                    .find(|(_, known)| name == known)
                    .map(|(action, _)| *action)
                    .ok_or_else(|| invalid("unknown game action"))?;
                match value {
                    InputValue::Button(pressed) => Ok((
                        action,
                        if *pressed {
                            ActionValue::PRESSED
                        } else {
                            ActionValue::RELEASED
                        },
                    )),
                    InputValue::Axis(_) => Err(invalid("game requires button input")),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let scheduled = app.world_mut().resource_mut::<ScheduledInput>().unwrap();
        scheduled.enabled = true;
        scheduled.frames.insert(frame, values);
        Ok(())
    });
    inspector
}
#[cfg(test)]
mod tests;

/// Controlled invalid-origin fixture for cross-target defensive recovery acceptance.
/// It is not exposed as a player command or mutable inspection field.
#[cfg(any(test, feature = "movement-acceptance"))]
pub fn build_recovery_fixture() -> App {
    let mut app = build_game();
    app.world_mut().resource_mut::<Session>().unwrap().phase = Phase::Playing;
    app.world_mut()
        .resource_mut::<Session>()
        .unwrap()
        .origin
        .phase = Phase::Playing;
    app.update_schedule(Startup);
    let id = app
        .world()
        .iter::<Character>()
        .find(|(_, c)| c.index == 1)
        .unwrap()
        .0;
    app.world_mut().get_mut::<Position>(id).unwrap().y = -2000;
    *app.world_mut().get_mut::<Movement>(id).unwrap() = Movement {
        velocity_y: -10,
        grounded: false,
        support: None,
        collisions: Default::default(),
    };
    app
}

#[cfg(any(test, feature = "movement-acceptance"))]
pub(crate) fn fixture_set_character(
    app: &mut App,
    index: usize,
    position: Position,
    velocity_y: i32,
    grounded: bool,
) {
    app.update_schedule(Startup);
    let id = app
        .world()
        .iter::<Character>()
        .find(|(_, c)| c.index == index)
        .unwrap()
        .0;
    *app.world_mut().get_mut::<Position>(id).unwrap() = position;
    *app.world_mut().get_mut::<Movement>(id).unwrap() = Movement {
        velocity_y,
        grounded,
        support: None,
        collisions: Default::default(),
    };
}

pub(crate) fn validate_origin(origin: &RecordingOrigin) -> Result<(), ProtocolError> {
    if !matches!(origin.phase, Phase::Start | Phase::Playing)
        || (origin.phase == Phase::Start && origin.recovery_message_ticks != 0)
        || (origin.recovery_message_ticks != 0 && origin.recovery_message_ticks != 120)
    {
        return Err(invalid("invalid recovery recording origin"));
    }
    let names: BTreeSet<_> = origin.blocked_actions.iter().collect();
    if names.len() != origin.blocked_actions.len()
        || names
            .iter()
            .any(|name| !SCHEMA.iter().any(|(_, known)| name.as_str() == *known))
        || (origin.recovery_message_ticks == 0 && !names.is_empty())
    {
        return Err(invalid("invalid blocked actions in recording origin"));
    }
    Ok(())
}
pub(crate) fn apply_origin(app: &mut App, origin: &RecordingOrigin) {
    let session = app.world_mut().resource_mut::<Session>().unwrap();
    session.phase = origin.phase;
    session.blocked = origin
        .blocked_actions
        .iter()
        .map(|name| SCHEMA.iter().find(|(_, known)| name == known).unwrap().0)
        .collect();
    session.recovery_message_ticks = origin.recovery_message_ticks;
    session.origin = origin.clone();
    sync_hud(app.world_mut());
    app.refresh_extracted();
}
