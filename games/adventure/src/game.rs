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
pub const FIXTURE: &str = "adventure-v1";
const PLAYER_HALF: i32 = 200;
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Position {
    pub x: i32,
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
}
pub(crate) const SCHEMA: [(Action, &str); 6] = [
    (Action::Up, "up"),
    (Action::Down, "down"),
    (Action::Left, "left"),
    (Action::Right, "right"),
    (Action::Switch, "switch"),
    (Action::Restart, "restart"),
];
#[derive(Default)]
struct ScheduledInput {
    enabled: bool,
    frames: BTreeMap<u64, Vec<(Action, ActionValue)>>,
    tracker: InputTracker<Action>,
}
struct PendingSwitch;
struct Session {
    generation: u64,
    tick: u64,
    recording: InputRecording<Action>,
    truncated: bool,
    active: usize,
    blocked: BTreeSet<Action>,
    consumed: InputFrame<Action>,
    effective_tracker: InputTracker<Action>,
}
impl Session {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            tick: 0,
            recording: InputRecording::new(RecordingHeader::new(16_666_667, 81, 0x81)),
            truncated: false,
            active: 0,
            blocked: BTreeSet::new(),
            consumed: InputFrame::default(),
            effective_tracker: InputTracker::default(),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
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
fn initial_position(index: usize) -> Position {
    Position {
        x: if index == 0 { 1500 } else { 3500 },
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
    app.world_mut().insert_resource(Session::new(0));
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
        Position { x: 6000, z: 4000 },
        Visual {
            mesh: floor,
            scale: Vec3::new(12.0, 1.0, 8.0),
            height: 0.0,
            color: BaseColor::rgb(50, 68, 85),
        },
    ));
    for (name, x, z, sx, sz) in [
        ("wall-west", -100, 4000, 0.2, 8.4),
        ("wall-east", 12100, 4000, 0.2, 8.4),
        ("wall-north", 6000, -100, 12.0, 0.2),
    ] {
        world.spawn_with((
            Name::new(name),
            Position { x, z },
            Visual {
                mesh: cube,
                scale: Vec3::new(sx, 0.3, sz),
                height: 0.15,
                color: BaseColor::rgb(85, 105, 130),
            },
        ));
    }
    for index in 0..2 {
        world.spawn_with((
            Name::new(character_name(index)),
            Character { index },
            initial_position(index),
            Visual {
                mesh: cube,
                scale: if index == 0 {
                    Vec3::new(0.35, 0.95, 0.35)
                } else {
                    Vec3::new(0.65, 0.65, 0.65)
                },
                height: if index == 0 { 0.475 } else { 0.325 },
                color: if index == 0 {
                    BaseColor::rgb(65, 215, 235)
                } else {
                    BaseColor::rgb(240, 155, 70)
                },
            },
        ));
    }
    world.insert_resource(Markers { cube, triangle });
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
    }
    let generation = world
        .resource::<Session>()
        .unwrap()
        .generation
        .checked_add(1)
        .expect("session generation exhausted");
    world.insert_resource(Session::new(generation));
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
pub(crate) fn clear_scheduled_input(world: &mut World) {
    world.insert_resource(ScheduledInput::default());
}
fn tick(world: &mut World) {
    crate::player::prepare_tick(world);
    let next = world.resource::<FixedTime>().unwrap().tick() + 1;
    let scheduled = world.resource_mut::<ScheduledInput>().unwrap();
    let frame = if scheduled.enabled {
        Some(
            scheduled
                .tracker
                .sample(scheduled.frames.remove(&next).unwrap_or_default()),
        )
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
    let active: BTreeSet<_> = input.active_actions().map(|(a, _)| *a).collect();
    let reset = input.just_pressed(&Action::Restart);
    let switch = input.just_pressed(&Action::Switch);
    if reset {
        reset_world(world);
    }
    let session = world.resource_mut::<Session>().unwrap();
    // A fresh press also proves release/repress between fixed ticks. Preserve
    // this edge in the raw recording so replay makes the same decision.
    session
        .blocked
        .retain(|a| active.contains(a) && !input.just_pressed(a));
    if reset || switch {
        if !reset {
            session.active = 1 - session.active;
        }
        session.blocked.extend(active.iter().copied());
    }
    let effective: Vec<_> = active
        .iter()
        .filter(|a| !session.blocked.contains(a) && !matches!(a, Action::Switch | Action::Restart))
        .map(|a| (*a, ActionValue::PRESSED))
        .collect();
    session.consumed = session.effective_tracker.sample(effective);
    let dx = i32::from(session.consumed.is_active(&Action::Right))
        - i32::from(session.consumed.is_active(&Action::Left));
    let dz = i32::from(session.consumed.is_active(&Action::Down))
        - i32::from(session.consumed.is_active(&Action::Up));
    let target = session.active;
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
    let id = world
        .iter::<Character>()
        .find(|(_, c)| c.index == target)
        .unwrap()
        .0;
    let position = world.get_mut::<Position>(id).unwrap();
    position.x = (position.x + dx * step).clamp(PLAYER_HALF, 12000 - PLAYER_HALF);
    position.z = (position.z + dz * step).clamp(PLAYER_HALF, 8000 - PLAYER_HALF);
    sync_hud(world);
}
fn sync_hud(world: &mut World) {
    let active = world.resource::<Session>().unwrap().active;
    let id = world.iter::<Hud>().next().unwrap().0;
    world.get_mut::<UiText>(id).unwrap().text = format!(
        "ACTIVE: {}  [Q] SWITCH  [R] RESTART",
        character_name(active).to_uppercase()
    );
}
pub fn extract_overlay(world: &World) -> RenderFrame {
    let mut frame = RenderFrame::new(320, 180, Color::rgba(0, 0, 0, 0));
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
                Vec3::new(x, visual.height, z),
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
                    Vec3::new(x, 1.15, z),
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
                            Vec3::new(x + ox, 0.045, z + oz),
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
    RenderFrame3d::new(
        *world.resource::<PerspectiveCamera>().unwrap(),
        *world.resource::<Lighting3d>().unwrap(),
        world.resource::<MeshAssets>().unwrap(),
        draws,
        Frame3dLimits {
            max_draws: 32,
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
    restart(app);
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
        .map(|(id, c)| (character_name(c.index), *world.get::<Position>(id).unwrap()))
        .collect();
    serde_json::json!({"fixture":FIXTURE,"frame":world.resource::<FixedTime>().unwrap().tick(),"session_generation":s.generation,"session_tick":s.tick,"characters":characters,"active_character":character_name(s.active),"consumed_input":RecordedButtons::capture(&s.consumed,&SCHEMA).unwrap(),"blocked_actions":s.blocked.iter().map(|a|SCHEMA.iter().find(|(v,_)|v==a).unwrap().1).collect::<Vec<_>>(),"recorded_ticks":s.recording.len(),"recording_valid":true,"recording_truncated":s.truncated,"pending_inputs":world.resource::<ScheduledInput>().unwrap().frames.len()})
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
