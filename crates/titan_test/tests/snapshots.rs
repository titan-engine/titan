//! Golden assertions run in child processes so update-mode tests never mutate
//! the parallel test runner's environment.

use bevy_ecs::prelude::*;
use bevy_reflect::{Reflect, TypePath};
use std::{
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
use titan_test::{
    titan_snapshot::{DiffConfig, EntityMatching, SnapshotConfig, TypeFilter, WorldSnapshot},
    Sim, SnapshotAssertConfig,
};

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Health {
    value: f64,
}

#[derive(Component, Reflect)]
#[reflect(Component)]
struct StableKey {
    number: u32,
}

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Noise {
    value: f64,
}

#[derive(Resource, Reflect)]
#[reflect(Resource)]
struct ResourceNoise {
    value: f64,
}

struct Golden(PathBuf);

impl Golden {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "titan-golden-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Self(directory.join("snapshots/health.json"))
    }

    fn run(&self, update: &str, options: &[(&str, &str)]) -> Output {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "golden_subprocess_worker", "--nocapture"])
            .env("TITAN_GOLDEN_TEST_PATH", &self.0)
            .env("TITAN_UPDATE_SNAPSHOTS", update);
        if update == "<unset>" {
            command.env_remove("TITAN_UPDATE_SNAPSHOTS");
        }
        for (key, value) in options {
            command.env(format!("TITAN_GOLDEN_TEST_{key}"), value);
        }
        command.output().unwrap()
    }

    fn create(&self) {
        let output = self.run("1", &[]);
        assert!(output.status.success(), "{}", output_text(&output));
    }
}

