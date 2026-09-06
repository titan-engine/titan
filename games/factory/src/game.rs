//! Factory construction, snapshot transport, and bounded ore-to-plate production.
mod production;
mod transport;
pub use production::build_production_fixture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    path::{Path, PathBuf},
};
use titan::inspection::{InspectionConfig, Inspector};
use titan::render::{
    Color, Image, ImageAssets, ImageId, RenderFrame, SoftwareRenderer, SpriteDraw,
};
use titan::{App, Component, FixedTime, FixedUpdate, Name, World};
use titan_protocol::{
    CaptureResult, CommandMetadata, ErrorCode, FieldMetadata, ProtocolError, QueryMetadata,
};
pub use transport::build_transport_fixture;
use transport::{Item, Slots};

pub const WIDTH: i32 = 384;
pub const HEIGHT: i32 = 256;
const TILE: f64 = 32.0;
const RECORD_LIMIT: usize = 256;
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Facing {
    N,
    E,
    S,
    W,
}
impl Facing {
    fn clockwise(self) -> Self {
        match self {
            Self::N => Self::E,
            Self::E => Self::S,
            Self::S => Self::W,
            Self::W => Self::N,
        }
    }
    fn opposite(self) -> Self {
        self.clockwise().clockwise()
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Conveyor,
    Extractor,
    Processor,
    Delivery,
}
#[derive(Component, Clone, Debug, Serialize)]
struct Structure {
    x: i32,
    y: i32,
    kind: Kind,
    facing: Facing,
    slots: Slots,
    progress: u32,
    remaining: u32,
    last_transfer_reason: Option<&'static str>,
}
impl Structure {
    fn inputs(&self) -> Vec<Facing> {
        match self.kind {
            Kind::Extractor => vec![],
            Kind::Conveyor => [Facing::N, Facing::E, Facing::S, Facing::W]
                .into_iter()
                .filter(|d| *d != self.facing)
                .collect(),
            _ => vec![self.facing.opposite()],
        }
    }
    fn output(&self) -> Option<Facing> {
        (self.kind != Kind::Delivery).then_some(self.facing)
    }
    fn value(&self) -> Value {
        json!({"x":self.x,"y":self.y,"kind":self.kind,"facing":self.facing,"inputs":self.inputs(),"output":self.output(),"slots":self.slots,"progress":self.progress,"remaining":self.remaining,"item_positions":transport::item_positions(self),"last_transfer_reason":self.last_transfer_reason})
    }
}
#[derive(Component)]
struct Deposit;
#[derive(Clone, Copy, Serialize)]
struct Camera {
    x: f64,
    y: f64,
    zoom: f64,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            x: 0.,
            y: 0.,
            zoom: 1.,
        }
    }
}
#[derive(Clone, Copy, Serialize)]
struct Selection {
    kind: Kind,
    facing: Facing,
}
struct State {
    tick: u64,
    production_enabled: bool,
    diagnostic: Option<String>,
    seeded: u64,
    extracted: u64,
    delivered: u64,
    discarded_ore: u64,
    discarded_plate: u64,
    completion_tick: Option<u64>,
    camera: Camera,
    selection: Selection,
    hover: Option<(i32, i32)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            tick: 0,
            production_enabled: true,
            diagnostic: None,
            seeded: 0,
            extracted: 0,
            delivered: 0,
            discarded_ore: 0,
            discarded_plate: 0,
            completion_tick: None,
            camera: Camera::default(),
            selection: Selection {
                kind: Kind::Conveyor,
                facing: Facing::E,
            },
            hover: None,
        }
    }
}
#[derive(Default)]
struct Recording {
    records: VecDeque<Value>,
    dropped: u64,
}
struct Art(ImageId);
#[derive(Default)]
struct Epoch(u64);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TileArgs {
    x: i32,
    y: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sequence {
    operations: Vec<Value>,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Place {
        kind: Kind,
        x: i32,
        y: i32,
        facing: Facing,
    },
    Rotate {
        x: i32,
        y: i32,
    },
    Remove {
        x: i32,
        y: i32,
    },
    Inspect {
        x: i32,
        y: i32,
    },
    Select {
        kind: Option<Kind>,
        facing: Option<Facing>,
    },
    Advance {
        ticks: u32,
    },
    Restart,
}

pub fn build_game() -> App {
    let mut app = App::new();
    let mut assets = ImageAssets::new();
    let pixel = assets.insert(Image::from_fn(1, 1, |_, _| Color::WHITE).unwrap());
    app.world_mut().insert_resource(assets);
    app.world_mut().insert_resource(Art(pixel));
    app.world_mut().insert_resource(Recording::default());
    app.world_mut().insert_resource(Epoch::default());
    reset_world(app.world_mut());
    app.add_systems(FixedUpdate, transport::tick);
    app.add_extractor(render_frame);
    app.refresh_extracted();
    app
}
fn reset_world(world: &mut World) {
    let epoch = world.resource_mut::<Epoch>().unwrap();
    epoch.0 = epoch
        .0
        .checked_add(1)
        .expect("factory restart epoch overflow");
    let entities: Vec<_> = world
        .iter::<Structure>()
        .map(|(e, _)| e)
        .chain(world.iter::<Deposit>().map(|(e, _)| e))
        .collect();
    for e in entities {
        world.despawn(e);
    }
    world.spawn_with((Deposit, Name::new("ore deposit (1,3)")));
    world.spawn_with((
        Structure {
            x: 10,
            y: 3,
            kind: Kind::Delivery,
            facing: Facing::E,
            slots: Slots::default(),
            progress: 0,
            remaining: 0,
            last_transfer_reason: None,
        },
        Name::new("delivery"),
    ));
    world.insert_resource(State::default());
}
pub fn restart(app: &mut App) {
    reset_world(app.world_mut());
    app.refresh_extracted();
}
fn tile(x: i32, y: i32) -> Result<(), String> {
    if (0..12).contains(&x) && (0..8).contains(&y) {
        Ok(())
    } else {
        Err(format!(
            "OUT_OF_BOUNDS: tile ({x},{y}) must be x=0..11, y=0..7"
        ))
    }
}
fn at(world: &World, x: i32, y: i32) -> Option<titan::Entity> {
    world
        .iter::<Structure>()
        .find(|(_, s)| s.x == x && s.y == y)
        .map(|(e, _)| e)
}
fn inspect_tile(world: &World, x: i32, y: i32) -> Value {
    json!({"x":x,"y":y,"terrain":if (x,y)==(1,3){"ore_deposit"}else{"ground"},"structure":at(world,x,y).map(|e|transport::structure_value(world, world.get::<Structure>(e).unwrap()))})
}
fn apply(app: &mut App, op: Operation) -> Result<Value, String> {
    if app
        .world()
        .resource::<State>()
        .unwrap()
        .completion_tick
        .is_some()
        && matches!(
            op,
            Operation::Place { .. }
                | Operation::Rotate { .. }
                | Operation::Remove { .. }
                | Operation::Select { .. }
        )
    {
        return Err("COMPLETE: restart before construction".into());
    }
    if let Some(diagnostic) = &app.world().resource::<State>().unwrap().diagnostic
        && !matches!(op, Operation::Inspect { .. } | Operation::Restart)
    {
        return Err(diagnostic.clone());
    }
    match op {
        Operation::Place { kind, x, y, facing } => {
            tile(x, y)?;
            if kind == Kind::Delivery {
                return Err("FIXED_DELIVERY: delivery is not player-placeable".into());
            }
            if at(app.world(), x, y).is_some() {
                return Err(format!(
                    "OCCUPIED: tile ({x},{y}) already contains a structure"
                ));
            }
            if (kind == Kind::Extractor) != ((x, y) == (1, 3)) {
                return Err(
                    "INVALID_TERRAIN: extractors require (1,3); other structures require ground"
                        .into(),
                );
            }
            app.world_mut().spawn_with((
                Structure {
                    x,
                    y,
                    kind,
                    facing,
                    slots: Slots::default(),
                    progress: 0,
                    remaining: 0,
                    last_transfer_reason: None,
                },
                Name::new(format!("{kind:?} ({x},{y})")),
            ));
            Ok(inspect_tile(app.world(), x, y))
        }
        Operation::Rotate { x, y } | Operation::Remove { x, y } => {
            tile(x, y)?;
            let entity = at(app.world(), x, y)
                .ok_or_else(|| format!("EMPTY_TILE: tile ({x},{y}) has no structure"))?;
            if app.world().get::<Structure>(entity).unwrap().kind == Kind::Delivery {
                return Err("FIXED_DELIVERY: delivery cannot be rotated or removed".into());
            }
            if matches!(op, Operation::Remove { .. }) {
                let slots = app.world().get::<Structure>(entity).unwrap().slots;
                let ore = slots.items().filter(|item| *item == Item::Ore).count() as u64;
                let plate = slots.items().filter(|item| *item == Item::Plate).count() as u64;
                let state = app.world_mut().resource_mut::<State>().unwrap();
                let discarded_ore = state
                    .discarded_ore
                    .checked_add(ore)
                    .ok_or("COUNTER_OVERFLOW: discarded ore")?;
                let discarded_plate = state
                    .discarded_plate
                    .checked_add(plate)
                    .ok_or("COUNTER_OVERFLOW: discarded plate")?;
                state.discarded_ore = discarded_ore;
                state.discarded_plate = discarded_plate;
                app.world_mut().despawn(entity);
                return Ok(
                    json!({"x":x,"y":y,"structure":null,"discarded_ore":ore,"discarded_plate":plate}),
                );
            } else {
                let s = app.world_mut().get_mut::<Structure>(entity).unwrap();
                s.facing = s.facing.clockwise();
            }
            Ok(inspect_tile(app.world(), x, y))
        }
        Operation::Inspect { x, y } => {
            tile(x, y)?;
            Ok(inspect_tile(app.world(), x, y))
        }
        Operation::Select { kind, facing } => {
            if kind == Some(Kind::Delivery) {
                return Err("FIXED_DELIVERY: choose conveyor, extractor or processor".into());
            }
            let state = app.world_mut().resource_mut::<State>().unwrap();
            if let Some(kind) = kind {
                state.selection.kind = kind;
            }
            if let Some(facing) = facing {
                state.selection.facing = facing;
            }
            Ok(json!(state.selection))
        }
        Operation::Advance { ticks } => {
            if ticks > 36000 {
                return Err("ADVANCE_LIMIT: use at most 36000 ticks per operation".into());
            }
            app.advance_fixed(u64::from(ticks));
            if let Some(diagnostic) = &app.world().resource::<State>().unwrap().diagnostic {
                return Err(diagnostic.clone());
            }
            Ok(json!({"tick":app.world().resource::<State>().unwrap().tick}))
        }
        Operation::Restart => {
            restart(app);
            Ok(json!({"tick":0}))
        }
    }
}
/// Executes and records one safe-boundary operation. Rejections preserve game state.
pub fn player_command(app: &mut App, text: &str) -> Result<String, String> {
    // Bound rejected payloads as well as record count.
    if text.len() > 4096 {
        let r = app.world_mut().resource_mut::<Recording>().unwrap();
        if r.records.len() == RECORD_LIMIT {
            r.records.pop_front();
            r.dropped = r.dropped.saturating_add(1);
        }
        r.records.push_back(json!({"operation":{"truncated":true},"result":{"ok":false,"error":"OPERATION_SIZE_LIMIT: use at most 4096 UTF-8 bytes"}}));
        return Err("OPERATION_SIZE_LIMIT: use at most 4096 UTF-8 bytes".into());
    }
    let value = serde_json::from_str::<Value>(text);
    let result = match value.as_ref() {
        Ok(value) => serde_json::from_value::<Operation>(value.clone())
            .map_err(|e| format!("INVALID_OPERATION: {e}"))
            .and_then(|op| apply(app, op)),
        Err(e) => Err(format!("INVALID_JSON: {e}")),
    };
    let record = json!({"operation":value.unwrap_or_else(|_|json!({"invalid_json":text.chars().take(512).collect::<String>()})),"tick":app.world().resource::<State>().unwrap().tick,"result":match &result {Ok(v)=>json!({"ok":true,"value":v}),Err(e)=>json!({"ok":false,"error":e})}});
    let recording = app.world_mut().resource_mut::<Recording>().unwrap();
    if recording.records.len() == RECORD_LIMIT {
        recording.records.pop_front();
        recording.dropped = recording.dropped.saturating_add(1);
    }
    recording.records.push_back(record);
    app.refresh_extracted();
    result.map(|v| v.to_string())
}
pub fn pointer(app: &mut App, x: f64, y: f64, action: &str) -> Result<String, String> {
    if !["hover", "place", "rotate", "remove", "inspect"].contains(&action) {
        return Err("INVALID_POINTER_ACTION: use hover, place, rotate, remove or inspect".into());
    }
    let camera = app.world().resource::<State>().unwrap().camera;
    let tx = ((x - f64::from(WIDTH) / 2. - camera.x) / camera.zoom + f64::from(WIDTH) / 2.) / TILE;
    let ty =
        ((y - f64::from(HEIGHT) / 2. - camera.y) / camera.zoom + f64::from(HEIGHT) / 2.) / TILE;
    let valid = x.is_finite()
        && y.is_finite()
        && x >= 0.
        && x < f64::from(WIDTH)
        && y >= 0.
        && y < f64::from(HEIGHT)
        && (0. ..12.).contains(&tx)
        && (0. ..8.).contains(&ty);
    let hovered = valid.then_some((tx.floor() as i32, ty.floor() as i32));
    if action == "hover" {
        app.world_mut().resource_mut::<State>().unwrap().hover = hovered;
        app.refresh_extracted();
        return Ok(json!({"hover":hovered.map(|(x,y)|json!({"x":x,"y":y}))}).to_string());
    }
    let (x, y) = hovered.ok_or("OUT_OF_BOUNDS: pointer is outside the visible grid")?;
    let selection = app.world().resource::<State>().unwrap().selection;
    let command = if action == "place" {
        json!({"op":action,"x":x,"y":y,"kind":selection.kind,"facing":selection.facing})
    } else {
        json!({"op":action,"x":x,"y":y})
    };
    player_command(app, &command.to_string())
}
pub fn camera(app: &mut App, dx: f64, dy: f64, zoom: f64) -> Result<(), String> {
    if !dx.is_finite() || !dy.is_finite() || !zoom.is_finite() || zoom <= 0. {
        return Err("INVALID_CAMERA: finite pan and positive finite zoom required".into());
    }
    let state = app.world_mut().resource_mut::<State>().unwrap();
    state.camera.x = (state.camera.x + dx).clamp(-768., 768.);
    state.camera.y = (state.camera.y + dy).clamp(-512., 512.);
    state.camera.zoom = (state.camera.zoom * zoom).clamp(0.5, 3.);
    state.hover = None;
    app.refresh_extracted();
    Ok(())
}
fn state_value(app: &App) -> Value {
    let state = app.world().resource::<State>().unwrap();
    let mut structures: Vec<_> = app.world().iter::<Structure>().map(|(_, s)| s).collect();
    structures.sort_by_key(|s| (s.y, s.x));
    json!({"frame":app.world().resource::<FixedTime>().unwrap().tick(),"tick":state.tick,"width":12,"height":8,"selection":state.selection,"camera":state.camera,"hover":state.hover.map(|(x,y)|json!({"x":x,"y":y})),"structures":structures.iter().map(|s|transport::structure_value(app.world(), s)).collect::<Vec<_>>(),"deposit":{"x":1,"y":3},"production_enabled":state.production_enabled,"diagnostic":state.diagnostic,"seeded":state.seeded,"extracted":state.extracted,"delivered":state.delivered,"discarded_ore":state.discarded_ore,"discarded_plate":state.discarded_plate,"completion_tick":state.completion_tick,"conserved":transport::conserved(app.world()),"outcome":if state.completion_tick.is_some(){"Complete"}else if state.diagnostic.is_some(){"Stopped"}else{"Running"},"objective":"Extract ore at (1,3), process ore into plates, deliver 10 plates to (10,3)."})
}
pub fn status(app: &App) -> String {
    state_value(app).to_string()
}
fn field(ty: &str, description: &str) -> FieldMetadata {
    FieldMetadata {
        type_name: ty.into(),
        description: description.into(),
        writable: false,
        minimum: None,
        maximum: None,
        unit: None,
    }
}
fn command_arguments(name: &str) -> BTreeMap<String, FieldMetadata> {
    let mut fields = BTreeMap::new();
    if name == "construct" {
        fields.insert("op".into(),field("string","place/rotate/remove/inspect/select/advance/restart; additional fields depend on operation"));
    }
    if ["construct", "place", "rotate", "remove"].contains(&name) {
        fields.insert(
            "x".into(),
            field("i32", "Column 0..11; required for tile operations"),
        );
        fields.insert(
            "y".into(),
            field("i32", "Row 0..7; required for tile operations"),
        );
    }
    if ["construct", "place", "select"].contains(&name) {
        fields.insert(
            "kind".into(),
            field(
                "string",
                "conveyor/extractor/processor; optional for select",
            ),
        );
        fields.insert(
            "facing".into(),
            field("string", "N/E/S/W; optional for select"),
        );
    }
    if ["construct", "advance"].contains(&name) {
        fields.insert(
            "ticks".into(),
            field("u32", "Number of fixed ticks, 0..36000"),
        );
    }
    fields
}
fn protocol(error: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidValue, error)
}
pub fn inspector_with_capture(
    config: InspectionConfig,
    capture: impl FnMut(&App) -> Result<CaptureResult, ProtocolError> + Send + 'static,
) -> Inspector {
    let mut inspector = Inspector::new(config);
    inspector
        .register_read_only_field::<Structure, _>("x", field("i32", "Grid column"), |s| s.x)
        .unwrap();
    inspector
        .register_read_only_field::<Structure, _>("y", field("i32", "Grid row"), |s| s.y)
        .unwrap();
    inspector
        .register_read_only_field::<Structure, _>("kind", field("string", "Structure kind"), |s| {
            serde_json::to_value(s.kind).unwrap()
        })
        .unwrap();
    inspector
        .register_read_only_field::<Structure, _>(
            "facing",
            field("string", "Output direction; delivery faces E"),
            |s| serde_json::to_value(s.facing).unwrap(),
        )
        .unwrap();
    inspector
        .register_read_only_field::<Structure, _>(
            "inputs",
            field("array", "Accepting input faces"),
            |s| s.inputs(),
        )
        .unwrap();
    inspector
        .register_read_only_field::<Structure, _>(
            "output",
            field("string|null", "Output face"),
            |s| s.output(),
        )
        .unwrap();
    inspector
        .register_read_only_field::<Structure, _>(
            "slots",
            field("object", "Distinct input, in-process and output item slots"),
            |s| serde_json::to_value(s.slots).unwrap(),
        )
        .unwrap();
    inspector
        .register_read_only_field::<Structure, _>(
            "progress",
            field("u32", "Extractor eligible ticks, 0..59"),
            |s| s.progress,
        )
        .unwrap();
    inspector
        .register_read_only_field::<Structure, _>("remaining", field("u32", "Processor work ticks, 0..120; zero with an in-process ore is blocked finished work"), |s| s.remaining)
        .unwrap();
    inspector
        .register_query(
            QueryMetadata {
                name: "state".into(),
                description: "Canonical bounded construction state".into(),
                arguments: BTreeMap::new(),
            },
            |app, _: Empty| Ok(state_value(app)),
        )
        .unwrap();
    inspector.register_query(QueryMetadata{name:"recording".into(),description:"Last 256 ordered boundary operations and outcomes; dropped counts truncated records".into(),arguments:BTreeMap::new()},|app,_:Empty|{let r=app.world().resource::<Recording>().unwrap();Ok(json!({"operations":r.records,"dropped":r.dropped}))}).unwrap();
    inspector
        .register_query(
            QueryMetadata {
                name: "tile".into(),
                description: "Inspect terrain, structure and ports at one grid tile".into(),
                arguments: [
                    ("x".into(), field("i32", "Column 0..11")),
                    ("y".into(), field("i32", "Row 0..7")),
                ]
                .into(),
            },
            |app, args: TileArgs| {
                tile(args.x, args.y).map_err(protocol)?;
                Ok(inspect_tile(app.world(), args.x, args.y))
            },
        )
        .unwrap();
    for name in [
        "construct",
        "place",
        "rotate",
        "remove",
        "select",
        "advance",
        "restart",
    ] {
        let command = name.to_owned();
        inspector.register_command(CommandMetadata{name:name.into(),description:format!("Validated factory {name}; use construct with tagged op JSON, or direct command arguments. Errors include actionable codes."),arguments:command_arguments(name)},move |app,mut args:Value| {
            if command!="construct" { let object=args.as_object_mut().ok_or_else(||protocol("INVALID_OPERATION: arguments must be an object".into()))?; if object.contains_key("op") {return Err(protocol("INVALID_OPERATION: direct commands must omit op".into()));} object.insert("op".into(),json!(command)); }
            player_command(app,&args.to_string()).map(|_|()).map_err(protocol)
        }).unwrap();
    }
    inspector.register_command(CommandMetadata{name:"sequence".into(),description:"Execute at most 256 tagged operations serially; individual rejections are recorded and later operations continue".into(),arguments:[("operations".into(),field("array","Ordered tagged operations"))].into()},|app,args:Sequence|{
        if args.operations.len()>256{return Err(protocol("SEQUENCE_LIMIT: use at most 256 operations".into()));}
        for op in args.operations {let _=player_command(app,&op.to_string());} Ok(())
    }).unwrap();
    inspector.register_capture_handler(capture);
    inspector
}
#[cfg(not(target_arch = "wasm32"))]
pub fn configured_inspector(output_path: PathBuf, config: InspectionConfig) -> Inspector {
    inspector_with_capture(config, move |app| {
        let image = render_image(app.world())?;
        write_ppm(&output_path, &image)
            .map_err(|e| ProtocolError::new(ErrorCode::Internal, e.to_string()))?;
        Ok(CaptureResult {
            identity: Default::default(),
            width: image.width(),
            height: image.height(),
            format: "ppm".into(),
            artifact: output_path.to_string_lossy().into_owned(),
            checksum: format!("{:016x}", image_checksum(&image)),
        })
    })
}
#[cfg(not(target_arch = "wasm32"))]
pub fn diagnostic_positions(world: &World) -> Value {
    let mut values: Vec<_> = world.iter::<Structure>().map(|(_, s)| s).collect();
    values.sort_by_key(|s| (s.y, s.x));
    json!(values.iter().map(|s| s.value()).collect::<Vec<_>>())
}
pub fn render_image(world: &World) -> Result<Image, ProtocolError> {
    SoftwareRenderer::render(
        &render_frame(world),
        world.resource::<ImageAssets>().unwrap(),
    )
    .map_err(|e| ProtocolError::new(ErrorCode::Internal, format!("render failed: {e:?}")))
}
fn render_frame(world: &World) -> RenderFrame {
    let mut frame = RenderFrame::new(WIDTH as u32, HEIGHT as u32, Color::rgb(16, 23, 31));
    let state = world.resource::<State>().unwrap();
    let pixel = world.resource::<Art>().unwrap().0;
    let mut rect = |x: f64, y: f64, w: f64, h: f64, color: Color| {
        let sx = ((x - 192.) * state.camera.zoom + 192. + state.camera.x).round() as i32;
        let sy = ((y - 128.) * state.camera.zoom + 128. + state.camera.y).round() as i32;
        let sw = (w * state.camera.zoom).round().max(1.) as i32;
        let sh = (h * state.camera.zoom).round().max(1.) as i32;
        let size = sw.min(sh);
        for offset in (0..sw.max(sh)).step_by(size as usize) {
            let offset = offset.min(sw.max(sh) - size);
            let scale = size;
            frame.push(
                SpriteDraw::new(
                    pixel,
                    sx + if sw > sh { offset } else { 0 },
                    sy + if sh >= sw { offset } else { 0 },
                )
                .with_pixel_scale(scale as u32)
                .with_tint(color),
            );
        }
    };
    for y in 0..8 {
        for x in 0..12 {
            rect(
                f64::from(x) * TILE + 1.,
                f64::from(y) * TILE + 1.,
                30.,
                30.,
                Color::rgb(38, 51, 60),
            );
        }
    }
    rect(36., 100., 24., 24., Color::rgb(132, 79, 45));
    for (_, s) in world.iter::<Structure>() {
        let x = f64::from(s.x) * TILE;
        let y = f64::from(s.y) * TILE;
        let color = match s.kind {
            Kind::Conveyor => Color::rgb(93, 111, 123),
            Kind::Extractor => Color::rgb(230, 157, 53),
            Kind::Processor => Color::rgb(171, 112, 212),
            Kind::Delivery => Color::rgb(67, 186, 131),
        };
        rect(x + 6., y + 6., 20., 20., color);
        // Center marks distinguish structures independently of color.
        match s.kind {
            Kind::Extractor => rect(x + 13., y + 10., 6., 12., Color::rgb(45, 36, 25)),
            Kind::Processor => {
                rect(x + 10., y + 11., 4., 10., Color::rgb(46, 30, 63));
                rect(x + 18., y + 11., 4., 10., Color::rgb(46, 30, 63));
            }
            Kind::Delivery => rect(x + 11., y + 11., 10., 10., Color::rgb(17, 66, 43)),
            Kind::Conveyor => {}
        }
        for direction in s.inputs() {
            let (px, py, w, h) = match direction {
                Facing::N => (12., 2., 8., 3.),
                Facing::E => (27., 12., 3., 8.),
                Facing::S => (12., 27., 8., 3.),
                Facing::W => (2., 12., 3., 8.),
            };
            rect(x + px, y + py, w, h, Color::rgb(107, 210, 241));
        }
        if let Some(direction) = s.output() {
            let (dx, dy) = match direction {
                Facing::N => (0., -1.),
                Facing::E => (1., 0.),
                Facing::S => (0., 1.),
                Facing::W => (-1., 0.),
            };
            for i in 0..4 {
                let distance = 4. + f64::from(i) * 2.;
                rect(
                    x + 14. + dx * distance,
                    y + 14. + dy * distance,
                    4.,
                    4.,
                    Color::rgb(255, 233, 141),
                );
            }
            rect(
                x + 14. + dx * 10. - dy * 3.,
                y + 14. + dy * 10. + dx * 3.,
                3.,
                3.,
                Color::rgb(255, 233, 141),
            );
            rect(
                x + 14. + dx * 10. + dy * 3.,
                y + 14. + dy * 10. - dx * 3.,
                3.,
                3.,
                Color::rgb(255, 233, 141),
            );
        }
    }
    for (_, s) in world.iter::<Structure>() {
        for position in transport::item_positions(s) {
            let color = if position["item"] == "ore" {
                Color::rgb(255, 125, 52)
            } else {
                Color::rgb(213, 236, 255)
            };
            let x = position["x"].as_f64().unwrap();
            let y = position["y"].as_f64().unwrap();
            rect(x - 4., y - 4., 8., 8., Color::rgb(16, 22, 28));
            rect(x - 3., y - 3., 6., 6., color);
        }
    }
    if let Some((x, y)) = state.hover {
        let x = f64::from(x) * TILE;
        let y = f64::from(y) * TILE;
        let c = Color::rgb(255, 255, 255);
        rect(x, y, 32., 2., c);
        rect(x, y + 30., 32., 2., c);
        rect(x, y, 2., 32., c);
        rect(x + 30., y, 2., 32., c);
    }
    frame
}
pub struct InteractiveInput {
    held: BTreeSet<String>,
    epoch: u64,
}
impl InteractiveInput {
    /// Bind held state to the current run, including a run already restarted.
    pub fn for_app(app: &App) -> Self {
        Self {
            held: BTreeSet::new(),
            epoch: app.world().resource::<Epoch>().unwrap().0,
        }
    }
    fn sync_epoch(&mut self, app: &App) {
        let epoch = app.world().resource::<Epoch>().unwrap().0;
        if self.epoch != epoch {
            self.clear();
            self.epoch = epoch;
        }
    }
    pub fn clear(&mut self) {
        self.held.clear();
    }
    pub fn cancel_action(&mut self, app: &App, name: &str) -> Result<(), String> {
        self.set_action(app, name, false)
    }
    pub fn set_action(&mut self, app: &App, name: &str, pressed: bool) -> Result<(), String> {
        if !["up", "down", "left", "right"].contains(&name) {
            return Err(format!("unknown action: {name}"));
        }
        // Drop keys from the previous run before applying this new event.
        self.sync_epoch(app);
        if pressed {
            self.held.insert(name.into());
        } else {
            self.held.remove(name);
        }
        Ok(())
    }
    pub fn tick(&mut self, app: &mut App) {
        self.sync_epoch(app);
        let x = i32::from(self.held.contains("left")) - i32::from(self.held.contains("right"));
        let y = i32::from(self.held.contains("up")) - i32::from(self.held.contains("down"));
        if x != 0 || y != 0 {
            let _ = camera(app, f64::from(x) * 4., f64::from(y) * 4., 1.);
        }
        app.advance_fixed(1);
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn write_ppm(path: &Path, image: &Image) -> std::io::Result<()> {
    let mut bytes = format!("P6\n{} {}\n255\n", image.width(), image.height()).into_bytes();
    for pixel in image.pixels().as_chunks::<4>().0 {
        bytes.extend_from_slice(&pixel[..3]);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)
}
pub fn image_checksum(image: &Image) -> u64 {
    image
        .pixels()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command(app: &mut App, value: Value) -> Result<Value, String> {
        player_command(app, &value.to_string()).map(|s| serde_json::from_str(&s).unwrap())
    }
    #[test]
    fn validates_every_construction_rule_without_mutation() {
        let mut app = build_game();
        for operation in [
            json!({"op":"place","kind":"extractor","x":0,"y":0,"facing":"E"}),
            json!({"op":"place","kind":"conveyor","x":1,"y":3,"facing":"E"}),
            json!({"op":"place","kind":"processor","x":1,"y":3,"facing":"E"}),
            json!({"op":"place","kind":"delivery","x":0,"y":0,"facing":"E"}),
            json!({"op":"place","kind":"conveyor","x":12,"y":0,"facing":"E"}),
            json!({"op":"place","kind":"conveyor","x":-1,"y":0,"facing":"E"}),
            json!({"op":"place","kind":"conveyor","x":0,"y":8,"facing":"E"}),
            json!({"op":"place","kind":"conveyor","x":10,"y":3,"facing":"E"}),
            json!({"op":"place","kind":"unknown","x":0,"y":0,"facing":"E"}),
            json!({"op":"place","kind":"conveyor","x":0,"y":0,"facing":"NE"}),
            json!({"op":"place","kind":"conveyor","x":0,"y":0,"facing":"E","extra":true}),
            json!({"op":"rotate","x":10,"y":3}),
            json!({"op":"remove","x":10,"y":3}),
            json!({"op":"rotate","x":0,"y":0}),
            json!({"op":"remove","x":0,"y":0}),
            json!({"op":"select","kind":"delivery","facing":"S"}),
            json!({"op":"advance","ticks":36001}),
        ] {
            let before = status(&app);
            assert!(command(&mut app, operation.clone()).is_err(), "{operation}");
            assert_eq!(before, status(&app), "{operation}");
        }
        assert_eq!(
            app.world().resource::<Recording>().unwrap().records.len(),
            17
        );
    }
    #[test]
    fn construction_rotation_ports_and_removal_are_inspectable() {
        let mut app = build_game();
        for (kind, x, y) in [("extractor", 1, 3), ("conveyor", 0, 0), ("processor", 2, 2)] {
            command(
                &mut app,
                json!({"op":"place","kind":kind,"x":x,"y":y,"facing":"E"}),
            )
            .unwrap();
            let old = status(&app);
            assert!(
                command(
                    &mut app,
                    json!({"op":"place","kind":kind,"x":x,"y":y,"facing":"E"})
                )
                .unwrap_err()
                .starts_with("OCCUPIED")
            );
            assert_eq!(old, status(&app));
            for facing in ["S", "W", "N", "E"] {
                let value = command(&mut app, json!({"op":"rotate","x":x,"y":y})).unwrap();
                assert_eq!(value["structure"]["facing"], facing);
                assert_eq!(value["structure"]["output"], facing);
                let input_count = if kind == "extractor" {
                    0
                } else if kind == "processor" {
                    1
                } else {
                    3
                };
                assert_eq!(
                    value["structure"]["inputs"].as_array().unwrap().len(),
                    input_count
                );
                assert!(
                    !value["structure"]["inputs"]
                        .as_array()
                        .unwrap()
                        .contains(&json!(facing))
                );
            }
            command(&mut app, json!({"op":"remove","x":x,"y":y})).unwrap();
            assert_eq!(
                command(&mut app, json!({"op":"inspect","x":x,"y":y})).unwrap()["structure"],
                Value::Null
            );
        }
    }
    #[test]
    fn camera_inverse_maps_pointer_and_rejects_off_canvas() {
        let mut app = build_game();
        for zoom in [0.5, 1., 1.7, 3.] {
            restart(&mut app);
            camera(&mut app, 11., -7., zoom).unwrap();
            let x = (2.5 * TILE - 192.) * zoom + 192. + 11.;
            let y = (3.5 * TILE - 128.) * zoom + 128. - 7.;
            if x >= 0. {
                pointer(&mut app, x, y, "hover").unwrap();
                assert_eq!(state_value(&app)["hover"], json!({"x":2,"y":3}));
                pointer(&mut app, x, y, "place").unwrap();
                assert!(at(app.world(), 2, 3).is_some());
            }
            let before = status(&app);
            assert!(pointer(&mut app, -1., 100., "place").is_err());
            assert!(pointer(&mut app, f64::NAN, 100., "place").is_err());
            assert!(pointer(&mut app, WIDTH as f64, 100., "place").is_err());
            assert_eq!(before, status(&app));
        }
        let before = status(&app);
        assert!(camera(&mut app, f64::NAN, 0., 1.).is_err());
        assert!(camera(&mut app, 0., 0., 0.).is_err());
        assert_eq!(before, status(&app));
    }
    #[test]
    fn deterministic_sequence_and_restart_have_fresh_construction_state() {
        let operations = [
            json!({"op":"place","kind":"extractor","x":1,"y":3,"facing":"E"}),
            json!({"op":"place","kind":"conveyor","x":2,"y":3,"facing":"N"}),
            json!({"op":"rotate","x":2,"y":3}),
            json!({"op":"remove","x":10,"y":3}),
            json!({"op":"advance","ticks":60}),
        ];
        let mut a = build_game();
        let mut b = build_game();
        for op in operations {
            assert_eq!(command(&mut a, op.clone()), command(&mut b, op));
            assert_eq!(status(&a), status(&b));
        }
        assert_eq!(state_value(&a)["tick"], 60);
        assert_eq!(
            image_checksum(&render_image(a.world()).unwrap()),
            image_checksum(&render_image(b.world()).unwrap())
        );
        command(
            &mut a,
            json!({"op":"select","kind":"processor","facing":"W"}),
        )
        .unwrap();
        camera(&mut a, 100., -20., 2.).unwrap();
        pointer(&mut a, 200., 100., "hover").unwrap();
        command(&mut a, json!({"op":"restart"})).unwrap();
        let mut actual = state_value(&a);
        let mut initial = state_value(&build_game());
        actual.as_object_mut().unwrap().remove("frame");
        initial.as_object_mut().unwrap().remove("frame");
        assert_eq!(actual, initial);
        command(
            &mut a,
            json!({"op":"place","kind":"conveyor","x":2,"y":3,"facing":"S"}),
        )
        .unwrap();
        assert_eq!(state_value(&a)["structures"][0]["facing"], "S");
    }
    #[test]
    fn restart_clears_even_input_pending_before_first_tick() {
        let mut app = build_game();
        let mut input = InteractiveInput::for_app(&app);
        input.set_action(&app, "right", true).unwrap();
        command(&mut app, json!({"op":"restart"})).unwrap();
        input.tick(&mut app);
        assert_eq!(
            state_value(&app)["camera"],
            json!({"x":0.,"y":0.,"zoom":1.})
        );
        input.set_action(&app, "right", true).unwrap();
        input.tick(&mut app);
        assert_eq!(state_value(&app)["camera"]["x"], -4.);
        command(&mut app, json!({"op":"restart"})).unwrap();
        input.tick(&mut app);
        assert_eq!(state_value(&app)["camera"]["x"], 0.);
    }
    #[test]
    fn restart_preserves_new_events_before_next_tick() {
        let mut app = build_game();
        let mut input = InteractiveInput::for_app(&app);
        input.set_action(&app, "down", true).unwrap();
        restart(&mut app);
        input.set_action(&app, "right", true).unwrap();
        input.tick(&mut app);
        assert_eq!(
            state_value(&app)["camera"],
            json!({"x":-4.,"y":0.,"zoom":1.})
        );
        restart(&mut app);
        let mut fresh = InteractiveInput::for_app(&app);
        fresh.set_action(&app, "right", true).unwrap();
        fresh.tick(&mut app);
        assert_eq!(
            state_value(&app)["camera"],
            json!({"x":-4.,"y":0.,"zoom":1.})
        );
    }
    #[test]
    fn oversized_rejected_operation_does_not_expand_history() {
        let mut app = build_game();
        let before = status(&app);
        assert!(
            player_command(&mut app, &"x".repeat(100000))
                .unwrap_err()
                .starts_with("OPERATION_SIZE_LIMIT")
        );
        assert_eq!(status(&app), before);
        assert!(
            app.world().resource::<Recording>().unwrap().records[0]
                .to_string()
                .len()
                < 200
        );
    }
    #[test]
    fn recording_is_bounded_and_malformed_operations_are_recorded() {
        let mut app = build_game();
        for _ in 0..300 {
            assert!(player_command(&mut app, "{").is_err());
        }
        let r = app.world().resource::<Recording>().unwrap();
        assert_eq!(r.records.len(), 256);
        assert_eq!(r.dropped, 44);
        assert_eq!(r.records[0]["result"]["ok"], false);
    }
}
