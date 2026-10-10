//! Windowed presentation for the demo; all gameplay lives in the library.

use core::sync::atomic::{AtomicBool, Ordering};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use bevy::{
    app::AppExit,
    asset::RenderAssetUsages,
    core_pipeline::tonemapping::Tonemapping,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    input::mouse::AccumulatedMouseMotion,
    platform::sync::Arc,
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured},
    },
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use titan_doom::{
    CombatState, EnemyState, GamePhase, GameplayActions, GameplayOutcome, GameplayPlugin, Level,
    ObjectKind, PlayerState, FIXED_HZ,
};

#[derive(Component)]
struct ObjectVisual {
    id: String,
    kind: ObjectKind,
}

#[derive(Resource)]
struct EnemyMaterials {
    idle: Handle<StandardMaterial>,
    chase: Handle<StandardMaterial>,
    attack: Handle<StandardMaterial>,
    dead: Handle<StandardMaterial>,
}

#[derive(Component, Clone, Default)]
struct CombatHud;

#[derive(Component, Clone, Default)]
struct FeedbackHud;

#[derive(Component, Clone, Default)]
struct PhaseHud;

#[derive(Component, Clone, Default)]
struct WeaponFlash;

#[derive(Resource, Default)]
struct HudFeedback {
    last_event_tick: Option<u64>,
    last_player_tick: u64,
    remaining: f32,
    priority: u8,
    message: String,
}

#[derive(Resource, Default)]
struct Capture {
    path: Option<PathBuf>,
    frames: u32,
    saved: Arc<AtomicBool>,
}

