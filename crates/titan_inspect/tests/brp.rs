//! Uses BRP's real request mailbox and dispatcher without sockets or a GPU.
use bevy_app::App;
use bevy_ecs::{
    prelude::*,
    schedule::{Schedule, ScheduleLabel, Schedules},
};
use bevy_remote::{
    error_codes, BrpMessage, BrpResult, BrpSender, RemoteMethodSystemId, RemoteMethods,
    RemotePlugin,
};
use serde_json::{json, Value};
use titan_inspect::{schedules::observe_schedule, InspectPlugin};

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct Demo;
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct Cold;
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
enum Sets {
    Outer,
    Inner,
    Empty,
}
#[derive(Resource, Default)]
struct Counter(u32);
#[derive(Component)]
struct Position;

fn a(mut counter: ResMut<Counter>) {
    counter.0 += 1;
}
fn b(_counter: ResMut<Counter>) {}
fn c() {}
fn unordered(_counter: ResMut<Counter>) {}
fn component_a(_q: Query<&mut Position>) {}
fn component_b(_q: Query<&mut Position>) {}
fn exclusive(_world: &mut World) {}
fn condition() -> bool {
    false
}
fn set_condition() -> bool {
    true
}

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<Counter>();
    app.add_systems(
        Demo,
        (
            a.before(Sets::Inner).run_if(condition),
            b.in_set(Sets::Inner),
            c.after(Sets::Empty),
            unordered,
            component_a,
            component_b,
            exclusive,
        ),
    );
    app.configure_sets(
        Demo,
        (
            Sets::Inner.in_set(Sets::Outer).before(Sets::Empty),
            Sets::Outer.run_if(set_condition),
            Sets::Empty,
        ),
    );
    app.add_systems(Cold, c);
    // Reverse order deliberately: registration must not depend on plugin order.
    app.add_plugins((InspectPlugin, RemotePlugin::default()));
    app.finish();
    app.cleanup();
    initialize(&mut app, Demo);
    app.update();
    app
}

fn initialize(app: &mut App, label: impl ScheduleLabel) {
    app.world_mut()
        .schedule_scope(label, |world, schedule| schedule.initialize(world).unwrap());
}

fn call(app: &mut App, method: &str, params: Option<Value>) -> BrpResult {
    let (sender, receiver) = async_channel::bounded(1);
    app.world()
        .resource::<BrpSender>()
        .try_send(BrpMessage {
            method: method.to_owned(),
            params,
            sender,
        })
        .unwrap();
    app.update();
    receiver
        .try_recv()
        .expect("dispatcher must send exactly one response")
}

fn inspect(app: &mut App, method: &str) -> Value {
    call(app, method, Some(json!({"schedule":"Demo"}))).unwrap()
}

fn system<'a>(response: &'a Value, suffix: &str) -> &'a Value {
    response["systems"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"].as_str().unwrap().ends_with(suffix))
        .unwrap()
}

fn contains(page: &Value, suffix: &str) -> bool {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s.as_str().unwrap().ends_with(suffix))
}

