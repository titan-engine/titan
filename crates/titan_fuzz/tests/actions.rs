//! Action campaigns, shrinking, panic safety, and standalone RON regressions.
use bevy_app::Update;
use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use titan_fuzz::{ActionConfig, ActionFuzz, ActionRng, ActionScript, ActionTick, FuzzReport};
use titan_test::Sim;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Actions {
    movement: i32,
    jump: bool,
}

#[derive(Resource, Default)]
struct Player(i32);

fn game() -> Sim {
    Sim::new(|app| {
        app.init_resource::<Player>();
    })
}

fn apply(world: &mut World, action: &Actions) {
    // Deliberately broken: jumping lets movement ignore the wall.
    let mut player = world.resource_mut::<Player>();
    player.0 = if action.jump {
        player.0 + action.movement
    } else {
        (player.0 + action.movement).min(2)
    };
}

fn wall(world: &World) -> Result<(), String> {
    let x = world.resource::<Player>().0;
    if x > 2 {
        Err(format!("player inside wall: x={x}"))
    } else {
        Ok(())
    }
}

fn campaign(
    root: &std::path::Path,
    budget: u64,
) -> FuzzReport<ActionScript<Actions>, ActionConfig> {
    ActionFuzz::new(
        game,
        Actions::default(),
        |rng| Actions {
            movement: 3 + rng.below(8) as i32,
            jump: rng.below(2) == 0,
        },
        apply,
    )
    .invariant("outside wall", wall)
    .cases(4)
    .ticks(64)
    .seed(7)
    .max_shrink_runs(budget)
    .simplify_action(|action| {
        vec![Actions {
            movement: 3,
            ..action.clone()
        }]
    })
    .test_name("action-regression")
    .output_dir(root)
    .run()
}

#[test]
fn broken_handler_shrinks_to_readable_action_and_saved_regression_fails_identically() {
    let root = std::env::temp_dir().join(format!("titan-action-regression-{}", std::process::id()));
    let report = campaign(&root, 300);
    assert_eq!(report, campaign(&root, 300));
    let FuzzReport::Failed(failure) = &report else {
        panic!("{report}");
    };
    assert_eq!(failure.script.ticks, 1);
    assert_eq!(
        failure.script.events,
        vec![ActionTick {
            tick: 0,
            action: Actions {
                movement: 3,
                jump: true
            }
        }]
    );
    assert!(failure.original_events > failure.script.events.len());
    assert_eq!(failure.failing_tick, 0);
    assert!(failure.shrink_runs <= 300);
    assert!(report.to_string().contains("script.replay"));
    assert!(ron::ser::to_string(&report).unwrap().contains("Failed"));
    assert!(!failure.path.exists());
    assert!(std::panic::catch_unwind(|| report.assert_ok()).is_err());
    let saved = std::fs::read_to_string(&failure.path).unwrap();
    let script = ActionScript::<Actions>::from_ron(&saved).unwrap();
    assert_eq!(script, failure.script);
    let mut sim = game();
    script.replay(&mut sim, apply);
    assert_eq!(wall(sim.world()), Err(failure.message.clone()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn shrink_run_budget_includes_confirmations() {
    for budget in [0, 1, 2, 3, 7] {
        let report = campaign(std::path::Path::new("unused"), budget);
        let FuzzReport::Failed(failure) = report else {
            panic!("expected failure");
        };
        assert!(failure.shrink_runs <= budget);
        let mut sim = game();
        failure.script.replay(&mut sim, apply);
        assert!(wall(sim.world()).is_err());
    }
}

#[test]
fn missing_ticks_apply_idle_and_replay_can_resume() {
    let script = ActionScript {
        version: 1,
        ticks: 3,
        idle: 0,
        events: vec![
            ActionTick { tick: 0, action: 5 },
            ActionTick { tick: 2, action: 8 },
        ],
    };
    let mut sim = game();
    let mut prefix = script.clone();
    prefix.ticks = 2;
    prefix.events.truncate(1);
    prefix.replay(&mut sim, |world, action| {
        world.resource_mut::<Player>().0 = *action;
    });
    assert_eq!(sim.resource::<Player>().0, 0);
    script.replay(&mut sim, |world, action| {
        world.resource_mut::<Player>().0 = *action;
    });
    assert_eq!(sim.resource::<Player>().0, 8);
    assert_eq!(sim.current_tick(), 3);
}

#[test]
fn invalid_action_scripts_are_rejected_before_playback() {
    for source in [
        "(version: 2, ticks: 0, idle: 0, events: [])",
        "(version: 1, ticks: 1, idle: 0, events: [(tick: 1, action: 5)])",
        "(version: 1, ticks: 2, idle: 0, events: [(tick: 0, action: 1), (tick: 0, action: 2)])",
        "(version: 1, ticks: 2, idle: 0, events: [(tick: 1, action: 1), (tick: 0, action: 2)])",
        "(version: 1, ticks: 0, idle: 0, events: [], unknown: 0)",
    ] {
        assert!(ActionScript::<i32>::from_ron(source).is_err(), "{source}");
    }
    let mut sim = game();
    let script = ActionScript {
        version: 2,
        ticks: 1,
        idle: 0,
        events: vec![],
    };
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || script.replay(&mut sim, |_, _| {})
    ))
    .is_err());
    assert_eq!(sim.current_tick(), 0);
}