fn main() -> Result<AppExit, Box<dyn Error>> {
    let mut level = Level::demo();
    let mut capture = Capture::default();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--level" => {
                let path = args.next().ok_or("--level requires a RON file path")?;
                level = Level::parse(&fs::read_to_string(path)?)?;
            }
            "--capture" => {
                capture.path = Some(args.next().ok_or("--capture requires a PNG path")?.into());
            }
            _ => {
                return Err(format!(
                    "unknown argument {arg:?}; use --level FILE or --capture FILE.png"
                )
                .into())
            }
        }
    }

    // App::run transfers the world to the runner. Keep an independent completion
    // signal so every exit path, including an early window close, is checked.
    let capture_saved = capture.path.as_ref().map(|_| Arc::clone(&capture.saved));
    let exit = App::new()
        .insert_resource(level)
        .insert_resource(capture)
        .init_resource::<HudFeedback>()
        .insert_resource(Time::<Fixed>::from_hz(FIXED_HZ))
        .insert_resource(ClearColor(Color::srgb(0.055, 0.065, 0.085)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Titan — industrial combat demo".into(),
                resolution: (960, 600).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(GameplayPlugin)
        .add_systems(Startup, (setup, presentation_scene.spawn()))
        .add_systems(FixedPostUpdate, remember_outcome)
        .add_systems(
            RunFixedMainLoop,
            human_actions.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
        )
        .add_systems(
            Update,
            (present_player, present_objects, present_hud, capture_frame),
        )
        .run();
    Ok(checked_exit(
        exit,
        capture_saved.map(|saved| saved.load(Ordering::Relaxed)),
    ))
}

/// A requested capture must have saved its image, regardless of how the app exited.
fn checked_exit(exit: AppExit, capture_saved: Option<bool>) -> AppExit {
    if exit.is_success() && capture_saved == Some(false) {
        error!("Automatic capture ended before an image was saved");
        AppExit::error()
    } else {
        exit
    }
}

// Input is just an adapter. Pending look and one-shots survive frames with no
// fixed tick, and gameplay consumes them exactly once even in multi-tick frames.
fn human_actions(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut actions: ResMut<GameplayActions>,
) {
    let was_locked = cursor.grab_mode == CursorGrabMode::Locked;
    if keys.just_pressed(KeyCode::Escape) || !window.focused {
        cursor.visible = true;
        cursor.grab_mode = CursorGrabMode::None;
    } else if mouse.just_pressed(MouseButton::Left) {
        cursor.visible = false;
        cursor.grab_mode = CursorGrabMode::Locked;
    }
    if !window.focused {
        *actions = GameplayActions::default();
        return;
    }
    let axis =
        |positive, negative| f32::from(keys.pressed(positive)) - f32::from(keys.pressed(negative));
    actions.movement = Vec2::new(
        axis(KeyCode::KeyD, KeyCode::KeyA),
        axis(KeyCode::KeyW, KeyCode::KeyS),
    );
    actions.look_delta += Vec2::new(
        axis(KeyCode::ArrowLeft, KeyCode::ArrowRight),
        axis(KeyCode::ArrowUp, KeyCode::ArrowDown),
    ) * (1.8 * time.delta_secs());
    // OR accumulation preserves one-shots on frames without a fixed tick.
    actions.interact |= keys.just_pressed(KeyCode::KeyE);
    actions.restart |= keys.just_pressed(KeyCode::KeyR);
    let mouse_active = was_locked && cursor.grab_mode == CursorGrabMode::Locked;
    actions.fire =
        keys.pressed(KeyCode::Space) || (mouse_active && mouse.pressed(MouseButton::Left));
    // The activation click must not fire or consume pre-capture pointer motion.
    if mouse_active {
        actions.look_delta -= motion.delta * 0.0025;
    }
}

fn setup(
    mut commands: Commands,
    level: Res<Level>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let wall_texture = images.add(placeholder_texture(true));
    let floor_texture = images.add(placeholder_texture(false));
    let wall = materials.add(StandardMaterial {
        base_color_texture: Some(wall_texture),
        perceptual_roughness: 1.0,
        ..default()
    });
    let floor = materials.add(StandardMaterial {
        base_color_texture: Some(floor_texture),
        perceptual_roughness: 1.0,
        ..default()
    });
    let wall_mesh = meshes.add(Cuboid::new(1.0, 2.6, 1.0));
    let floor_mesh = meshes.add(Plane3d::default().mesh().size(1.0, 1.0));
    for z in 0..level.height() {
        for x in 0..level.width() {
            let position = Vec3::new(x as f32 + 0.5, 0.0, z as f32 + 0.5);
            if level.is_wall(x as i32, z as i32) {
                commands.spawn((
                    Mesh3d(wall_mesh.clone()),
                    MeshMaterial3d(wall.clone()),
                    Transform::from_translation(position + Vec3::Y * 1.3),
                ));
            } else {
                commands.spawn((
                    Mesh3d(floor_mesh.clone()),
                    MeshMaterial3d(floor.clone()),
                    Transform::from_translation(position),
                ));
            }
        }
    }
    spawn_object_visuals(
        &mut commands,
        &level,
        &mut meshes,
        &mut materials,
        &mut images,
    );
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.72, 0.8, 1.0),
        brightness: 450.0,
        ..default()
    });
}

