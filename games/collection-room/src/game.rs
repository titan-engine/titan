//! Deterministic, GPU-independent collection room. Positions are millimeters.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use titan::input::{ActionValue, InputFrame, InputRecording, InputTracker, RecordingHeader};
use titan::inspection::{InspectionConfig, Inspector};
use titan::render::three_d::*;
use titan::replay::RecordedButtons;
use titan::{App, Component, FixedTime, FixedUpdate, Name, Startup, World};
use titan_protocol::{
    CommandMetadata, ErrorCode, FieldMetadata, InputValue, ProtocolError, QueryMetadata,
};

pub const MAX_RECORDING_TICKS: usize = 4096;
pub const ROOM_BOUND: i32 = 4500;
pub const AXIAL_STEP: i32 = 250;
pub const DIAGONAL_STEP: i32 = 177;
pub const FIXTURE: &str = "collection-room-v1";
const PLAYER_HALF: i32 = 200;
const PICKUP_RADIUS: i32 = 350;
const OBSTACLES: [(i32, i32, i32); 2] = [(0, 0, 750), (2000, 1000, 500)];
const GEMS: [(i32, i32); 3] = [(-1000, 3000), (-1000, -2000), (3000, -2000)];

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Position {
    pub x: i32,
    pub z: i32,
}
#[derive(Component)]
struct Player;
#[derive(Component)]
struct Collectible {
    collected: bool,
}
#[derive(Component, Default)]
pub struct Progress {
    pub collected: u32,
    pub completed: bool,
}
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
}
const SCHEMA: [(Action, &str); 4] = [
    (Action::Up, "up"),
    (Action::Down, "down"),
    (Action::Left, "left"),
    (Action::Right, "right"),
];
#[derive(Default)]
struct ScheduledInput {
    enabled: bool,
    frames: BTreeMap<u64, Vec<(Action, ActionValue)>>,
    tracker: InputTracker<Action>,
}
struct Session {
    generation: u64,
    tick: u64,
    recording: InputRecording<Action>,
    valid: bool,
    truncated: bool,
}
impl Session {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            tick: 0,
            recording: InputRecording::new(RecordingHeader::new(16_666_667, 45, 0x45)),
            valid: true,
            truncated: false,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Teleport {
    x: i32,
    z: i32,
}
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

pub fn build_game() -> App {
    let mut app = App::new();
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.world_mut().insert_resource(ScheduledInput::default());
    app.world_mut().insert_resource(Session::new(0));
    app.add_systems(Startup, setup);
    app.add_systems(FixedUpdate, tick);
    app.add_extractor(extract);
    app
}
fn initial_position() -> Position {
    Position { x: -3000, z: 3000 }
}
fn setup(world: &mut World) {
    let mut assets = MeshAssets::new();
    let cube = assets.insert(Mesh::cube(1.0).unwrap()).unwrap();
    let floor = assets.insert(Mesh::floor(10.0).unwrap()).unwrap();
    world.insert_resource(assets);
    // Camera's -Z forward tilts 45 degrees downward with a negative X rotation.
    world.insert_resource(
        PerspectiveCamera::new(
            Vec3::new(0.0, 10.0, 10.0),
            Quaternion::new(-0.38268343, 0.0, 0.0, 0.9238795).unwrap(),
            std::f32::consts::FRAC_PI_3,
            16.0 / 9.0,
            0.1,
            50.0,
        )
        .unwrap(),
    );
    world.insert_resource(Lighting3d::new(Vec3::new(-1.0, 3.0, 2.0), 0.3, 0.7).unwrap());
    world.spawn_with((
        Name::new("floor"),
        Position { x: 0, z: 0 },
        Visual {
            mesh: floor,
            scale: Vec3::ONE,
            height: 0.0,
            color: BaseColor::rgb(50, 68, 85),
        },
    ));
    for (index, (x, z, half)) in OBSTACLES.into_iter().enumerate() {
        world.spawn_with((
            Name::new(format!("obstacle-{}", index + 1)),
            Position { x, z },
            Visual {
                mesh: cube,
                scale: Vec3::new(half as f32 / 500.0, 1.5, half as f32 / 500.0),
                height: 0.75,
                color: BaseColor::rgb(115, 135, 160),
            },
        ));
    }
    // Four visible walls align their inner faces with the player footprint bounds.
    for (name, x, z, sx, sz) in [
        ("wall-west", -4800, 0, 0.2, 9.8),
        ("wall-east", 4800, 0, 0.2, 9.8),
        ("wall-north", 0, -4800, 9.8, 0.2),
        ("wall-south", 0, 4800, 9.8, 0.2),
    ] {
        world.spawn_with((
            Name::new(name),
            Position { x, z },
            Visual {
                mesh: cube,
                scale: Vec3::new(sx, 0.4, sz),
                height: 0.2,
                color: BaseColor::rgb(85, 105, 130),
            },
        ));
    }
    world.spawn_with((
        Name::new("player"),
        initial_position(),
        Player,
        Progress::default(),
        Visual {
            mesh: cube,
            scale: Vec3::new(0.4, 0.7, 0.4),
            height: 0.35,
            color: BaseColor::rgb(65, 215, 235),
        },
    ));
    for (index, (x, z)) in GEMS.into_iter().enumerate() {
        world.spawn_with((
            Name::new(format!("collectible-{}", index + 1)),
            Position { x, z },
            Collectible { collected: false },
            Visual {
                mesh: cube,
                scale: Vec3::new(0.3, 0.3, 0.3),
                height: 0.3,
                color: BaseColor::rgb(255, 205, 60),
            },
        ));
    }
}
/// Reset game input/progress/replay timeline; the host FixedTime remains monotonic.
pub fn restart(app: &mut App) {
    app.update_schedule(Startup);
    let player = app.world().iter::<Player>().next().unwrap().0;
    *app.world_mut().get_mut::<Position>(player).unwrap() = initial_position();
    *app.world_mut().get_mut::<Progress>(player).unwrap() = Progress::default();
    let gems: Vec<_> = app
        .world()
        .iter::<Collectible>()
        .map(|(id, _)| id)
        .collect();
    for id in gems {
        app.world_mut()
            .get_mut::<Collectible>(id)
            .unwrap()
            .collected = false;
    }
    let generation = app
        .world()
        .resource::<Session>()
        .unwrap()
        .generation
        .checked_add(1)
        .expect("session generation exhausted");
    app.world_mut().insert_resource(Session::new(generation));
    app.world_mut().insert_resource(ScheduledInput::default());
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.refresh_extracted();
}
fn permitted(position: Position) -> bool {
    position.x.abs() <= ROOM_BOUND
        && position.z.abs() <= ROOM_BOUND
        && !OBSTACLES.iter().any(|&(x, z, half)| {
            (position.x - x).abs() < half + PLAYER_HALF
                && (position.z - z).abs() < half + PLAYER_HALF
        })
}
fn tick(world: &mut World) {
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
    let input = world.resource::<InputFrame<Action>>().unwrap().clone();
    let dx = i32::from(input.is_active(&Action::Right)) - i32::from(input.is_active(&Action::Left));
    let dz = i32::from(input.is_active(&Action::Down)) - i32::from(input.is_active(&Action::Up));
    let step = if dx != 0 && dz != 0 {
        DIAGONAL_STEP
    } else {
        AXIAL_STEP
    };
    let player = world.iter::<Player>().next().unwrap().0;
    let mut position = *world.get::<Position>(player).unwrap();
    // Stable X-then-Z sliding; each displacement is smaller than any obstacle.
    let x_candidate = Position {
        x: position.x + dx * step,
        ..position
    };
    if permitted(x_candidate) {
        position = x_candidate;
    }
    let z_candidate = Position {
        z: position.z + dz * step,
        ..position
    };
    if permitted(z_candidate) {
        position = z_candidate;
    }
    *world.get_mut::<Position>(player).unwrap() = position;
    let touching: Vec<_> = world
        .iter::<Collectible>()
        .filter_map(|(id, gem)| {
            let p = world.get::<Position>(id).unwrap();
            let distance = (p.x - position.x).pow(2) + (p.z - position.z).pow(2);
            (!gem.collected && distance <= PICKUP_RADIUS.pow(2)).then_some(id)
        })
        .collect();
    for id in &touching {
        world.get_mut::<Collectible>(*id).unwrap().collected = true;
    }
    let progress = world.get_mut::<Progress>(player).unwrap();
    progress.collected += touching.len() as u32;
    progress.completed |= progress.collected == GEMS.len() as u32;
    let session = world.resource_mut::<Session>().unwrap();
    session.tick += 1;
    if session.recording.len() < MAX_RECORDING_TICKS {
        session.recording.push(input);
    } else {
        session.truncated = true;
    }
}
/// CPU-owned immutable 3D snapshot; collected objects remain inspectable but hidden.
pub fn extract(world: &World) -> Result<RenderFrame3d, Frame3dError> {
    RenderFrame3d::new(
        *world.resource::<PerspectiveCamera>().unwrap(),
        *world.resource::<Lighting3d>().unwrap(),
        world.resource::<MeshAssets>().unwrap(),
        world.iter::<Visual>().filter_map(|(id, visual)| {
            if world
                .get::<Collectible>(id)
                .is_some_and(|gem| gem.collected)
            {
                return None;
            }
            let position = world.get::<Position>(id).unwrap();
            Some(Draw3d {
                mesh: visual.mesh,
                transform: Transform3d::new(
                    Vec3::new(
                        position.x as f32 / 1000.0,
                        visual.height,
                        position.z as f32 / 1000.0,
                    ),
                    Quaternion::IDENTITY,
                    visual.scale,
                )
                .unwrap(),
                color: visual.color,
                order: (u64::from(id.index()) << 32) | u64::from(id.generation()),
            })
        }),
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
    let session = app.world().resource::<Session>().unwrap();
    if !session.valid {
        return Err(invalid(
            "recording invalidated by teleport; restart before recording",
        ));
    }
    Ok(Recording {
        format_version: 1,
        fixture: FIXTURE.into(),
        frames: session
            .recording
            .frames()
            .iter()
            .map(|frame| {
                RecordedButtons::capture(frame, &SCHEMA).expect("game uses digital schema")
            })
            .collect(),
        truncated: session.truncated,
    })
}
/// Validate completely before changing state; bounded replay uses shared edge decoding.
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
        .map(|frame| {
            frame
                .decode(&SCHEMA)
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
    let session = world.resource::<Session>().unwrap();
    let player = world.iter::<Player>().next().map(|(id, _)| id);
    let position = player.and_then(|id| world.get::<Position>(id));
    let progress = player.and_then(|id| world.get::<Progress>(id));
    let remaining: Vec<_> = world
        .iter::<Collectible>()
        .filter(|(_, gem)| !gem.collected)
        .map(|(id, _)| world.get::<Name>(id).unwrap().to_string())
        .collect();
    serde_json::json!({"fixture":FIXTURE,"frame":world.resource::<FixedTime>().unwrap().tick(),"session_generation":session.generation,"session_tick":session.tick,"position":position,"collected":progress.map_or(0,|p|p.collected),"total":GEMS.len(),"completed":progress.is_some_and(|p|p.completed),"remaining":remaining,"recorded_ticks":session.recording.len(),"recording_valid":session.valid,"recording_truncated":session.truncated,"pending_inputs":world.resource::<ScheduledInput>().unwrap().frames.len()})
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
    let mutation_enabled = config.mutation_enabled;
    let mut inspector = Inspector::new(config);
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
        .register_read_only_field::<Progress, _>(
            "collected",
            field("u32", "Collected count"),
            |p| p.collected,
        )
        .unwrap();
    inspector
        .register_read_only_field::<Progress, _>(
            "completed",
            field("bool", "Latched room completion"),
            |p| p.completed,
        )
        .unwrap();
    inspector
        .register_read_only_field::<Collectible, _>(
            "collected",
            field("bool", "Collected exactly once"),
            |p| p.collected,
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
                    field("object", "Collection room recording object"),
                )]),
            },
            |app, args: ReplayArgs| replay(app, args.recording),
        )
        .unwrap();
    inspector.register_command(CommandMetadata{name:"teleport".into(),description:"Mutation opt-in only; bounded unobstructed millimeter position; invalidates recording".into(),arguments:BTreeMap::from([("x".into(),field("i32","Millimeter coordinate")),("z".into(),field("i32","Millimeter coordinate"))])},move |app,args:Teleport| {
        if !mutation_enabled {return Err(ProtocolError::new(ErrorCode::MutationDisabled,"runtime mutation was not explicitly enabled"));}
        // Validate range before subtraction/abs: hostile i32::MIN never overflows.
        if !(-ROOM_BOUND..=ROOM_BOUND).contains(&args.x) || !(-ROOM_BOUND..=ROOM_BOUND).contains(&args.z) || !permitted(Position{x:args.x,z:args.z}) {return Err(invalid("position outside room or intersects an obstacle"));}
        app.update_schedule(Startup);
        let player=app.world().iter::<Player>().next().unwrap().0;
        *app.world_mut().get_mut::<Position>(player).unwrap()=Position{x:args.x,z:args.z};
        app.world_mut().resource_mut::<Session>().unwrap().valid=false;
        app.refresh_extracted();
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
                    .ok_or_else(|| invalid("unknown movement action"))?;
                match value {
                    InputValue::Button(pressed) => Ok((
                        action,
                        if *pressed {
                            ActionValue::PRESSED
                        } else {
                            ActionValue::RELEASED
                        },
                    )),
                    InputValue::Axis(_) => Err(invalid("movement requires button input")),
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