#[test]
fn factory_adapter_system_and_invariant_panics_are_caught() {
    for stage in 0..4 {
        let report = ActionFuzz::new(
            || {
                if stage == 0 {
                    panic!("factory panic");
                }
                Sim::new(|app| {
                    if stage == 2 {
                        app.add_systems(Update, || panic!("system panic"));
                    }
                })
            },
            (),
            |_| (),
            |_, _| {
                if stage == 1 {
                    panic!("adapter panic");
                }
            },
        )
        .invariant("rule", |_| {
            if stage == 3 {
                panic!("invariant panic");
            }
            Ok(())
        })
        .cases(1)
        .ticks(3)
        .run();
        let FuzzReport::Failed(failure) = report else {
            panic!("expected failure");
        };
        assert_eq!(failure.invariant, "no_panics");
        assert_eq!(failure.failing_tick, 0);
        assert_eq!(failure.ticks, 1);
    }
}

#[test]
fn opaque_panics_are_not_shrunk() {
    let report = ActionFuzz::new(game, (), |_| (), |_, _| std::panic::panic_any(42_u32))
        .cases(1)
        .ticks(4)
        .run();
    let FuzzReport::Failed(failure) = report else {
        panic!("expected failure");
    };
    assert_eq!(failure.shrink_runs, 0);
    assert_eq!(failure.script.events.len(), 4);
}

#[test]
fn original_replay_disagreement_is_flaky() {
    let calls = AtomicU64::new(0);
    let report = ActionFuzz::new(
        game,
        (),
        |_| (),
        |_, _| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("first only");
            }
        },
    )
    .cases(1)
    .ticks(3)
    .run();
    let FuzzReport::Flaky(failure) = report else {
        panic!("expected flaky");
    };
    assert_eq!(failure.shrink_runs, 0);
    assert_eq!(failure.ticks, 3);
}

#[test]
fn failed_candidate_confirmation_preserves_original_as_flaky() {
    let calls = AtomicU64::new(0);
    let report = ActionFuzz::new(game, 0, |_| 1, |_, _| {})
        .invariant("unstable", |_| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            if call < 3 {
                Err("broken".into())
            } else {
                Ok(())
            }
        })
        .cases(1)
        .ticks(4)
        .run();
    let FuzzReport::Flaky(failure) = report else {
        panic!("expected flaky");
    };
    assert_eq!(failure.ticks, 4);
    assert_eq!(failure.script.events.len(), 4);
    assert_eq!(failure.shrink_runs, 2);
}

