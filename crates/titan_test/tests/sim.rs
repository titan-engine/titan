//! Timing, app lifecycle, inspection, and headless plugin integration tests.

use std::{any::Any, panic::AssertUnwindSafe, time::Duration};

use bevy_app::{App, FixedUpdate, Plugin, Startup, Update};
use bevy_diagnostic::FrameCount;
use bevy_ecs::prelude::*;
use bevy_state::prelude::*;
use bevy_time::{Fixed, Real, Time, Virtual};
use bevy_transform::prelude::*;
use titan_test::{ExecutorKind, Sim};

#[derive(Resource, Default)]
struct Counts {
    startup: u32,
    updates: u32,
    fixed: u32,
    deltas: Vec<Duration>,
}

fn counting_sim(dt: f64) -> Sim {
    Sim::new(|app| {
        app.init_resource::<Counts>()
            .add_systems(Startup, |mut counts: ResMut<Counts>| counts.startup += 1)
            .add_systems(Update, |time: Res<Time>, mut counts: ResMut<Counts>| {
                counts.updates += 1;
                counts.deltas.push(time.delta());
            })
            .add_systems(FixedUpdate, |mut counts: ResMut<Counts>| counts.fixed += 1);
    })
    .with_fixed_dt(dt)
}

#[test]
fn construction_does_not_run_startup_and_first_tick_advances_full_dt() {
    let mut sim = counting_sim(0.02);
    assert_eq!(sim.current_tick(), 0);
    assert_eq!(sim.resource::<Counts>().startup, 0);
    assert_eq!(sim.resource::<Counts>().updates, 0);
    assert_eq!(sim.resource::<FrameCount>().0, 0);

    sim.tick();
    assert_eq!(sim.current_tick(), 1);
    assert_eq!(sim.resource::<Counts>().startup, 1);
    assert_eq!(sim.resource::<Counts>().updates, 1);
    assert_eq!(sim.resource::<Counts>().fixed, 1);
    assert_eq!(
        sim.resource::<Counts>().deltas,
        vec![Duration::from_millis(20)]
    );
    assert_eq!(
        sim.resource::<Time<Real>>().elapsed(),
        Duration::from_millis(20)
    );
    assert_eq!(
        sim.resource::<Time<Fixed>>().timestep(),
        Duration::from_millis(20)
    );
    assert_eq!(sim.resource::<FrameCount>().0, 1);

    sim.run_ticks(0);
    assert_eq!(sim.current_tick(), 1);
    sim.run_ticks(4);
    assert_eq!(sim.current_tick(), 5);
    assert_eq!(sim.resource::<Counts>().startup, 1);
    assert_eq!(sim.resource::<Counts>().fixed, 5);
}

#[test]
fn frame_and_fixed_timestep_agree_even_above_virtual_times_default_clamp() {
    for dt in [0.001, 0.03125, 0.5, 1.0] {
        let mut sim = counting_sim(dt).with_executor_kind(ExecutorKind::SingleThreaded);
        sim.run_ticks(8);
        let expected = Duration::from_secs_f64(dt);
        assert_eq!(sim.current_tick(), 8, "dt = {dt}");
        assert_eq!(sim.resource::<Counts>().updates, 8, "dt = {dt}");
        assert_eq!(sim.resource::<Counts>().fixed, 8, "dt = {dt}");
        assert_eq!(
            sim.resource::<Counts>().deltas,
            vec![expected; 8],
            "dt = {dt}"
        );
        assert_eq!(sim.resource::<Time<Fixed>>().timestep(), expected);
        assert_eq!(sim.resource::<Time<Virtual>>().elapsed(), expected * 8);
        assert_eq!(sim.resource::<Time<Real>>().elapsed(), expected * 8);
    }
}

#[test]
fn default_timestep_also_runs_one_fixed_step_on_the_first_tick() {
    let mut sim = Sim::new(|app| {
        app.init_resource::<Counts>()
            .add_systems(FixedUpdate, |mut counts: ResMut<Counts>| counts.fixed += 1);
    });
    sim.tick();
    assert_eq!(sim.resource::<Counts>().fixed, 1);
    assert_eq!(
        sim.resource::<Time<Real>>().delta(),
        Duration::from_secs_f64(1.0 / 60.0)
    );
}

#[test]
fn invalid_timesteps_fail_instead_of_hanging_the_fixed_loop() {
    for dt in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e-20] {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            Sim::new(|_| {}).with_fixed_dt(dt);
        }));
        assert!(result.is_err(), "accepted invalid dt {dt}");
    }
}

#[derive(Resource, Default)]
struct Lifecycle {
    finished: bool,
    cleaned: bool,
    started: bool,
}

struct LifecyclePlugin;

impl Plugin for LifecyclePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Lifecycle>().add_systems(
            Startup,
            |mut lifecycle: ResMut<Lifecycle>| {
                assert!(lifecycle.finished);
                assert!(lifecycle.cleaned);
                lifecycle.started = true;
            },
        );
    }

    fn finish(&self, app: &mut App) {
        app.world_mut().resource_mut::<Lifecycle>().finished = true;
    }

    fn cleanup(&self, app: &mut App) {
        app.world_mut().resource_mut::<Lifecycle>().cleaned = true;
    }
}