impl Drop for Golden {
    fn drop(&mut self) {
        if let Some(directory) = self.0.parent().and_then(|parent| parent.parent()) {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn option(name: &str, default: &str) -> String {
    std::env::var(format!("TITAN_GOLDEN_TEST_{name}")).unwrap_or_else(|_| default.to_owned())
}

#[test]
fn golden_subprocess_worker() {
    let Some(path) = std::env::var_os("TITAN_GOLDEN_TEST_PATH") else {
        return;
    };
    let value = option("VALUE", "100").parse::<f64>().unwrap();
    let noise = option("NOISE", "1").parse::<f64>().unwrap();
    let mut sim = Sim::new(|app| {
        app.register_type::<Health>()
            .register_type::<StableKey>()
            .register_type::<Noise>()
            .register_type::<ResourceNoise>();
        if option("SHIFT", "0") == "1" {
            app.world_mut()
                .spawn((Name::new("unrelated"), Noise { value: noise }));
        }
        let player = app
            .world_mut()
            .spawn((
                Name::new("Player"),
                Health { value },
                StableKey { number: 7 },
                Noise { value: noise },
            ))
            .id();
        if option("UNKEYED", "0") == "1" {
            app.world_mut().entity_mut(player).remove::<Name>();
        }
        if option("KEY_MISSING", "0") == "1" {
            app.world_mut().entity_mut(player).remove::<StableKey>();
        }
        if option("DUPLICATE", "0") == "1" {
            app.world_mut().spawn((
                Name::new("Player"),
                Health { value },
                StableKey { number: 7 },
            ));
        }
        app.world_mut()
            .insert_resource(ResourceNoise { value: noise });
    });
    sim.run_ticks(120);
    let mut capture = SnapshotConfig {
        components: TypeFilter::only([
            Health::type_path().into(),
            StableKey::type_path().into(),
            Noise::type_path().into(),
        ]),
        resources: TypeFilter::only([ResourceNoise::type_path().into()]),
        ..Default::default()
    };
    capture.components.deny::<Noise>();
    capture.resources.deny::<ResourceNoise>();
    let matching = match option("MATCH", "name").as_str() {
        "name" => EntityMatching::ByName,
        "key" => EntityMatching::ByComponent(StableKey::type_path().into()),
        "id" => EntityMatching::ById,
        other => panic!("unknown matcher {other}"),
    };
    let config = SnapshotAssertConfig::new(capture)
        .with_diff(DiffConfig {
            float_tolerance: option("TOLERANCE", "0").parse().unwrap(),
        })
        .with_entity_matching(matching)
        .with_entity_filter(|entity| entity.components.contains_key(Health::type_path()));
    sim.assert_snapshot(PathBuf::from(path), &config);
    assert_eq!(sim.current_tick(), 120);
}

#[test]
fn golden_matching_passes_and_changed_field_has_readable_context() {
    let golden = Golden::new();
    golden.create();
    assert!(golden.run("<unset>", &[]).status.success());
    let output = golden.run("0", &[("VALUE", "90")]);
    assert!(!output.status.success());
    let text = output_text(&output);
    for expected in [
        "golden_subprocess_worker",
        "tick 120",
        golden.0.to_str().unwrap(),
        "Health",
        "value: 100.0 -> 90.0",
        "Name(\"Player\")",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in {text}");
    }
    assert_eq!(
        std::fs::read_to_string(&golden.0)
            .unwrap()
            .matches("90.0")
            .count(),
        0
    );
}

#[test]
fn golden_missing_fails_and_only_exact_update_variable_creates_and_overwrites() {
    let golden = Golden::new();
    for update in ["<unset>", "0", "true", ""] {
        let output = golden.run(update, &[]);
        assert!(!output.status.success());
        let text = output_text(&output);
        assert!(text.contains("missing golden file"), "{text}");
        assert!(text.contains("TITAN_UPDATE_SNAPSHOTS=1"), "{text}");
        assert!(!golden.0.exists());
    }
    golden.create();
    let json = std::fs::read_to_string(&golden.0).unwrap();
    let saved: WorldSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(saved.entities.len(), 1);
    assert!(saved.resources.is_empty());
    assert!(json.contains("\n  \"entities\": {"));
    assert!(json.ends_with('\n'));
    let output = golden.run("1", &[("VALUE", "90")]);
    assert!(output.status.success(), "{}", output_text(&output));
    assert!(golden.run("0", &[("VALUE", "90")]).status.success());
    assert!(!golden.run("0", &[]).status.success());
}

#[test]
fn golden_tolerance_and_component_resource_filters_are_respected() {
    let golden = Golden::new();
    golden.create();
    let options = [
        ("VALUE", "100.001"),
        ("TOLERANCE", "0.01"),
        ("NOISE", "999"),
    ];
    let output = golden.run("0", &options);
    assert!(output.status.success(), "{}", output_text(&output));
    assert!(!golden
        .run("0", &[("VALUE", "100.1"), ("TOLERANCE", "0.01")])
        .status
        .success());
    assert!(!golden.run("0", &[("VALUE", "100.001")]).status.success());
}

#[test]
fn golden_name_and_struct_key_matching_survive_unrelated_earlier_spawns() {
    let golden = Golden::new();
    golden.create();
    for matching in ["name", "key"] {
        let output = golden.run("0", &[("SHIFT", "1"), ("MATCH", matching)]);
        assert!(output.status.success(), "{}", output_text(&output));
    }
    // ID matching deliberately remains fragile across runs.
    assert!(!golden
        .run("0", &[("SHIFT", "1"), ("MATCH", "id")])
        .status
        .success());
}

#[test]
fn golden_unkeyed_and_duplicate_entities_do_not_silently_pass() {
    let golden = Golden::new();
    golden.create();
    for options in [
        vec![("UNKEYED", "1")],
        vec![("DUPLICATE", "1")],
        vec![("KEY_MISSING", "1"), ("MATCH", "key")],
        vec![("DUPLICATE", "1"), ("MATCH", "key")],
    ] {
        let output = golden.run("0", &options);
        assert!(!output.status.success());
        let text = output_text(&output);
        assert!(text.contains("Player"), "{text}");
        assert!(text.contains("key"), "{text}");
    }
}

#[test]
fn golden_invalid_json_and_io_errors_are_not_treated_as_missing() {
    let golden = Golden::new();
    golden.create();
    std::fs::write(&golden.0, "not JSON").unwrap();
    let output = golden.run("0", &[]);
    assert!(!output.status.success());
    assert!(output_text(&output).contains("invalid golden JSON"));
    // Only an explicit update may replace malformed files.
    golden.create();
    assert!(golden.run("<unset>", &[]).status.success());
    std::fs::remove_file(&golden.0).unwrap();
    std::fs::create_dir(&golden.0).unwrap();
    let output = golden.run("0", &[]);
    assert!(!output.status.success());
    assert!(output_text(&output).contains("cannot read golden file"));
    let output = golden.run("1", &[]);
    assert!(!output.status.success());
    assert!(output_text(&output).contains("cannot write golden file"));
}
