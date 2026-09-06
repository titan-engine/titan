use super::*;
use titan_protocol::{
    EntityQuery, PageRequest, Request, RequestEnvelope, Response, ResponseOutcome,
};

fn request(app: &mut App, inspector: &mut Inspector, request: Request) -> Response {
    match inspector
        .handle(app, &RequestEnvelope::new("compatibility", request))
        .outcome
    {
        ResponseOutcome::Success { response } => response,
        outcome => panic!("unexpected response: {outcome:?}"),
    }
}

#[test]
fn old_host_component_keys_survive_extraction_and_game_transitions() {
    for module in [
        "procedural_rpg::game",
        "play_rpg::game",
        "replay_rpg::game",
        "titan_browser::game",
        "offscreen::game",
    ] {
        let mut app = build_game();
        app.update_schedule(Startup);
        let mut config = InspectionConfig::controlled("compatibility", "rpg");
        config.mutation_enabled = true;
        let mut inspector =
            inspector_with_capture(config, |_| unreachable!("capture not requested"));
        register_legacy_component_names(&mut inspector, module);
        let position = format!("{module}::Position");
        let expected = [
            "Position",
            "Player",
            "Shard",
            "Shrine",
            "ActiveShrine",
            "QuestHud",
            "journal::JournalNode",
        ];
        let names: std::collections::BTreeSet<_> = app
            .world()
            .component_metadata()
            .iter()
            .map(|metadata| inspector.component_name(metadata.type_name).to_owned())
            .collect();
        for suffix in expected.into_iter().filter(|name| *name != "ActiveShrine") {
            assert!(
                names.contains(&format!("{module}::{suffix}")),
                "{module}: {names:?}"
            );
        }
        assert!(!names.iter().any(|name| name.starts_with("titan_rpg::")));
        let Response::Entities(page) = request(
            &mut app,
            &mut inspector,
            Request::Entities {
                query: EntityQuery {
                    name: Some("player".into()),
                    with_components: vec![position.clone()],
                },
                page: PageRequest::default(),
            },
        ) else {
            panic!("entity page")
        };
        assert_eq!(page.entities.len(), 1);
        let player = page.entities[0].id;
        let Response::Entity(details) =
            request(&mut app, &mut inspector, Request::Entity { entity: player })
        else {
            panic!("entity")
        };
        assert!(details.components.contains_key(&position));
        assert!(details.component_fields[&position]["x"].writable);
        let original_x = details.components[&position]["x"].clone();
        request(
            &mut app,
            &mut inspector,
            Request::SetField {
                entity: player,
                component: position.clone(),
                field: "x".into(),
                value: original_x,
            },
        );
        replay(&mut app, &recorded_walk());
        assert_eq!(
            image_checksum(&render_image(app.world()).unwrap()),
            0xf7a298f62ad75c1c
        );
        let Response::Entities(page) = request(
            &mut app,
            &mut inspector,
            Request::Entities {
                query: EntityQuery {
                    name: None,
                    with_components: vec![format!("{module}::ActiveShrine")],
                },
                page: PageRequest::default(),
            },
        ) else {
            panic!("entity page")
        };
        assert_eq!(page.entities.len(), 1);
    }
}