fn spawn_object_visuals(
    commands: &mut Commands,
    level: &Level,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    let enemy_texture = images.add(object_texture(ObjectKind::Enemy));
    let enemy_material = |color| StandardMaterial {
        base_color: color,
        base_color_texture: Some(enemy_texture.clone()),
        alpha_mode: AlphaMode::Mask(0.5),
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    let enemies = EnemyMaterials {
        idle: materials.add(enemy_material(Color::WHITE)),
        chase: materials.add(enemy_material(Color::srgb(1.0, 0.72, 0.48))),
        attack: materials.add(enemy_material(Color::srgb(1.0, 0.3, 0.3))),
        dead: materials.add(enemy_material(Color::srgb(0.32, 0.34, 0.36))),
    };
    for object in level.objects() {
        let (mesh, material, height) = match object.kind {
            ObjectKind::Spawn | ObjectKind::Marker => continue,
            ObjectKind::Enemy => (
                meshes.add(Rectangle::new(0.8, 1.3)),
                enemies.idle.clone(),
                0.65,
            ),
            ObjectKind::RedDoor => (
                meshes.add(Cuboid::new(1.0, 2.6, 1.0)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.65, 0.045, 0.035),
                    base_color_texture: Some(images.add(placeholder_texture(true))),
                    perceptual_roughness: 1.0,
                    ..default()
                }),
                1.3,
            ),
            kind => (
                meshes.add(Rectangle::new(
                    if kind == ObjectKind::Exit { 0.9 } else { 0.5 },
                    if kind == ObjectKind::Exit { 1.2 } else { 0.5 },
                )),
                materials.add(StandardMaterial {
                    base_color_texture: Some(images.add(object_texture(kind))),
                    alpha_mode: AlphaMode::Mask(0.5),
                    unlit: true,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                }),
                if kind == ObjectKind::Exit { 0.6 } else { 0.35 },
            ),
        };
        commands.spawn((
            Name::new(format!("visual:{}", object.id)),
            ObjectVisual {
                id: object.id.clone(),
                kind: object.kind,
            },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_xyz(object.position.x, height, object.position.y),
        ));
    }
    commands.insert_resource(enemies);
}