#[test]
fn shrinking_cannot_switch_rules_or_panic_payloads() {
    let report = ActionFuzz::new(
        game,
        0,
        |_| 1,
        |world, action| world.resource_mut::<Player>().0 = *action,
    )
    .invariant("original", |world| {
        if world.resource::<Player>().0 == 1 {
            Err("original violation".into())
        } else {
            Ok(())
        }
    })
    .invariant("other", |_| Err("other violation".into()))
    .cases(1)
    .ticks(4)
    .run();
    let FuzzReport::Failed(failure) = report else {
        panic!("expected failure");
    };
    assert_eq!(failure.invariant, "original");
    assert_eq!(failure.script.events.len(), 1);
    let report = ActionFuzz::new(
        game,
        0,
        |_| 1,
        |_, action| {
            if *action == 1 {
                panic!("original panic");
            } else {
                panic!("other panic");
            }
        },
    )
    .cases(1)
    .ticks(4)
    .run();
    let FuzzReport::Failed(failure) = report else {
        panic!("expected failure");
    };
    assert_eq!(failure.message, "original panic");
    assert_eq!(failure.script.events.len(), 1);
}

#[test]
fn zero_limits_and_generation_errors_have_explicit_semantics() {
    let constructions = AtomicU64::new(0);
    let factory = || {
        constructions.fetch_add(1, Ordering::SeqCst);
        game()
    };
    let generate = |_: &mut ActionRng| -> () {
        panic!("must not generate");
    };
    ActionFuzz::new(factory, (), generate, |_, _| {})
        .cases(0)
        .run()
        .assert_ok();
    assert_eq!(constructions.load(Ordering::SeqCst), 0);
    ActionFuzz::new(factory, (), generate, |_, _| {})
        .cases(2)
        .ticks(0)
        .run()
        .assert_ok();
    assert_eq!(constructions.load(Ordering::SeqCst), 2);
    assert!(
        std::panic::catch_unwind(|| ActionFuzz::new(game, (), generate, |_, _| {})
            .cases(1)
            .ticks(1)
            .run())
        .is_err()
    );
}

