//! Acceptance tests use real asset loads and BRP dispatch, without HTTP or a GPU.
#[path = "../examples/support/asset_demo.rs"]
mod asset_demo;

use asset_demo::{call, demo_app, settle, DemoAsset, DemoHandles};
use bevy_app::App;
use bevy_asset::{AssetServer, Assets, Handle, LoadState, UntypedAssetLoadFailedEvent};
use bevy_ecs::message::Messages;
use bevy_remote::{error_codes, RemoteMethodSystemId, RemoteMethods, RemotePlugin};
use serde_json::{json, Value};
use titan_inspect::{assets::FAILURE_CAPACITY, InspectPlugin};

fn at_path<'a>(response: &'a Value, path: &str) -> &'a Value {
    response["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["path"] == path)
        .unwrap()
}

#[test]
fn assets_brp_valid_missing_and_startup_history() {
    let mut app = demo_app();
    settle(&mut app);
    let response = call(&mut app, "titan.assets", None).unwrap();
    let loaded = at_path(&response, "valid.demo");
    assert_eq!(loaded["state"], "loaded");
    assert_eq!(loaded["dependency_state"], "loaded");
    assert_eq!(loaded["recursive_dependency_state"], "loaded");
    assert!(loaded["type"].as_str().unwrap().ends_with("::DemoAsset"));
    assert!(loaded["error"].is_null());
    assert_eq!(loaded["dependencies"]["total"], 0);
    assert_eq!(loaded["dependency_chain_complete"], true);
    let failed = at_path(&response, "missing.demo");
    assert_eq!(failed["state"], "failed");
    assert_eq!(failed["dependency_state"], "failed");
    assert_eq!(failed["recursive_dependency_state"], "failed");
    assert_eq!(failed["dependency_error"], failed["error"]);
    assert_eq!(failed["recursive_dependency_error"], failed["error"]);
    assert!(failed["error"].as_str().unwrap().contains("missing.demo"));
    assert!(failed["dependencies"].is_null());
    assert_eq!(failed["dependency_chain_complete"], false);
    assert_eq!(response["total"], 4);
    let failures = call(&mut app, "titan.asset_failures", None).unwrap();
    assert_eq!(failures["total"], 1);
    assert_eq!(failures["items"][0]["path"], "missing.demo");
    assert_eq!(failures["items"][0]["type"], failed["type"]);
    assert_eq!(failures["items"][0]["error"], failed["error"]);
    // Drop all handles: history must outlive both short-lived messages and assets.
    app.world_mut().remove_resource::<DemoHandles>();
    for _ in 0..300 {
        app.update();
    }
    assert!(app
        .world()
        .resource::<Messages<UntypedAssetLoadFailedEvent>>()
        .is_empty());
    assert_eq!(
        failures,
        call(&mut app, "titan.asset_failures", None).unwrap()
    );
}