// Original pixel-art silhouettes. Alpha-masked quads provide billboards without
// downloaded textures or any game branding; each item has a distinct shape.
fn object_texture(kind: ObjectKind) -> Image {
    const SIZE: u32 = 32;
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let color = match kind {
                ObjectKind::Enemy => {
                    if (10..22).contains(&x) && (3..12).contains(&y) {
                        if (x == 13 || x == 18) && y == 7 {
                            [255, 225, 100, 255]
                        } else {
                            [140, 164, 150, 255]
                        }
                    } else if (6..26).contains(&x) && (12..23).contains(&y) {
                        if (14..18).contains(&x) {
                            [70, 80, 86, 255]
                        } else {
                            [160, 65, 45, 255]
                        }
                    } else if ((8..14).contains(&x) || (18..24).contains(&x))
                        && (23..31).contains(&y)
                    {
                        [62, 76, 85, 255]
                    } else {
                        [0, 0, 0, 0]
                    }
                }
                ObjectKind::Health => {
                    if (4..28).contains(&x) && (7..27).contains(&y) {
                        if ((13..19).contains(&x) && (10..24).contains(&y))
                            || ((9..23).contains(&x) && (14..20).contains(&y))
                        {
                            [225, 45, 45, 255]
                        } else {
                            [225, 235, 230, 255]
                        }
                    } else {
                        [0, 0, 0, 0]
                    }
                }
                ObjectKind::Ammo => {
                    if (4..28).contains(&x) && (9..28).contains(&y) {
                        if y < 13 || x % 6 < 2 {
                            [85, 65, 30, 255]
                        } else {
                            [230, 184, 65, 255]
                        }
                    } else {
                        [0, 0, 0, 0]
                    }
                }
                ObjectKind::RedKey => {
                    if ((5..17).contains(&x)
                        && (5..17).contains(&y)
                        && !((8..14).contains(&x) && (8..14).contains(&y)))
                        || ((13..18).contains(&x) && (14..29).contains(&y))
                        || ((17..24).contains(&x) && (21..25).contains(&y))
                    {
                        [255, 65, 65, 255]
                    } else {
                        [0, 0, 0, 0]
                    }
                }
                ObjectKind::Exit if (2..30).contains(&x) && (2..30).contains(&y) => {
                    // Bright green portal with an upward arrow.
                    if ((14..18).contains(&x) && (10..25).contains(&y))
                        || ((7..25).contains(&x)
                            && (6..=15).contains(&y)
                            && x.abs_diff(16) <= y - 6)
                    {
                        [230, 255, 220, 255]
                    } else if !(5..=26).contains(&x) || !(5..=26).contains(&y) {
                        [80, 255, 130, 255]
                    } else {
                        [15, 105, 60, 255]
                    }
                }
                _ => [0, 0, 0, 0],
            };
            pixels.extend_from_slice(&color);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// Fixed presentation objects, expressed as a composable Bevy Scene Notation list.
fn presentation_scene() -> impl SceneList {
    bsn_list! {
        #WorldLight
        DirectionalLight { illuminance: 3500.0 }
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, -0.5, 0.0))
        --
        #PlayerView
        Camera3d
        Tonemapping::Reinhard
        Transform
        --
        #Controls
        Text("WASD: walk | Arrows/mouse: aim | Space/held click: fire\nE: open door | R: restart | Click: capture mouse | Esc: release | F12: screenshot")
        TextFont { font_size: FontSize::Px(16.0) }
        Node {
            position_type: PositionType::Absolute,
            left: px(16),
            bottom: px(16),
        }
        --
        #CombatStats
        CombatHud
        Text("HEALTH 100   AMMO 12   RED KEY NO")
        TextFont { font_size: FontSize::Px(24.0) }
        TextColor(Color::srgb(1.0, 0.9, 0.7))
        BackgroundColor(Color::srgba(0.02, 0.025, 0.035, 0.88))
        Node {
            position_type: PositionType::Absolute,
            left: px(16),
            top: px(16),
            padding: UiRect::all(px(10)),
        }
        --
        #GameplayFeedback
        FeedbackHud
        Text("Find the red key. Open the red door with E. Reach the green exit.")
        TextFont { font_size: FontSize::Px(18.0) }
        BackgroundColor(Color::srgba(0.02, 0.025, 0.035, 0.88))
        Node {
            position_type: PositionType::Absolute,
            left: px(16),
            top: px(76),
            padding: UiRect::all(px(8)),
        }
        --
        #PhaseOverlay
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        Children [
            #PhaseMessage
            PhaseHud
            Text("")
            TextFont { font_size: FontSize::Px(32.0) }
            TextColor(Color::srgb(1.0, 0.85, 0.55))
            Node { margin: UiRect::top(px(110)) }
        ]
        --
        #WeaponOverlay
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            bottom: px(64),
            width: px(64),
            height: px(84),
        }
        Children [
            #WeaponBody
            BackgroundColor(Color::srgb(0.17, 0.21, 0.25))
            Node {
                position_type: PositionType::Absolute,
                left: px(-32),
                bottom: px(0),
                width: px(64),
                height: px(48),
            }
            --
            #WeaponBarrel
            WeaponFlash
            BackgroundColor(Color::srgb(0.38, 0.43, 0.48))
            Node {
                position_type: PositionType::Absolute,
                left: px(-12),
                bottom: px(40),
                width: px(24),
                height: px(44),
            }
        ]
        --
        #CrosshairOverlay
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        Children [
            #Crosshair
            Text("+")
            TextFont { font_size: FontSize::Px(22.0) }
        ]
    }
}

