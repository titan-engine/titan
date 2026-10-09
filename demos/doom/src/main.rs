//! Windowed presentation for the demo; all gameplay lives in the library.

use std::{env, error::Error, fs, path::PathBuf};

use bevy::{
    app::AppExit,
    asset::RenderAssetUsages,
    core_pipeline::tonemapping::Tonemapping,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured},
    },
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use titan_doom::{GameplayActions, GameplayPlugin, Level, PlayerState, FIXED_HZ};

#[derive(Resource, Default)]
struct Capture {
    path: Option<PathBuf>,
    frames: u32,
}

fn main() -> Result<(), Box<dyn Error>> {
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

    App::new()
        .insert_resource(level)
        .insert_resource(capture)
        .insert_resource(Time::<Fixed>::from_hz(FIXED_HZ))
        .insert_resource(ClearColor(Color::srgb(0.055, 0.065, 0.085)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Titan — industrial walk-through".into(),
                resolution: (960, 600).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(GameplayPlugin)
        .add_systems(Startup, setup)
        .add_systems(
            RunFixedMainLoop,
            human_actions.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
        )
        .add_systems(Update, (present_player, capture_frame))
        .run();
    Ok(())
}

// Input is just an adapter. Pending look survives frames with no fixed tick and
// is consumed exactly once by gameplay, even when a frame has several ticks.
fn human_actions(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut actions: ResMut<GameplayActions>,
) {
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
    if cursor.grab_mode == CursorGrabMode::Locked {
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
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.72, 0.8, 1.0),
        brightness: 450.0,
        ..default()
    });
    commands.spawn((
        DirectionalLight {
            illuminance: 3500.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, -0.5, 0.0)),
    ));
    commands.spawn((
        Camera3d::default(),
        Tonemapping::Reinhard,
        Transform::default(),
    ));
    commands.spawn((
        Text::new("WASD: walk | Arrows: aim | Click: mouse look | Esc: release | F12: screenshot"),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            left: px(16),
            bottom: px(16),
            ..default()
        },
    ));
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_child((
            Text::new("+"),
            TextFont {
                font_size: FontSize::Px(22.0),
                ..default()
            },
        ));
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

fn capture_frame(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut capture: ResMut<Capture>,
) {
    capture.frames += 1;
    if capture.frames == 120
        && let Some(path) = capture.path.take()
    {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path))
            .observe(
                |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::Success);
                },
            );
    }
    if keys.just_pressed(KeyCode::F12) {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk("titan-doom.png"));
    }
}