#[test]
fn assets_brp_reports_failed_direct_and_transitive_dependencies() {
    let mut app = demo_app();
    settle(&mut app);
    let response = call(&mut app, "titan.assets", None).unwrap();
    let missing = at_path(&response, "missing.demo");
    let parent = at_path(&response, "parent.demo");
    // The parent contents loaded; only its dependencies failed.
    assert_eq!(parent["state"], "loaded");
    assert_eq!(parent["dependency_state"], "failed");
    assert_eq!(parent["recursive_dependency_state"], "failed");
    assert_eq!(parent["dependency_error"], missing["error"]);
    assert_eq!(parent["dependencies"]["items"][0]["id"], missing["id"]);
    assert_eq!(parent["dependencies"]["items"][0]["state"], "failed");
    let scene = at_path(&response, "scene.demo");
    assert_eq!(scene["dependency_state"], "loaded");
    assert_eq!(scene["recursive_dependency_state"], "failed");
    let chain = &scene["dependency_chain"];
    assert_eq!(chain["total"], 2);
    assert_eq!(chain["items"][0]["parent_id"], scene["id"]);
    assert_eq!(chain["items"][0]["id"], parent["id"]);
    assert_eq!(chain["items"][1]["parent_id"], parent["id"]);
    assert_eq!(chain["items"][1]["id"], missing["id"]);
    assert_eq!(chain["items"][1]["error"], missing["error"]);
    assert_eq!(scene["dependency_chain_complete"], false);
    let limited = call(
        &mut app,
        "titan.assets",
        Some(json!({"path_prefix":"scene", "limit":1})),
    )
    .unwrap();
    assert_eq!(limited["items"][0]["dependency_chain"]["total"], 2);
    assert_eq!(
        limited["items"][0]["dependency_chain"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(limited["items"][0]["dependency_chain"]["truncated"], true);
}

#[test]
fn assets_brp_filters_and_truncates_after_filtering() {
    let mut app = demo_app();
    settle(&mut app);
    let all = call(&mut app, "titan.assets", None).unwrap();
    let asset_type = at_path(&all, "valid.demo")["type"].clone();
    let failed = call(
        &mut app,
        "titan.assets",
        Some(json!({"type":asset_type, "state":"failed", "path_prefix":"miss", "limit":1})),
    )
    .unwrap();
    assert_eq!(failed["total"], 1);
    assert_eq!(failed["truncated"], false);
    assert_eq!(failed["items"][0]["path"], "missing.demo");
    let limited = call(&mut app, "titan.assets", Some(json!({"limit":2}))).unwrap();
    assert_eq!(limited["total"], 4);
    assert_eq!(limited["truncated"], true);
    assert_eq!(
        limited["items"],
        json!(all["items"].as_array().unwrap()[..2])
    );
    for params in [
        json!({"type":"unknown"}),
        json!({"path_prefix":"unknown"}),
        json!({"state":"loading"}),
    ] {
        assert_eq!(
            call(&mut app, "titan.assets", Some(params)).unwrap()["total"],
            0
        );
    }
    assert_eq!(
        call(
            &mut app,
            "titan.asset_failures",
            Some(json!({"path_prefix":"unknown"}))
        )
        .unwrap()["total"],
        0
    );
    assert_eq!(
        call(
            &mut app,
            "titan.asset_failures",
            Some(json!({"type":"unknown"}))
        )
        .unwrap()["total"],
        0
    );
}

#[test]
fn assets_brp_in_memory_uuid_assets_and_cycles_are_bounded() {
    let mut app = demo_app();
    settle(&mut app);
    let mut assets = app.world_mut().resource_mut::<Assets<DemoAsset>>();
    let a = assets.reserve_handle();
    let b = assets.reserve_handle();
    assets
        .insert(
            a.id(),
            DemoAsset {
                children: vec![b.clone(), b.clone()],
            },
        )
        .unwrap();
    assets
        .insert(
            b.id(),
            DemoAsset {
                children: vec![a.clone()],
            },
        )
        .unwrap();
    assets
        .insert(
            Handle::<DemoAsset>::default().id(),
            DemoAsset { children: vec![] },
        )
        .unwrap();
    let result = call(&mut app, "titan.assets", None).unwrap();
    let memory: Vec<_> = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["path"].is_null())
        .collect();
    assert_eq!(memory.len(), 3);
    assert!(memory
        .iter()
        .all(|item| item["state"] == "loaded" && item["server_managed"] == false));
    assert!(memory.iter().all(|item| item["dependency_state"].is_null()));
    assert!(memory
        .iter()
        .any(|item| item["id"].as_str().unwrap().contains(":uuid:")));
    let cycle = memory
        .iter()
        .find(|item| item["dependencies"]["total"] == 1)
        .unwrap();
    assert_eq!(cycle["dependency_chain"]["total"], 2);
    assert_eq!(cycle["dependency_chain_complete"], true);
    let with_prefix = call(&mut app, "titan.assets", Some(json!({"path_prefix":""}))).unwrap();
    assert_eq!(with_prefix["total"], 4);
}

#[test]
fn assets_brp_history_ring_retains_newest_and_reports_eviction() {
    let mut app = demo_app();
    settle(&mut app);
    let id = app.world().resource::<DemoHandles>().0[1].id().untyped();
    let LoadState::Failed(error) = app.world().resource::<AssetServer>().load_state(id) else {
        panic!("missing asset failed");
    };
    for i in 0..FAILURE_CAPACITY + 2 {
        app.world_mut().write_message(UntypedAssetLoadFailedEvent {
            id,
            path: format!("missing_{i:03}.demo").into(),
            error: error.as_ref().clone(),
        });
    }
    app.update();
    let response = call(&mut app, "titan.asset_failures", Some(json!({"limit":2}))).unwrap();
    assert_eq!(response["capacity"], FAILURE_CAPACITY);
    assert_eq!(response["total"], FAILURE_CAPACITY);
    assert_eq!(response["dropped"], 3);
    assert_eq!(response["history_truncated"], true);
    assert_eq!(response["truncated"], true);
    assert_eq!(
        response["items"][0]["path"],
        format!("missing_{:03}.demo", FAILURE_CAPACITY + 1)
    );
    assert_eq!(response["items"][0]["sequence"], FAILURE_CAPACITY + 3);
    let filtered = call(
        &mut app,
        "titan.asset_failures",
        Some(json!({"path_prefix":"missing_25", "limit":1})),
    )
    .unwrap();
    assert_eq!(filtered["total"], 8);
    assert_eq!(filtered["items"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["truncated"], true);
    assert_eq!(filtered["dropped"], 3);
}

#[test]
fn assets_brp_invalid_params_and_no_asset_plugin() {
    let mut app = App::new();
    app.add_plugins((InspectPlugin, RemotePlugin::default()));
    app.finish();
    app.cleanup();
    app.update();
    for method in ["titan.assets", "titan.asset_failures"] {
        assert_eq!(call(&mut app, method, None).unwrap()["total"], 0);
        for params in [
            json!({"limit":0}),
            json!({"limit":257}),
            json!({"limit":1.5}),
            json!({"limit":-1}),
            json!({"unknown":true}),
            json!([]),
            json!({"type":3}),
            json!({"path_prefix":false}),
        ] {
            assert_eq!(
                call(&mut app, method, Some(params)).unwrap_err().code,
                error_codes::INVALID_PARAMS
            );
        }
    }
    for state in ["bad", "FAILED"] {
        assert_eq!(
            call(&mut app, "titan.assets", Some(json!({"state":state})))
                .unwrap_err()
                .code,
            error_codes::INVALID_PARAMS
        );
    }
    assert_eq!(
        call(
            &mut app,
            "titan.asset_failures",
            Some(json!({"state":"failed"}))
        )
        .unwrap_err()
        .code,
        error_codes::INVALID_PARAMS
    );
}

#[test]
fn assets_brp_pathless_async_failure_preserves_null_path_and_prefix_semantics() {
    use core::time::Duration;
    use std::time::Instant;

    let mut app = demo_app();
    settle(&mut app);
    let handle = app
        .world()
        .resource::<AssetServer>()
        .add_async(async { Err::<DemoAsset, _>(std::io::Error::other("pathless async failure")) });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app
        .world()
        .resource::<AssetServer>()
        .load_state(handle.id())
        .is_failed()
    {
        assert!(Instant::now() < deadline, "async failure did not settle");
        app.update();
        std::thread::sleep(Duration::from_millis(1));
    }
    let assets = call(&mut app, "titan.assets", Some(json!({"state":"failed"}))).unwrap();
    let pathless = assets["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["path"].is_null())
        .unwrap();
    assert_eq!(pathless["state"], "failed");
    assert!(pathless["error"]
        .as_str()
        .unwrap()
        .contains("pathless async failure"));
    let failures = call(&mut app, "titan.asset_failures", None).unwrap();
    let failure = failures["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == pathless["id"])
        .unwrap()
        .clone();
    assert!(failure["path"].is_null());
    assert_eq!(failure["type"], pathless["type"]);
    assert_eq!(failure["error"], pathless["error"]);
    for method in ["titan.assets", "titan.asset_failures"] {
        let filtered = call(&mut app, method, Some(json!({"path_prefix":""}))).unwrap();
        assert!(filtered["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| !item["path"].is_null() && item["id"] != pathless["id"]));
    }
    drop(handle);
    for _ in 0..300 {
        app.update();
    }
    let retained = call(&mut app, "titan.asset_failures", None).unwrap();
    assert!(retained["items"].as_array().unwrap().contains(&failure));
    let filtered = call(
        &mut app,
        "titan.asset_failures",
        Some(json!({"path_prefix":""})),
    )
    .unwrap();
    assert_eq!(filtered["total"], 1);
    assert_eq!(filtered["items"][0]["path"], "missing.demo");
}

#[test]
fn assets_brp_loading_is_visible_without_polling_or_loading() {
    let mut app = demo_app();
    // Read directly before updating the app so even fast IO cannot change the state.
    let server = app.world().resource::<AssetServer>();
    let loading: Handle<DemoAsset> = server.load("valid.demo");
    let before = server.asset_ids();
    let method = *app
        .world()
        .resource::<RemoteMethods>()
        .get("titan.assets")
        .unwrap();
    let RemoteMethodSystemId::Instant(method) = method else {
        panic!("instant handler")
    };
    let result = app
        .world_mut()
        .run_system_with(method, Some(json!({"state":"loading"})))
        .unwrap()
        .unwrap();
    assert_eq!(at_path(&result, "valid.demo")["state"], "loading");
    let server = app.world().resource::<AssetServer>();
    assert_eq!(before, server.asset_ids());
    assert!(server.get_load_state(loading.id()).unwrap().is_loading());
    assert_eq!(app.world().resource::<Assets<DemoAsset>>().len(), 0);
}