#[test]
fn saved_transient_failure_stops_at_the_failing_update_even_without_shrinking() {
    for budget in [0, 1] {
        let root =
            std::env::temp_dir().join(format!("titan-transient-{}-{budget}", std::process::id()));
        let report = ActionFuzz::new(
            || {
                Sim::new(|app| {
                    app.init_resource::<Player>()
                        .add_systems(Update, |mut player: ResMut<Player>| player.0 += 1);
                })
            },
            (),
            |_| (),
            |_, _| {},
        )
        .invariant("transient", |world| {
            if world.resource::<Player>().0 == 4 {
                Err("frame four".into())
            } else {
                Ok(())
            }
        })
        .cases(1)
        .ticks(12)
        .max_shrink_runs(budget)
        .output_dir(&root)
        .run();
        let FuzzReport::Failed(failure) = &report else {
            panic!("expected failure");
        };
        assert_eq!(failure.ticks, 12); // Original provenance remains intact.
        let path = report.save_script().unwrap().unwrap();
        let script = ActionScript::<()>::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(script.ticks, 4);
        let mut sim = Sim::new(|app| {
            app.init_resource::<Player>()
                .add_systems(Update, |mut player: ResMut<Player>| player.0 += 1);
        });
        script.replay(&mut sim, |_, _| {});
        assert_eq!(sim.resource::<Player>().0, 4);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[derive(Resource, Default)]
struct Flags {
    frame: u64,
    a: bool,
    b: bool,
}

#[test]
fn final_tick_failure_spends_its_small_shrink_budget_on_deletion() {
    let report = ActionFuzz::new(
        || {
            Sim::new(|app| {
                app.init_resource::<Flags>()
                    .add_systems(Update, |mut flags: ResMut<Flags>| {
                        flags.frame += 1;
                    });
            })
        },
        0,
        |_| 1,
        |world, action| {
            if *action != 0 {
                world.resource_mut::<Flags>().a = true;
            }
        },
    )
    .invariant("final tick", |world| {
        let flags = world.resource::<Flags>();
        if flags.frame == 2 && flags.a {
            Err("action received".into())
        } else {
            Ok(())
        }
    })
    .cases(1)
    .ticks(2)
    .max_shrink_runs(2)
    .run();
    let FuzzReport::Failed(failure) = report else {
        panic!("expected failure");
    };
    assert_eq!(failure.failing_tick, 1);
    assert_eq!(failure.original_events, 2);
    assert_eq!(failure.script.events.len(), 1);
    assert_eq!(failure.shrink_runs, 2);
}

#[test]
fn moving_an_action_across_another_event_keeps_minimizing_that_action() {
    let mut rng = ActionRng::new(0, 0);
    let draws: Vec<_> = (0..12).map(|_| rng.next_u64()).collect();
    let report = ActionFuzz::new(
        || {
            Sim::new(|app| {
                app.init_resource::<Flags>()
                    .add_systems(Update, |mut flags: ResMut<Flags>| flags.frame += 1);
            })
        },
        0,
        |rng| {
            let value = rng.next_u64();
            let index = draws.iter().position(|draw| *draw == value);
            match index {
                Some(8) => 1,
                Some(10) => 2,
                _ => 0,
            }
        },
        |world, action| {
            let mut flags = world.resource_mut::<Flags>();
            if *action == 1 && flags.frame == 8 {
                flags.a = true;
            }
            if *action == 2 {
                flags.b = true;
            }
        },
    )
    .invariant("both flags", |world| {
        let flags = world.resource::<Flags>();
        if flags.a && flags.b {
            Err("both set".into())
        } else {
            Ok(())
        }
    })
    .cases(1)
    .ticks(12)
    .max_shrink_runs(500)
    .run();
    let FuzzReport::Failed(failure) = report else {
        panic!("expected failure");
    };
    assert_eq!(
        failure.script.events,
        vec![
            ActionTick { tick: 0, action: 2 },
            ActionTick { tick: 8, action: 1 }
        ]
    );
    assert_eq!(failure.ticks, 9);
}

#[derive(Debug, Clone, Deserialize)]
struct Unserializable;

impl Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("deliberate serialization error"))
    }
}

#[test]
fn serialization_errors_do_not_hide_the_gameplay_finding() {
    let root = std::env::temp_dir().join(format!("titan-serialize-error-{}", std::process::id()));
    let report = ActionFuzz::new(game, Unserializable, |_| Unserializable, |_, _| {})
        .invariant("gameplay rule", |_| Err("gameplay finding".into()))
        .cases(1)
        .ticks(1)
        .output_dir(&root)
        .run();
    let displayed = report.to_string();
    assert!(displayed.contains("gameplay rule"));
    assert!(displayed.contains("gameplay finding"));
    assert!(displayed.contains("deliberate serialization error"));
    assert!(report.save_script().is_err());
    let panic = std::panic::catch_unwind(|| report.assert_ok()).unwrap_err();
    let message = panic.downcast_ref::<String>().unwrap();
    assert!(message.contains("gameplay finding"));
    assert!(message.contains("Could not save reproduction"));
    assert!(message.contains("deliberate serialization error"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn seeded_action_rng_has_stable_independent_streams() {
    let mut rng = ActionRng::new(0, 0);
    assert_eq!(rng.next_u64(), 0xa706_dd2f_4d19_7e6f);
    let mut first = ActionRng::new(42, 3);
    let mut second = ActionRng::new(42, 3);
    for _ in 0..20 {
        assert_eq!(first.next_u64(), second.next_u64());
        assert!(first.below(5) < 5);
        second.below(5);
        assert!((0.0..1.0).contains(&first.unit_f64()));
        second.unit_f64();
    }
}