#[test]
fn from_app_finishes_and_cleans_plugins_before_startup() {
    let mut app = App::new();
    app.add_plugins((bevy_time::TimePlugin, LifecyclePlugin));
    let mut sim = Sim::from_app(app).with_fixed_dt(0.02);
    assert!(!sim.resource::<Lifecycle>().started);
    sim.tick();
    assert!(sim.resource::<Lifecycle>().started);
    assert_eq!(sim.current_tick(), 1);
}

#[derive(Component, Debug, PartialEq, Eq)]
struct Score(u32);

#[derive(Component)]
struct Player;

#[test]
fn world_resource_single_and_filtered_query_support_assertions() {
    let mut sim = counting_sim(0.02);
    let player = sim.world_mut().spawn((Player, Score(7))).id();
    sim.world_mut().spawn(Score(99));
    assert!(sim.world().entities().contains(player));
    assert_eq!(sim.resource::<Counts>().updates, 0);
    assert_eq!(sim.single::<&Score, With<Player>>(), &Score(7));
    assert_eq!(sim.query::<&Score, With<Player>>(), vec![&Score(7)]);
    assert!(sim.query::<&Score, Without<Score>>().is_empty());

    let mut scores: Vec<_> = sim
        .query::<&Score, ()>()
        .into_iter()
        .map(|score| score.0)
        .collect();
    scores.sort_unstable();
    assert_eq!(scores, vec![7, 99]);
}

#[test]
fn run_until_checks_initial_condition_and_stops_at_first_matching_tick() {
    let mut sim = counting_sim(0.02);
    assert_eq!(sim.run_until(0, |_| true), 0);
    assert_eq!(sim.current_tick(), 0);
    assert_eq!(
        sim.run_until(5, |world| world.resource::<Counts>().updates == 3),
        3
    );
    assert_eq!(sim.current_tick(), 3);
    assert_eq!(
        sim.run_until(5, |world| world.resource::<Counts>().updates == 3),
        0
    );
    assert_eq!(sim.current_tick(), 3);
    assert_eq!(
        sim.run_until_named(2, "reach five updates", |world| world
            .resource::<Counts>()
            .updates
            == 5),
        2
    );
    assert_eq!(sim.current_tick(), 5);
}

fn panic_text(payload: Box<dyn Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else {
        "non-text panic".to_owned()
    }
}

#[test]
fn run_until_timeout_names_the_condition_and_tick_budget() {
    let mut sim = counting_sim(0.02);
    sim.run_ticks(2);
    sim.world_mut().spawn(Name::new("WaitingPlayer"));
    let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
        sim.run_until_named(3, "player lands on the platform", |_| false);
    }))
    .expect_err("an unmet condition must fail, not silently pass");
    let text = panic_text(panic);
    assert!(text.contains("player lands on the platform"), "{text}");
    assert!(text.contains('3'), "missing tick budget: {text}");
    assert!(text.contains("tick 5"), "missing current tick: {text}");
    assert!(
        text.contains("WaitingPlayer"),
        "missing entity context: {text}"
    );
    assert_eq!(sim.current_tick(), 5);
}

#[test]
fn single_fails_usefully_for_missing_or_ambiguous_matches() {
    let mut sim = Sim::new(|_| {});
    let missing = std::panic::catch_unwind(AssertUnwindSafe(|| {
        sim.single::<&Score, ()>();
    }))
    .expect_err("single must reject no matches");
    let text = panic_text(missing);
    assert!(text.contains("Score") && text.contains("tick 0"), "{text}");

    sim.world_mut().spawn((Score(1), Name::new("FirstPlayer")));
    sim.world_mut().spawn((Score(2), Name::new("SecondPlayer")));
    let ambiguous = std::panic::catch_unwind(AssertUnwindSafe(|| {
        sim.single::<&Score, ()>();
    }))
    .expect_err("single must reject multiple matches");
    let text = panic_text(ambiguous);
    assert!(text.contains("Score") && text.contains("tick 0"), "{text}");
    assert!(
        text.contains("FirstPlayer") && text.contains("SecondPlayer"),
        "{text}"
    );
}

#[test]
fn missing_resource_failure_includes_type_and_current_tick() {
    let mut sim = Sim::new(|_| {});
    sim.run_ticks(2);
    let missing = std::panic::catch_unwind(AssertUnwindSafe(|| {
        sim.resource::<Counts>();
    }))
    .expect_err("missing resources must not silently pass");
    let text = panic_text(missing);
    assert!(text.contains("Counts") && text.contains("tick 2"), "{text}");
}

#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
enum Phase {
    #[default]
    Ready,
    Playing,
}

#[test]
fn headless_plugins_run_state_transitions_and_transform_propagation() {
    let mut sim = Sim::new(|app| {
        app.init_state::<Phase>();
    });
    let parent = sim
        .world_mut()
        .spawn(Transform::from_xyz(2.0, 0.0, 0.0))
        .id();
    let child = sim
        .world_mut()
        .spawn((Transform::from_xyz(3.0, 0.0, 0.0), ChildOf(parent)))
        .id();
    sim.tick();
    assert_eq!(
        sim.world()
            .get::<GlobalTransform>(child)
            .unwrap()
            .translation()
            .x,
        5.0
    );
    sim.world_mut()
        .resource_mut::<NextState<Phase>>()
        .set(Phase::Playing);
    sim.tick();
    assert_eq!(sim.resource::<State<Phase>>().get(), &Phase::Playing);
}