// Original procedural assets: no Doom/Freedoom assets or external downloads.
fn placeholder_texture(wall: bool) -> Image {
    const SIZE: u32 = 32;
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let color = if wall {
                let seam = y % 8 == 0 || (x + (y / 8 % 2) * 8) % 16 == 0;
                if seam {
                    [33, 40, 48, 255]
                } else {
                    [103, 119, 130, 255]
                }
            } else if x == 0 || y == 0 {
                [32, 34, 39, 255]
            } else if (x / 8 + y / 8) % 2 == 0 {
                [70, 66, 57, 255]
            } else {
                [62, 59, 52, 255]
            };
            pixels.extend_from_slice(&color);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

fn present_player(player: Res<PlayerState>, mut camera: Single<&mut Transform, With<Camera3d>>) {
    camera.translation = Vec3::new(player.position.x, 0.85, player.position.y);
    camera.rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
}

fn present_objects(
    player: Res<PlayerState>,
    combat: Res<CombatState>,
    enemies: Res<EnemyMaterials>,
    mut visuals: Query<(
        &ObjectVisual,
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    for (visual, mut transform, mut visibility, mut material) in &mut visuals {
        let Some(object) = combat.objects.iter().find(|object| object.id == visual.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let dead = visual.kind == ObjectKind::Enemy && object.enemy_state == EnemyState::Dead;
        *visibility = if object.active || dead {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        transform.translation.x = object.position.x;
        transform.translation.z = object.position.y;
        if visual.kind != ObjectKind::RedDoor {
            // Rotate only around Y, preserving an upright cylindrical billboard.
            let toward_player = player.position - object.position;
            transform.rotation =
                Quat::from_rotation_y(ops::atan2(toward_player.x, toward_player.y));
        }
        if visual.kind == ObjectKind::Enemy {
            transform.translation.y = if dead { 0.12 } else { 0.65 };
            transform.scale.y = if dead { 0.18 } else { 1.0 };
            material.0 = match object.enemy_state {
                EnemyState::Idle => enemies.idle.clone(),
                EnemyState::Chase => enemies.chase.clone(),
                EnemyState::Attack => enemies.attack.clone(),
                EnemyState::Dead => enemies.dead.clone(),
            };
        }
    }
}

fn outcome_message(outcome: &GameplayOutcome) -> (u8, &'static str) {
    match outcome {
        GameplayOutcome::ShotHit => (0, "Target hit."),
        GameplayOutcome::ShotBlocked => (0, "Shot blocked by a wall or closed door."),
        GameplayOutcome::ShotMiss => (0, "Shot missed. Aim at the sentry's chest."),
        GameplayOutcome::EmptyAmmo => (1, "Out of ammo! Find a yellow ammo box."),
        GameplayOutcome::EnemyKilled => (2, "Sentry down."),
        GameplayOutcome::PlayerDamaged => (1, "Taking damage! Move away from the sentry."),
        GameplayOutcome::PlayerDied => (3, "You died. Press R to restart."),
        GameplayOutcome::HealthCollected => (2, "Health collected."),
        GameplayOutcome::AmmoCollected => (2, "Ammo collected."),
        GameplayOutcome::KeyCollected => {
            (2, "Red key collected. Approach the red door and press E.")
        }
        GameplayOutcome::MissingKey => (2, "Door locked: find the red key first."),
        GameplayOutcome::DoorOpened => (2, "Red door opened. Reach the green exit."),
        GameplayOutcome::NoDoor => (1, "No door in reach. Move closer and face the red door."),
        GameplayOutcome::ExitReached => (3, "Exit reached! Press R to play again."),
        GameplayOutcome::Restarted => (3, "Restarted: health, ammo, pickups and enemies reset."),
    }
}

// Observe every fixed tick, not just the final tick of a presentation frame.
// This keeps key/door outcomes visible even when the frame catches up several ticks.
fn remember_outcome(
    combat: Res<CombatState>,
    player: Res<PlayerState>,
    mut status: ResMut<HudFeedback>,
) {
    if player.tick < status.last_player_tick {
        *status = HudFeedback::default();
    }
    status.last_player_tick = player.tick;
    if let Some(event) = combat
        .events
        .iter()
        .max_by_key(|event| outcome_message(&event.outcome).0)
        && status.last_event_tick != Some(event.tick)
    {
        status.last_event_tick = Some(event.tick);
        let (priority, message) = outcome_message(&event.outcome);
        if status.remaining == 0.0 || priority >= status.priority {
            status.priority = priority;
            status.message = message.into();
            status.remaining = 3.0;
        }
    }
}

fn present_hud(
    combat: Res<CombatState>,
    time: Res<Time>,
    mut stats: Single<&mut Text, (With<CombatHud>, Without<FeedbackHud>, Without<PhaseHud>)>,
    mut feedback: Single<&mut Text, (With<FeedbackHud>, Without<CombatHud>, Without<PhaseHud>)>,
    mut phase: Single<&mut Text, (With<PhaseHud>, Without<CombatHud>, Without<FeedbackHud>)>,
    mut barrel: Single<&mut BackgroundColor, With<WeaponFlash>>,
    mut status: ResMut<HudFeedback>,
) {
    stats.0 = format!(
        "HEALTH {:3}   AMMO {:3}   RED KEY {}",
        combat.health,
        combat.ammo,
        if combat.red_key { "YES" } else { "NO" },
    );
    status.remaining = (status.remaining - time.delta_secs()).max(0.0);
    feedback.0 = if status.remaining > 0.0 {
        status.message.clone()
    } else if combat.red_key {
        "Open the red door with E, then walk into the green exit.".into()
    } else {
        "Find the red key. Walk over health/ammo to collect them.".into()
    };
    phase.0 = match combat.phase {
        GamePhase::Playing => String::new(),
        GamePhase::Dead => "YOU DIED\nPress R to restart".into(),
        GamePhase::Won => "EXIT REACHED\nPress R to play again".into(),
    };
    barrel.0 = if combat.weapon_cooldown > 0 && combat.phase == GamePhase::Playing {
        Color::srgb(1.0, 0.7, 0.2)
    } else {
        Color::srgb(0.38, 0.43, 0.48)
    };
}

/// Save a captured image, preserving failures in the process exit status.
fn capture_exit(image: &Image, path: &Path) -> AppExit {
    let save = || -> Result<(), Box<dyn Error>> {
        image.clone().try_into_dynamic()?.to_rgb8().save(path)?;
        Ok(())
    };
    match save() {
        Ok(()) => {
            info!("Screenshot saved to {}", path.display());
            AppExit::Success
        }
        Err(error) => {
            error!("Cannot save screenshot to {}: {error}", path.display());
            AppExit::error()
        }
    }
}

fn capture_frame(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut capture: ResMut<Capture>,
) {
    capture.frames += 1;
    if capture.frames == 120
        && let Some(path) = capture.path.take()
    {
        commands.spawn(Screenshot::primary_window()).observe(
            move |captured: On<ScreenshotCaptured>,
                  capture: Res<Capture>,
                  mut exit: MessageWriter<AppExit>| {
                let result = capture_exit(&captured.image, &path);
                capture.saved.store(result.is_success(), Ordering::Relaxed);
                exit.write(result);
            },
        );
    }
    if keys.just_pressed(KeyCode::F12) {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk("titan-doom.png"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bsn_presentation_spawns_camera_light_and_hud_hierarchy() {
        use bevy::{app::TaskPoolPlugin, asset::AssetPlugin, scene::ScenePlugin};

        let mut app = App::new();
        app.add_plugins((
            TaskPoolPlugin::default(),
            AssetPlugin::default(),
            ScenePlugin,
        ));
        let world = app.world_mut();
        let roots = world.spawn_scene_list(presentation_scene()).unwrap();
        assert_eq!(roots.len(), 8);
        assert_eq!(
            world
                .query_filtered::<Entity, With<Camera3d>>()
                .iter(world)
                .count(),
            1
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<DirectionalLight>>()
                .iter(world)
                .count(),
            1
        );
        let overlay = roots
            .iter()
            .copied()
            .find(|entity| {
                world
                    .get::<Name>(*entity)
                    .is_some_and(|name| name.as_str() == "CrosshairOverlay")
            })
            .unwrap();
        let children = world.get::<Children>(overlay).unwrap();
        assert_eq!(children.len(), 1);
        let crosshair = children[0];
        assert_eq!(world.get::<Text>(crosshair).unwrap().0, "+");
        assert_eq!(
            world.get::<TextFont>(crosshair).unwrap().font_size,
            FontSize::Px(22.0)
        );
    }

    #[test]
    fn mouse_grab_discards_free_cursor_motion_on_activation() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<Time>()
            .init_resource::<GameplayActions>()
            .add_systems(Update, human_actions);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                CursorOptions::default(),
                PrimaryWindow,
            ))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(100.0, 50.0);
        app.update();
        assert_eq!(
            app.world().resource::<GameplayActions>().look_delta,
            Vec2::ZERO
        );
        assert_eq!(
            app.world().get::<CursorOptions>(window).unwrap().grab_mode,
            CursorGrabMode::Locked
        );
        assert!(!app.world().resource::<GameplayActions>().fire);

        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(2.0, 3.0);
        app.update();
        let look = app.world().resource::<GameplayActions>().look_delta;
        assert!(look.distance(Vec2::new(-0.005, -0.0075)) < 0.000001);
        assert!(app.world().resource::<GameplayActions>().fire);

        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert_eq!(
            app.world().resource::<GameplayActions>().look_delta,
            Vec2::ZERO
        );
        assert_eq!(
            app.world().get::<CursorOptions>(window).unwrap().grab_mode,
            CursorGrabMode::None
        );
        assert!(!app.world().resource::<GameplayActions>().fire);
    }

    #[test]
    fn hud_remembers_interactions_across_empty_ticks_and_routine_shots() {
        use titan_doom::GameplayEvent;

        let mut app = App::new();
        app.insert_resource(Level::demo())
            .add_plugins(GameplayPlugin)
            .init_resource::<HudFeedback>()
            .add_systems(FixedPostUpdate, remember_outcome);
        let event = |tick, outcome| GameplayEvent {
            tick,
            object_id: Some("red-key".into()),
            outcome,
        };
        app.world_mut().resource_mut::<PlayerState>().tick = 1;
        app.world_mut().resource_mut::<CombatState>().events =
            vec![event(1, GameplayOutcome::KeyCollected)];
        app.world_mut().run_schedule(FixedPostUpdate);
        let message = app.world().resource::<HudFeedback>().message.clone();
        assert!(message.contains("Red key collected"));

        app.world_mut().resource_mut::<CombatState>().events.clear();
        app.world_mut().run_schedule(FixedPostUpdate);
        assert_eq!(app.world().resource::<HudFeedback>().message, message);
        app.world_mut().resource_mut::<PlayerState>().tick = 2;
        app.world_mut().resource_mut::<CombatState>().events =
            vec![event(2, GameplayOutcome::ShotBlocked)];
        app.world_mut().run_schedule(FixedPostUpdate);
        assert_eq!(app.world().resource::<HudFeedback>().message, message);

        app.world_mut().resource_mut::<PlayerState>().tick = 0;
        app.world_mut().resource_mut::<CombatState>().events =
            vec![event(0, GameplayOutcome::Restarted)];
        app.world_mut().run_schedule(FixedPostUpdate);
        assert!(app
            .world()
            .resource::<HudFeedback>()
            .message
            .contains("Restarted"));
    }

    #[test]
    fn object_visuals_hide_consumed_items_and_restore_on_restart() {
        let level = Level::parse(
            r########"(
                rows: ["#######", "#.....#", "#.....#", "#.....#", "#######"],
                objects: [
                    (id: "spawn", kind: Spawn, position: (1.5, 2.5), yaw: 0.0),
                    (id: "enemy", kind: Enemy, position: (3.5, 2.5), yaw: 0.0),
                    (id: "door", kind: RedDoor, position: (5.5, 2.5), yaw: 0.0),
                    (id: "ammo", kind: Ammo, position: (2.5, 3.5), yaw: 0.0),
                ],
            )"########,
        )
        .unwrap();
        let mut app = App::new();
        app.insert_resource(level)
            .add_plugins(GameplayPlugin)
            .insert_resource(EnemyMaterials {
                idle: default(),
                chase: default(),
                attack: default(),
                dead: default(),
            })
            .add_systems(Update, present_objects);
        let objects = app.world().resource::<CombatState>().objects.clone();
        for object in objects {
            app.world_mut().spawn((
                ObjectVisual {
                    id: object.id,
                    kind: object.kind,
                },
                Transform::default(),
                Visibility::Inherited,
                MeshMaterial3d::<StandardMaterial>(default()),
            ));
        }
        for object in &mut app.world_mut().resource_mut::<CombatState>().objects {
            object.active = false;
            if object.kind == ObjectKind::Enemy {
                object.enemy_state = EnemyState::Dead;
            }
        }
        app.update();
        let world = app.world_mut();
        for (visual, transform, visibility) in world
            .query::<(&ObjectVisual, &Transform, &Visibility)>()
            .iter(world)
        {
            if visual.kind == ObjectKind::Enemy {
                assert_eq!(*visibility, Visibility::Inherited);
                assert_eq!(transform.scale.y, 0.18);
            } else {
                assert_eq!(*visibility, Visibility::Hidden);
            }
        }
        app.world_mut().resource_mut::<GameplayActions>().restart = true;
        app.world_mut().run_schedule(FixedUpdate);
        app.update();
        let world = app.world_mut();
        for (transform, visibility) in world.query::<(&Transform, &Visibility)>().iter(world) {
            assert_eq!(*visibility, Visibility::Inherited);
            assert_eq!(transform.scale.y, 1.0);
        }
    }

    #[test]
    fn keyboard_one_shots_survive_frames_without_a_gameplay_tick() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<Time>()
            .init_resource::<GameplayActions>()
            .add_systems(Update, human_actions);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ));
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::KeyE);
            keys.press(KeyCode::KeyR);
            keys.press(KeyCode::Space);
        }
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        let actions = app.world().resource::<GameplayActions>();
        assert!(actions.interact && actions.restart);
        assert!(!actions.fire, "fire is held, not a pending one-shot");
    }

    #[test]
    fn early_exit_fails_before_and_after_a_capture_request_is_submitted() {
        let mut capture = Capture {
            path: Some("capture.png".into()),
            ..default()
        };
        let saved = Arc::clone(&capture.saved);
        let observed = || Some(saved.load(Ordering::Relaxed));
        assert!(checked_exit(AppExit::Success, observed()).is_error());
        // Taking the path to submit the request must not count as completion.
        capture.path.take();
        assert!(checked_exit(AppExit::Success, observed()).is_error());
        capture.saved.store(true, Ordering::Relaxed);
        assert_eq!(checked_exit(AppExit::Success, observed()), AppExit::Success);
        assert!(checked_exit(AppExit::error(), observed()).is_error());
        assert_eq!(checked_exit(AppExit::Success, None), AppExit::Success);

        for submitted in [false, true] {
            let mut app = App::new();
            app.add_plugins(WindowPlugin::default());
            let mut capture = Capture {
                path: Some("capture.png".into()),
                ..default()
            };
            let saved = Arc::clone(&capture.saved);
            if submitted {
                capture.path.take();
            }
            app.insert_resource(capture);
            let world = app.world_mut();
            let window = world
                .query_filtered::<Entity, With<Window>>()
                .single(world)
                .unwrap();
            world.write_message(bevy::window::WindowCloseRequested { window });
            // Bevy marks the window closing, then despawns it on the next frame.
            app.update();
            app.update();
            let exit = app.should_exit().unwrap();
            assert_eq!(exit, AppExit::Success);
            assert!(checked_exit(exit, Some(saved.load(Ordering::Relaxed))).is_error());
        }
    }

    #[test]
    fn automatic_capture_reports_write_and_conversion_failures() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("capture.png");
        let image = placeholder_texture(false);
        assert_eq!(capture_exit(&image, &path), AppExit::Success);
        assert!(fs::read(&path).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(capture_exit(&image, &directory.path().join("missing/capture.png")).is_error());
        assert!(capture_exit(&image, &directory.path().join("capture.unsupported")).is_error());
        let uninitialized = Image {
            data: None,
            ..default()
        };
        assert!(capture_exit(&uninitialized, &path).is_error());
    }
}
