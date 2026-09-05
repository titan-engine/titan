//! Game-owned persistence. No runtime entities, assets, UI or host clock are saved.
use std::io::{self, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use titan::{App, Entity, World};
use titan_protocol::{ErrorCode, ProtocolError};

use super::{
    Art, BitmapFont, DASH_COOLDOWN_TICKS, DASH_TICKS, DOT_SIZE, Enemy, HEIGHT, HudKind,
    ImageAssets, Outcome, Player, Position, RestartEpoch, Run, SEED, SURVIVAL_TICKS, UiNode,
    UiText, WIDTH, cancel_ui_pointer, clear_scheduled_input, sync_hud,
};

pub const MAX_SAVE_BYTES: usize = 64 * 1024;
const ENEMY_SLOTS: usize = 14;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SavedPosition {
    x: i32,
    y: i32,
}

impl From<Position> for SavedPosition {
    fn from(value: Position) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}
impl From<SavedPosition> for Position {
    fn from(value: SavedPosition) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SavedEnemy {
    position: SavedPosition,
    active: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SavedOutcome {
    Running,
    Won,
    Lost,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SavedRun {
    elapsed: u32,
    health: u32,
    spawned: u32,
    outcome: SavedOutcome,
    cooldown: u32,
    dash_remaining: u32,
    dash_cooldown: u32,
    facing: SavedPosition,
    dash_direction: SavedPosition,
    random: u32,
}

impl From<Run> for SavedRun {
    fn from(run: Run) -> Self {
        Self {
            elapsed: run.elapsed,
            health: run.health,
            spawned: run.spawned,
            outcome: match run.outcome {
                Outcome::Running => SavedOutcome::Running,
                Outcome::Won => SavedOutcome::Won,
                Outcome::Lost => SavedOutcome::Lost,
            },
            cooldown: run.cooldown,
            dash_remaining: run.dash_remaining,
            dash_cooldown: run.dash_cooldown,
            facing: SavedPosition {
                x: run.facing.0,
                y: run.facing.1,
            },
            dash_direction: SavedPosition {
                x: run.dash_direction.0,
                y: run.dash_direction.1,
            },
            random: run.random,
        }
    }
}
impl From<SavedRun> for Run {
    fn from(run: SavedRun) -> Self {
        Self {
            elapsed: run.elapsed,
            health: run.health,
            spawned: run.spawned,
            outcome: match run.outcome {
                SavedOutcome::Running => Outcome::Running,
                SavedOutcome::Won => Outcome::Won,
                SavedOutcome::Lost => Outcome::Lost,
            },
            cooldown: run.cooldown,
            dash_remaining: run.dash_remaining,
            dash_cooldown: run.dash_cooldown,
            facing: (run.facing.x, run.facing.y),
            dash_direction: (run.dash_direction.x, run.dash_direction.y),
            random: run.random,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Save {
    format_version: u32,
    game_seed: u32,
    player: SavedPosition,
    // Array order is the arena's enemy-pool order, not runtime entity IDs.
    enemies: [SavedEnemy; ENEMY_SLOTS],
    run: SavedRun,
}

fn invalid(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidValue, message)
}

fn validate(save: &Save) -> Result<(), ProtocolError> {
    if save.format_version != 1 || save.game_seed != SEED {
        return Err(invalid("unsupported arena save format or game seed"));
    }
    let bounded = |position: &SavedPosition| {
        (0..=WIDTH - DOT_SIZE).contains(&position.x)
            && (18..=HEIGHT - DOT_SIZE).contains(&position.y)
    };
    if !bounded(&save.player) || save.enemies.iter().any(|enemy| !bounded(&enemy.position)) {
        return Err(invalid("save positions must lie inside the arena"));
    }
    let run = &save.run;
    if run.elapsed > SURVIVAL_TICKS
        || run.health > 3
        || run.cooldown > 60
        || run.dash_cooldown > DASH_COOLDOWN_TICKS
        || run.dash_remaining >= DASH_TICKS
    {
        return Err(invalid("save gameplay counters exceed their bounds"));
    }
    let expected_outcome = if run.health == 0 {
        SavedOutcome::Lost
    } else if run.elapsed == SURVIVAL_TICKS {
        SavedOutcome::Won
    } else {
        SavedOutcome::Running
    };
    if run.outcome != expected_outcome
        || (run.health == 3 && run.cooldown != 0)
        || (run.health == 0 && run.cooldown != 60)
    {
        return Err(invalid(
            "save health, outcome and contact cooldown disagree",
        ));
    }
    let direction = |p: &SavedPosition| {
        (-1..=1).contains(&p.x) && (-1..=1).contains(&p.y) && (p.x != 0 || p.y != 0)
    };
    if !direction(&run.facing) || !direction(&run.dash_direction) {
        return Err(invalid("save directions must be nonzero unit-axis pairs"));
    }
    let expected_remaining = run
        .dash_cooldown
        .saturating_sub(DASH_COOLDOWN_TICKS - DASH_TICKS + 1);
    if run.dash_remaining != expected_remaining
        || (run.dash_cooldown > 0 && run.elapsed < DASH_COOLDOWN_TICKS - run.dash_cooldown + 1)
    {
        return Err(invalid("save dash phase and cooldown disagree"));
    }
    if run.spawned != run.elapsed.div_ceil(240)
        || save
            .enemies
            .iter()
            .enumerate()
            .any(|(slot, enemy)| enemy.active != (slot < run.spawned as usize))
    {
        return Err(invalid(
            "save elapsed ticks, spawn count and enemy pool disagree",
        ));
    }
    let mut expected_random = SEED;
    for _ in 0..run.spawned {
        expected_random = expected_random
            .wrapping_mul(1664525)
            .wrapping_add(1013904223);
    }
    if run.random != expected_random {
        return Err(invalid("save RNG state disagrees with spawn progress"));
    }
    if run.elapsed == 0 && *run != SavedRun::from(Run::default()) {
        return Err(invalid("a zero-tick save must have initial run state"));
    }
    Ok(())
}

/// Count serialization bytes without allocating an unbounded intermediate buffer.
fn check_size(value: &Value) -> Result<(), ProtocolError> {
    struct Limit(usize);
    impl Write for Limit {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > MAX_SAVE_BYTES - self.0 {
                return Err(io::Error::other("save exceeds 64 KiB"));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Limit(0), value)
        .map_err(|_| invalid("save exceeds the 64 KiB byte bound"))
}

struct Target {
    player: Entity,
    enemies: [Entity; ENEMY_SLOTS],
}

/// Preflight all fallible target assumptions before touching an initialized game.
fn target(world: &World) -> Result<Target, ProtocolError> {
    let error = || {
        ProtocolError::new(
            ErrorCode::Busy,
            "save/load requires an initialized arena with its player, enemy pool and UI",
        )
    };
    let players: Vec<_> = world.iter::<Player>().map(|(entity, _)| entity).collect();
    let [player] = players.as_slice() else {
        return Err(error());
    };
    let enemies: [Entity; ENEMY_SLOTS] = world
        .iter::<Enemy>()
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| error())?;
    if std::iter::once(*player)
        .chain(enemies)
        .any(|entity| world.get::<Position>(entity).is_none())
        || world.resource::<Run>().is_none()
        || world.resource::<RestartEpoch>().is_none()
        || world.resource::<Art>().is_none()
        || world.resource::<ImageAssets>().is_none()
        || world.resource::<BitmapFont>().is_none()
    {
        return Err(error());
    }
    let hud: Vec<_> = world.iter::<HudKind>().collect();
    if hud.len() != 3
        || hud.iter().any(|(entity, _)| {
            world.get::<UiNode>(*entity).is_none() || world.get::<UiText>(*entity).is_none()
        })
    {
        return Err(error());
    }
    Ok(Target {
        player: *player,
        enemies,
    })
}

/// Export bounded gameplay data from an initialized arena. Host time is excluded.
pub fn export_save(app: &App) -> Result<Value, ProtocolError> {
    export_save_world(app.world())
}

pub(crate) fn export_save_world(world: &World) -> Result<Value, ProtocolError> {
    let target = target(world)?;
    let save = Save {
        format_version: 1,
        game_seed: SEED,
        player: (*world.get::<Position>(target.player).unwrap()).into(),
        enemies: target.enemies.map(|entity| SavedEnemy {
            position: (*world.get::<Position>(entity).unwrap()).into(),
            active: world.get::<Enemy>(entity).unwrap().active,
        }),
        run: (*world.resource::<Run>().unwrap()).into(),
    };
    validate(&save)?;
    let value = serde_json::to_value(save).map_err(|error| invalid(error.to_string()))?;
    check_size(&value)?;
    Ok(value)
}

/// Validate fully, then restore gameplay in place at an exclusive safe point.
/// The caller enforces pause/control policy and resets its physical input/timing
/// when the game reset epoch changes. Existing entity and image handles remain.
pub fn load_save(app: &mut App, value: Value) -> Result<(), ProtocolError> {
    check_size(&value)?;
    let save: Save = serde_json::from_value(value)
        .map_err(|error| invalid(format!("invalid arena save: {error}")))?;
    validate(&save)?;
    let target = target(app.world())?;
    // Everything below is infallible for the validated initialized target.
    *app.world_mut().get_mut::<Position>(target.player).unwrap() = save.player.into();
    for (entity, enemy) in target.enemies.into_iter().zip(save.enemies) {
        *app.world_mut().get_mut::<Position>(entity).unwrap() = enemy.position.into();
        app.world_mut().get_mut::<Enemy>(entity).unwrap().active = enemy.active;
    }
    app.world_mut().insert_resource(Run::from(save.run));
    clear_scheduled_input(app);
    cancel_ui_pointer(app);
    let epoch = app.world_mut().resource_mut::<RestartEpoch>().unwrap();
    epoch.0 = epoch.0.wrapping_add(1);
    crate::live::begin_recording(app.world_mut());
    sync_hud(app.world_mut());
    app.refresh_extracted();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{self, Action, ScheduledInput};
    use titan::input::{ActionValue, InputTracker};
    use titan::render::RenderFrame;
    use titan::{FixedTime, Startup};

    fn ready() -> App {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        app
    }

    fn tick(app: &mut App, tracker: &mut InputTracker<Action>, actions: &[Action]) {
        app.world_mut().insert_resource(
            tracker.sample(
                actions
                    .iter()
                    .copied()
                    .map(|action| (action, ActionValue::PRESSED)),
            ),
        );
        app.advance_fixed(1);
    }

    fn checksum(app: &App) -> u64 {
        game::image_checksum(&game::render_image(app.world()).unwrap())
    }

    fn compare_continuation(mut original: App, ticks: u32) {
        let save = export_save(&original).unwrap();
        let original_frame = original.world().resource::<FixedTime>().unwrap().tick();
        let original_spawned = original.world().resource::<Run>().unwrap().spawned;
        let original_health = original.world().resource::<Run>().unwrap().health;
        let mut restored = ready();
        let entities: Vec<_> = restored.world().entities().collect();
        load_save(&mut restored, save.clone()).unwrap();
        assert_eq!(restored.world().entities().collect::<Vec<_>>(), entities);
        assert_eq!(restored.world().resource::<FixedTime>().unwrap().tick(), 0);
        assert_eq!(export_save(&restored).unwrap(), save);
        assert_eq!(checksum(&restored), checksum(&original));
        let mut original_input = InputTracker::new();
        let mut restored_input = InputTracker::new();
        for future in 1..=ticks {
            let actions: &[Action] = if future < 10 { &[Action::Right] } else { &[] };
            tick(&mut original, &mut original_input, actions);
            tick(&mut restored, &mut restored_input, actions);
            assert_eq!(
                export_save(&restored).unwrap(),
                export_save(&original).unwrap(),
                "future tick {future}"
            );
            if [1, 5, 10, 60, 120, 240, ticks].contains(&future) {
                assert_eq!(
                    checksum(&restored),
                    checksum(&original),
                    "future tick {future}"
                );
            }
        }
        assert_eq!(
            original.world().resource::<FixedTime>().unwrap().tick(),
            original_frame + u64::from(ticks)
        );
        assert_eq!(
            restored.world().resource::<FixedTime>().unwrap().tick(),
            u64::from(ticks)
        );
        let run = restored.world().resource::<Run>().unwrap();
        assert!(
            run.spawned > original_spawned,
            "continuation must exercise another RNG-driven spawn"
        );
        assert!(
            run.health < original_health,
            "continuation must exercise another contact"
        );
    }

    #[test]
    fn mid_dash_round_trip_preserves_rng_locked_motion_and_future_contact() {
        let mut app = ready();
        let mut input = InputTracker::new();
        for t in 0..239 {
            let action = if t < 30 {
                Action::Up
            } else if t < 90 {
                Action::Right
            } else if t < 150 {
                Action::Down
            } else {
                Action::Left
            };
            tick(&mut app, &mut input, &[action]);
        }
        tick(&mut app, &mut input, &[Action::Left, Action::Dash]);
        assert_eq!(app.world().resource::<Run>().unwrap().dash_remaining, 5);
        compare_continuation(app, 600);
    }

    #[test]
    fn contact_cooldown_round_trip_preserves_invulnerability_and_future_spawn() {
        let mut app = ready();
        app.advance_fixed(190);
        assert_eq!(app.world().resource::<Run>().unwrap().health, 2);
        assert_eq!(app.world().resource::<Run>().unwrap().cooldown, 60);
        compare_continuation(app, 300);
    }

    #[test]
    fn existing_world_load_preserves_host_clock_and_resets_derived_and_pending_state() {
        let mut source = ready();
        source.advance_fixed(190);
        let saved = export_save(&source).unwrap();
        let mut target = ready();
        target.advance_fixed(25);
        let host_frame = target.world().resource::<FixedTime>().unwrap().tick();
        let ids: Vec<_> = target.world().entities().collect();
        let image_count = target.world().resource::<ImageAssets>().unwrap().len();
        target
            .world_mut()
            .resource_mut::<ScheduledInput>()
            .unwrap()
            .enabled = true;
        target
            .world_mut()
            .resource_mut::<ScheduledInput>()
            .unwrap()
            .frames
            .insert(host_frame + 1, vec![(Action::Dash, ActionValue::PRESSED)]);
        game::handle_ui_pointer(&mut target, Some((8, 12)), true);
        let epoch = game::restart_epoch(&target);
        load_save(&mut target, saved.clone()).unwrap();
        assert_eq!(export_save(&target).unwrap(), saved);
        assert_eq!(
            target.world().resource::<FixedTime>().unwrap().tick(),
            host_frame
        );
        assert_eq!(game::restart_epoch(&target), epoch + 1);
        assert_eq!(target.world().entities().collect::<Vec<_>>(), ids);
        assert_eq!(
            target.world().resource::<ImageAssets>().unwrap().len(),
            image_count
        );
        assert_eq!(checksum(&target), checksum(&source));
        assert!(
            target
                .world()
                .iter::<UiText>()
                .any(|(_, text)| text.text.starts_with("HP 2"))
        );
        assert!(!target.world().resource::<ScheduledInput>().unwrap().enabled);
        assert!(
            target
                .world()
                .resource::<ScheduledInput>()
                .unwrap()
                .frames
                .is_empty()
        );
        assert!(
            game::handle_ui_pointer(&mut target, Some((8, 12)), false)
                .activated
                .is_none()
        );
        target.advance_fixed(1);
        assert_eq!(target.world().resource::<Run>().unwrap().dash_cooldown, 0);
    }

    #[test]
    fn malformed_saves_preserve_state_pixels_host_clock_and_pending_input() {
        let mut app = ready();
        app.advance_fixed(30);
        let save = export_save(&app).unwrap();
        let pixels = checksum(&app);
        let extracted = app.extracted::<RenderFrame>().unwrap().clone();
        let host_frame = app.world().resource::<FixedTime>().unwrap().tick();
        let epoch = game::restart_epoch(&app);
        {
            let pending = app.world_mut().resource_mut::<ScheduledInput>().unwrap();
            pending.enabled = true;
            pending
                .frames
                .insert(host_frame + 1, vec![(Action::Dash, ActionValue::PRESSED)]);
        }
        game::handle_ui_pointer(&mut app, Some((8, 12)), true);
        let mut invalids = Vec::new();
        for (path, value) in [
            ("/format_version", Value::from(2)),
            ("/game_seed", Value::from(0)),
            ("/player/x", Value::from(-1)),
            ("/player/y", Value::from(17)),
            ("/player/x", serde_json::json!(1.5)),
            ("/player/x", Value::Null),
            ("/run/health", Value::from(4)),
            ("/run/elapsed", Value::from(1201)),
            ("/run/cooldown", Value::from(61)),
            ("/run/cooldown", Value::from(1)),
            ("/run/dash_remaining", Value::from(6)),
            ("/run/dash_cooldown", Value::from(121)),
            ("/run/dash_remaining", Value::from(1)),
            ("/run/random", Value::from(0)),
            ("/run/spawned", Value::from(14)),
            ("/run/outcome", Value::from("won")),
            ("/run/facing/x", Value::from(9)),
            ("/run/dash_direction/x", Value::from(0)),
            ("/enemies/0/active", Value::from(false)),
            ("/enemies/0/position/x", Value::from(154)),
        ] {
            let mut invalid = save.clone();
            *invalid.pointer_mut(path).unwrap() = value;
            invalids.push(invalid);
        }
        let mut unknown = save.clone();
        unknown["extra"] = true.into();
        invalids.push(unknown);
        let mut unknown = save.clone();
        unknown["run"]["extra"] = true.into();
        invalids.push(unknown);
        let mut oversized = save.clone();
        oversized["extra"] = "x".repeat(MAX_SAVE_BYTES).into();
        invalids.push(oversized);
        let mut missing_enemy = save.clone();
        missing_enemy["enemies"].as_array_mut().unwrap().pop();
        invalids.push(missing_enemy);
        let mut missing = save.clone();
        missing.as_object_mut().unwrap().remove("run");
        invalids.push(missing);
        for invalid in invalids {
            assert_eq!(
                load_save(&mut app, invalid.clone()).unwrap_err().code,
                ErrorCode::InvalidValue,
                "{invalid}"
            );
            assert_eq!(export_save(&app).unwrap(), save);
            assert_eq!(checksum(&app), pixels);
            assert_eq!(app.extracted::<RenderFrame>().unwrap(), &extracted);
            assert_eq!(
                app.world().resource::<FixedTime>().unwrap().tick(),
                host_frame
            );
            assert_eq!(game::restart_epoch(&app), epoch);
            let pending = app.world().resource::<ScheduledInput>().unwrap();
            assert!(pending.enabled);
            assert_eq!(
                pending.frames[&(host_frame + 1)],
                [(Action::Dash, ActionValue::PRESSED)]
            );
        }
        app.advance_fixed(1);
        assert_eq!(
            app.world().resource::<Run>().unwrap().dash_cooldown,
            DASH_COOLDOWN_TICKS
        );
        assert!(
            game::handle_ui_pointer(&mut app, Some((8, 12)), false)
                .activated
                .is_some()
        );
        assert_eq!(app.world().resource::<Run>().unwrap().elapsed, 0);
    }

    #[test]
    fn fresh_initial_and_terminal_saves_round_trip_without_restarting_host_time() {
        for ticks in [0, 310] {
            let mut app = ready();
            app.advance_fixed(ticks);
            let saved = export_save(&app).unwrap();
            let mut target = ready();
            target.advance_fixed(5);
            load_save(&mut target, saved.clone()).unwrap();
            assert_eq!(export_save(&target).unwrap(), saved);
            assert_eq!(checksum(&target), checksum(&app));
            assert_eq!(target.world().resource::<FixedTime>().unwrap().tick(), 5);
        }
    }
}