#[test]
fn schedules_order_conditions_and_conflicts_over_brp() {
    let mut app = app();
    let schedules = call(&mut app, "titan.schedules", None).unwrap();
    let demo = schedules["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Demo")
        .unwrap();
    assert_eq!(demo["system_count"], 7);
    assert_eq!(demo["status"], "initialized");
    assert!(demo["executor_kind"].is_null());
    assert_eq!(demo["executor_kind_available"], false);
    assert!(schedules["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == "Main" && s["status"] == "running"));

    let systems = inspect(&mut app, "titan.systems");
    let a = system(&systems, "::a");
    assert!(contains(&a["before"], "::b"));
    assert!(contains(&a["before"], "::c"));
    assert!(contains(&a["run_conditions"], "::condition"));
    assert!(!contains(&a["run_conditions"], "::set_condition"));
    let b = system(&systems, "::b");
    assert!(contains(&b["after"], "::a"));
    assert!(contains(&b["before"], "::c"));
    assert!(contains(&b["sets"], "Inner"));
    assert!(contains(&b["sets"], "Outer"));
    assert!(contains(&b["run_conditions"], "::set_condition"));
    assert_eq!(system(&systems, "::exclusive")["exclusive"], true);
    assert_eq!(a["exclusive"], false);

    let ambiguities = inspect(&mut app, "titan.ambiguities");
    let pairs = ambiguities["ambiguities"]["items"].as_array().unwrap();
    let pair = |a: &str, b: &str| {
        pairs.iter().find(|pair| {
            pair["systems"][0].as_str().unwrap().ends_with(a)
                && pair["systems"][1].as_str().unwrap().ends_with(b)
        })
    };
    assert!(pair("::a", "::b").is_none());
    assert!(contains(
        &pair("::a", "::unordered").unwrap()["conflicts"],
        "::Counter"
    ));
    assert!(contains(
        &pair("::component_a", "::component_b").unwrap()["conflicts"],
        "::Position"
    ));
    assert_eq!(pair("::a", "::exclusive").unwrap()["world_access"], true);
    assert_eq!(pair("::a", "::exclusive").unwrap()["world_wide"], true);
    assert_eq!(pair("::a", "::unordered").unwrap()["world_wide"], false);
    // Inspection and initialization never execute gameplay systems/conditions.
    assert_eq!(app.world().resource::<Counter>().0, 0);
}

#[test]
fn stable_across_repeated_calls_and_fresh_launches() {
    let mut first = app();
    let mut second = app();
    for method in ["titan.schedules", "titan.systems", "titan.ambiguities"] {
        let params = (method != "titan.schedules").then(|| json!({"schedule":"Demo"}));
        let expected = call(&mut first, method, params.clone()).unwrap();
        for _ in 0..3 {
            assert_eq!(expected, call(&mut first, method, params.clone()).unwrap());
            assert_eq!(expected, call(&mut second, method, params.clone()).unwrap());
        }
    }
}

#[test]
fn uninitialized_pending_and_missing_are_explicit() {
    let mut app = app();
    for method in ["titan.systems", "titan.ambiguities"] {
        let response = call(&mut app, method, Some(json!({"schedule":"Cold"}))).unwrap();
        assert_eq!(response["status"], "uninitialized");
        assert!(response[if method == "titan.systems" {
            "systems"
        } else {
            "ambiguities"
        }]
        .is_null());
        for name in ["missing", "Main", "RemoteLast"] {
            assert_eq!(
                call(&mut app, method, Some(json!({"schedule":name})))
                    .unwrap_err()
                    .code,
                error_codes::INVALID_PARAMS
            );
        }
    }
    app.add_systems(Demo, c.run_if(condition));
    let pending = inspect(&mut app, "titan.systems");
    assert_eq!(pending["status"], "pending_rebuild");
    assert!(pending["systems"].is_null());
    initialize(&mut app, Demo);
    let rebuilt = inspect(&mut app, "titan.systems");
    assert_eq!(rebuilt["systems"]["total"], 8);
    assert!(rebuilt["systems"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"].as_str().unwrap().ends_with("::c")
            && contains(&s["run_conditions"], "::condition")));
}

#[test]
fn invalid_params_are_rejected() {
    let mut app = app();
    for method in ["titan.schedules", "titan.systems", "titan.ambiguities"] {
        for limit in [
            json!(0),
            json!(257),
            json!(-1),
            json!(1.5),
            json!("1"),
            Value::Null,
        ] {
            let mut params = json!({"limit":limit});
            if method != "titan.schedules" {
                params["schedule"] = json!("Demo");
            }
            assert_eq!(
                call(&mut app, method, Some(params)).unwrap_err().code,
                error_codes::INVALID_PARAMS
            );
        }
        for params in [
            json!([]),
            json!(true),
            json!({"schedule":"Demo", "unknown":1}),
        ] {
            assert_eq!(
                call(&mut app, method, Some(params)).unwrap_err().code,
                error_codes::INVALID_PARAMS
            );
        }
    }
    for params in [
        None,
        Some(Value::Null),
        Some(json!({})),
        Some(json!({"schedule":42})),
    ] {
        assert_eq!(
            call(&mut app, "titan.systems", params).unwrap_err().code,
            error_codes::INVALID_PARAMS
        );
    }
    assert!(call(&mut app, "titan.schedules", Some(Value::Null)).is_ok());
}

#[test]
fn outer_and_nested_lists_are_explicitly_truncated() {
    let mut app = app();
    // Many duplicate instances must not disappear during name sorting/deduplication.
    for _ in 0..300 {
        app.add_systems(Demo, c);
    }
    initialize(&mut app, Demo);
    let full = inspect(&mut app, "titan.systems");
    assert_eq!(full["systems"]["total"], 307);
    assert_eq!(full["systems"]["items"].as_array().unwrap().len(), 64);
    assert_eq!(full["systems"]["truncated"], true);
    for method in ["titan.schedules", "titan.systems", "titan.ambiguities"] {
        let params = if method == "titan.schedules" {
            json!({"limit":1})
        } else {
            json!({"schedule":"Demo", "limit":1})
        };
        let response = call(&mut app, method, Some(params)).unwrap();
        let page = match method {
            "titan.schedules" => &response,
            "titan.systems" => &response["systems"],
            _ => &response["ambiguities"],
        };
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        assert_eq!(page["truncated"], true);
        if method == "titan.systems" {
            let a = system(&response, "::a");
            assert_eq!(a["before"]["total"], 2);
            assert_eq!(a["before"]["items"].as_array().unwrap().len(), 1);
            assert_eq!(a["before"]["truncated"], true);
        }
    }
}

#[test]
fn late_schedule_capture_and_uncaptured_conditions() {
    let mut app = app();
    let mut schedule = Schedule::new(Cold);
    schedule.add_systems(c.run_if(condition));
    schedule.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(schedule);
    let response = call(&mut app, "titan.systems", Some(json!({"schedule":"Cold"}))).unwrap();
    assert!(system(&response, "::c")["run_conditions"].is_null());
    let mut schedule = Schedule::new(Cold);
    schedule.add_systems(c.run_if(condition));
    observe_schedule(&mut schedule);
    schedule.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(schedule);
    let response = call(&mut app, "titan.systems", Some(json!({"schedule":"Cold"}))).unwrap();
    assert!(contains(
        &system(&response, "::c")["run_conditions"],
        "::condition"
    ));
}

#[test]
fn nested_sets_conditions_and_conflicting_types_obey_limit() {
    #[derive(Resource)]
    struct Other;
    fn writer_a(_: ResMut<Counter>, _: ResMut<Other>) {}
    fn writer_b(_: ResMut<Counter>, _: ResMut<Other>) {}
    let mut app = app();
    app.insert_resource(Other);
    let mut schedule = Schedule::new(Demo);
    schedule.add_systems((
        a.in_set(Sets::Inner)
            .run_if(condition)
            .run_if(set_condition)
            .before(b),
        b,
        writer_a,
        writer_b,
    ));
    schedule.configure_sets(Sets::Inner.in_set(Sets::Outer));
    observe_schedule(&mut schedule);
    schedule.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(schedule);
    let response = call(
        &mut app,
        "titan.systems",
        Some(json!({"schedule":"Demo","limit":1})),
    )
    .unwrap();
    let a = system(&response, "::a");
    for field in ["sets", "run_conditions"] {
        assert_eq!(a[field]["items"].as_array().unwrap().len(), 1);
        assert_eq!(a[field]["truncated"], true);
        assert!(a[field]["total"].as_u64().unwrap() >= 2);
    }
    let response = inspect(&mut app, "titan.ambiguities");
    let pair = response["ambiguities"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| pair["systems"][0].as_str().unwrap().ends_with("::writer_a"))
        .unwrap();
    assert_eq!(pair["conflicts"]["total"], 2);
    let mut schedule = Schedule::new(Demo);
    schedule.add_systems((writer_a, writer_b));
    observe_schedule(&mut schedule);
    schedule.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(schedule);
    let limited = call(
        &mut app,
        "titan.ambiguities",
        Some(json!({"schedule":"Demo","limit":1})),
    )
    .unwrap();
    let conflicts = &limited["ambiguities"]["items"][0]["conflicts"];
    assert_eq!(conflicts["total"], 2);
    assert_eq!(conflicts["items"].as_array().unwrap().len(), 1);
    assert_eq!(conflicts["truncated"], true);
}

#[test]
fn replacements_never_reuse_old_condition_capture() {
    let mut app = app();
    // Keep the old schedule alive: invalidation must not depend on it being dropped.
    let old = app
        .world_mut()
        .resource_mut::<Schedules>()
        .remove(Demo)
        .unwrap();
    let mut replacement = Schedule::new(Demo);
    replacement.add_systems(a.run_if(set_condition));
    replacement.initialize(app.world_mut()).unwrap();
    app.world_mut()
        .resource_mut::<Schedules>()
        .insert(replacement);
    let response = inspect(&mut app, "titan.systems");
    assert!(system(&response, "::a")["run_conditions"].is_null());
    drop(old);
    let response = inspect(&mut app, "titan.systems");
    assert!(system(&response, "::a")["run_conditions"].is_null());
}

#[test]
fn zero_sized_systems_cannot_witness_replacement_identity() {
    let mut app = app();
    let mut old = Schedule::new(Demo);
    old.add_systems(ApplyDeferred.run_if(condition));
    observe_schedule(&mut old);
    old.initialize(app.world_mut()).unwrap();
    let mut replacement = Schedule::new(Demo);
    replacement.add_systems(ApplyDeferred.run_if(set_condition));
    replacement.initialize(app.world_mut()).unwrap();
    app.world_mut()
        .resource_mut::<Schedules>()
        .insert(replacement);
    let response = inspect(&mut app, "titan.systems");
    assert!(response["systems"]["items"][0]["run_conditions"].is_null());
    drop(old);
}

#[test]
fn detached_rebuild_does_not_displace_active_capture() {
    let mut app = app();
    let mut detached = app
        .world_mut()
        .resource_mut::<Schedules>()
        .remove(Demo)
        .unwrap();
    let mut active = Schedule::new(Demo);
    active.add_systems(a.run_if(set_condition));
    observe_schedule(&mut active);
    active.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(active);
    detached.add_systems(c.run_if(condition));
    detached.initialize(app.world_mut()).unwrap();
    let response = inspect(&mut app, "titan.systems");
    assert!(contains(
        &system(&response, "::a")["run_conditions"],
        "::set_condition"
    ));
    assert!(!contains(
        &system(&response, "::a")["run_conditions"],
        "::condition"
    ));
}

#[test]
fn empty_child_membership_does_not_create_a_dependency() {
    let mut app = app();
    let mut schedule = Schedule::new(Demo);
    schedule.add_systems((a.before(Sets::Outer), b.in_set(Sets::Outer), c));
    schedule.configure_sets(Sets::Empty.in_set(Sets::Outer).before(c));
    observe_schedule(&mut schedule);
    schedule.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(schedule);
    let response = inspect(&mut app, "titan.systems");
    assert!(contains(&system(&response, "::a")["before"], "::b"));
    assert!(!contains(&system(&response, "::a")["before"], "::c"));
    assert!(!contains(&system(&response, "::c")["after"], "::a"));
    // If the child acquires a system, it provides the required witness and its
    // inherited declaration orders a transitively before c.
    app.world_mut()
        .resource_mut::<Schedules>()
        .get_mut(Demo)
        .unwrap()
        .add_systems(unordered.in_set(Sets::Empty));
    initialize(&mut app, Demo);
    let response = inspect(&mut app, "titan.systems");
    assert!(contains(&system(&response, "::a")["before"], "::c"));
    assert!(contains(&system(&response, "::c")["after"], "::a"));
}

#[test]
fn long_ordering_chain_is_paged_before_expanding_system_details() {
    use bevy_ecs::system::IntoSystem;
    #[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
    struct Chain(u16);
    let mut app = app();
    let mut schedule = Schedule::new(Demo);
    for i in 0..1000 {
        schedule.add_systems(
            IntoSystem::into_system(c)
                .with_name(format!("system_{i:04}"))
                .in_set(Chain(i)),
        );
        if i < 999 {
            schedule.configure_sets(Chain(i).before(Chain(i + 1)));
        }
    }
    observe_schedule(&mut schedule);
    schedule.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(schedule);
    let response = call(
        &mut app,
        "titan.systems",
        Some(json!({"schedule":"Demo","limit":1})),
    )
    .unwrap();
    assert_eq!(response["systems"]["total"], 1000);
    assert_eq!(response["systems"]["items"].as_array().unwrap().len(), 1);
    let first = &response["systems"]["items"][0];
    assert_eq!(first["name"], "system_0000");
    assert_eq!(first["before"]["total"], 999);
    assert_eq!(first["before"]["items"], json!(["system_0001"]));
    assert_eq!(first["before"]["truncated"], true);
    assert_eq!(first["after"]["total"], 0);
}

#[test]
fn large_ambiguity_list_materializes_only_the_requested_prefix() {
    use bevy_ecs::system::IntoSystem;
    let mut app = app();
    let mut schedule = Schedule::new(Demo);
    for i in 0..300 {
        schedule.add_systems(IntoSystem::into_system(a).with_name(format!("writer_{i:03}")));
    }
    observe_schedule(&mut schedule);
    schedule.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(schedule);
    let response = call(
        &mut app,
        "titan.ambiguities",
        Some(json!({"schedule":"Demo","limit":1})),
    )
    .unwrap();
    let pairs = &response["ambiguities"];
    assert_eq!(pairs["total"], 300 * 299 / 2);
    assert_eq!(pairs["items"].as_array().unwrap().len(), 1);
    assert_eq!(pairs["truncated"], true);
    assert_eq!(
        pairs["items"][0]["systems"],
        json!(["writer_000", "writer_001"])
    );
    assert!(contains(&pairs["items"][0]["conflicts"], "::Counter"));
}

#[test]
fn captures_initialized_before_inspect_finish_are_preserved() {
    use bevy_app::Plugin;
    struct EarlyObservation;
    impl Plugin for EarlyObservation {
        fn build(&self, _app: &mut App) {}
        fn finish(&self, app: &mut App) {
            observe_and_initialize(app);
        }
    }
    fn observe_and_initialize(app: &mut App) {
        app.world_mut().schedule_scope(Demo, |world, schedule| {
            observe_schedule(schedule);
            schedule.initialize(world).unwrap();
        });
    }
    // Cover both explicit app setup before finish and an earlier plugin hook.
    for earlier_hook in [false, true] {
        let mut app = App::new();
        app.init_resource::<Counter>()
            .add_systems(Demo, a.in_set(Sets::Inner).run_if(condition))
            .configure_sets(
                Demo,
                (
                    Sets::Inner.in_set(Sets::Outer),
                    Sets::Outer.run_if(set_condition),
                ),
            );
        if earlier_hook {
            app.add_plugins(EarlyObservation);
        }
        app.add_plugins((InspectPlugin, RemotePlugin::default()));
        if !earlier_hook {
            observe_and_initialize(&mut app);
        }
        app.finish();
        app.cleanup();
        app.update();
        let response = inspect(&mut app, "titan.systems");
        let a = system(&response, "::a");
        assert!(contains(&a["run_conditions"], "::condition"));
        assert!(contains(&a["run_conditions"], "::set_condition"));
        assert!(contains(&a["sets"], "Outer"));
        assert_eq!(app.world().resource::<Counter>().0, 0);
        assert_eq!(response, inspect(&mut app, "titan.systems"));
        app.add_systems(Demo, c.run_if(condition));
        initialize(&mut app, Demo);
        let response = inspect(&mut app, "titan.systems");
        assert!(contains(
            &system(&response, "::a")["run_conditions"],
            "::set_condition"
        ));
        assert!(contains(
            &system(&response, "::c")["run_conditions"],
            "::condition"
        ));
    }
}

#[test]
fn non_exclusive_entity_mut_ambiguity_is_explicitly_world_wide() {
    fn world_writer_a(_query: Query<EntityMut>) {}
    fn world_writer_b(_query: Query<EntityMut>) {}
    let mut app = app();
    let mut schedule = Schedule::new(Demo);
    schedule.add_systems((world_writer_a, world_writer_b));
    observe_schedule(&mut schedule);
    schedule.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(schedule);
    let systems = inspect(&mut app, "titan.systems");
    assert_eq!(system(&systems, "::world_writer_a")["exclusive"], false);
    assert_eq!(system(&systems, "::world_writer_b")["exclusive"], false);
    let response = inspect(&mut app, "titan.ambiguities");
    assert_eq!(response["ambiguities"]["total"], 1);
    let pair = &response["ambiguities"]["items"][0];
    assert_eq!(pair["world_wide"], true);
    assert_eq!(pair["world_access"], true);
    assert_eq!(pair["conflicts"]["total"], 0);
    assert_eq!(pair["conflicts"]["items"], json!([]));
    assert_eq!(pair["conflicts"]["truncated"], false);
}

#[test]
fn ambiguity_pairs_are_sorted_by_system_names() {
    #[derive(Resource)]
    struct A;
    #[derive(Resource)]
    struct Z;
    fn alpha(_r: ResMut<Z>) {}
    fn beta(_r: ResMut<Z>) {}
    fn zebra(_r: ResMut<A>) {}
    fn zulu(_r: ResMut<A>) {}
    let mut app = app();
    app.insert_resource(A).insert_resource(Z);
    let mut schedule = Schedule::new(Demo);
    schedule.add_systems((alpha, beta, zebra, zulu));
    observe_schedule(&mut schedule);
    schedule.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(schedule);
    let response = inspect(&mut app, "titan.ambiguities");
    let items = response["ambiguities"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items[0]["systems"][0]
        .as_str()
        .unwrap()
        .ends_with("::alpha"));
    let limited = call(
        &mut app,
        "titan.ambiguities",
        Some(json!({"schedule":"Demo","limit":1})),
    )
    .unwrap();
    assert_eq!(limited["ambiguities"]["items"][0], items[0]);
}

#[test]
fn later_system_insertion_preserves_known_conditions() {
    use bevy_ecs::{
        schedule::{
            graph::DiGraph, FlattenedDependencies, NodeId, ScheduleBuildError, ScheduleBuildPass,
            ScheduleGraph, SystemKey, SystemSetKey,
        },
        system::IntoSystem,
    };
    use bevy_platform::hash::FixedHasher;
    use indexmap::IndexSet;
    #[derive(Debug)]
    struct AddSystem;
    impl ScheduleBuildPass for AddSystem {
        type EdgeOptions = ();
        fn add_dependency(&mut self, _: NodeId, _: NodeId, _: Option<&()>) {}
        fn collapse_set(
            &mut self,
            _: SystemSetKey,
            _: &IndexSet<SystemKey, FixedHasher>,
            _: &DiGraph<NodeId>,
        ) -> impl Iterator<Item = (NodeId, NodeId)> {
            core::iter::empty()
        }
        fn build(
            &mut self,
            _: &mut World,
            graph: &mut ScheduleGraph,
            mut dependencies: FlattenedDependencies<'_>,
        ) -> Result<(), ScheduleBuildError> {
            let existing = graph.systems.iter().next().unwrap().0;
            let added = graph
                .systems
                .insert(Box::new(IntoSystem::into_system(c)), Vec::new());
            dependencies.add_edge(existing, added);
            Ok(())
        }
    }
    let mut app = app();
    let mut schedule = Schedule::new(Demo);
    schedule.add_systems(a.in_set(Sets::Outer).run_if(condition));
    schedule.configure_sets(Sets::Outer.run_if(set_condition));
    observe_schedule(&mut schedule);
    schedule.add_build_pass(AddSystem);
    schedule.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(schedule);
    let response = inspect(&mut app, "titan.systems");
    assert_eq!(response["systems"]["total"], 2);
    let a = system(&response, "::a");
    assert!(contains(&a["run_conditions"], "::condition"));
    assert!(contains(&a["run_conditions"], "::set_condition"));
    assert!(system(&response, "::c")["run_conditions"].is_null());
}

#[test]
fn observing_again_captures_condition_modifying_passes() {
    use bevy_ecs::{
        schedule::{
            graph::DiGraph, FlattenedDependencies, NodeId, ScheduleBuildError, ScheduleBuildPass,
            ScheduleGraph, SystemKey, SystemSetKey,
        },
        system::IntoSystem,
    };
    use bevy_platform::hash::FixedHasher;
    use indexmap::IndexSet;
    #[derive(Debug)]
    struct AddCondition;
    impl ScheduleBuildPass for AddCondition {
        type EdgeOptions = ();
        fn add_dependency(&mut self, _: NodeId, _: NodeId, _: Option<&()>) {}
        fn collapse_set(
            &mut self,
            _: SystemSetKey,
            _: &IndexSet<SystemKey, FixedHasher>,
            _: &DiGraph<NodeId>,
        ) -> impl Iterator<Item = (NodeId, NodeId)> {
            core::iter::empty()
        }
        fn build(
            &mut self,
            _: &mut World,
            graph: &mut ScheduleGraph,
            _: FlattenedDependencies<'_>,
        ) -> Result<(), ScheduleBuildError> {
            graph.system_sets.insert(
                Sets::Outer.intern(),
                vec![Box::new(IntoSystem::into_system(condition))],
            );
            Ok(())
        }
    }
    let mut app = app();
    let mut schedule = Schedule::new(Cold);
    schedule.add_systems(c.in_set(Sets::Outer));
    observe_schedule(&mut schedule);
    schedule.add_build_pass(AddCondition);
    observe_schedule(&mut schedule);
    schedule.initialize(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<Schedules>().insert(schedule);
    let response = call(&mut app, "titan.systems", Some(json!({"schedule":"Cold"}))).unwrap();
    assert!(contains(
        &system(&response, "::c")["run_conditions"],
        "::condition"
    ));
}

#[test]
fn direct_handlers_are_read_only_even_for_cold_schedules() {
    let mut app = app();
    // Dispatch directly without advancing any schedule, matching titan_remote's unit tests.
    let handler = *app
        .world()
        .resource::<RemoteMethods>()
        .get("titan.systems")
        .unwrap();
    let RemoteMethodSystemId::Instant(handler) = handler else {
        panic!("instant method");
    };
    app.world_mut()
        .run_system_with(handler, Some(json!({"schedule":"Cold"})))
        .unwrap()
        .unwrap();
    let schedule = app.world().resource::<Schedules>().get(Cold).unwrap();
    assert!(schedule.systems().is_err());
    assert_eq!(schedule.systems_len(), 1);
}
