//! Read-only player explanations, shared by every host and inspection query.
use super::*;

pub fn interface(app: &App) -> Value {
    state_value(app)
}

pub fn set_preview_action(app: &mut App, action: &str) -> Result<(), String> {
    if !["place", "rotate", "remove", "inspect"].contains(&action) {
        return Err("INVALID_POINTER_ACTION: use place, rotate, remove or inspect".into());
    }
    app.world_mut()
        .resource_mut::<State>()
        .unwrap()
        .preview_action = action.into();
    app.refresh_extracted();
    Ok(())
}
pub fn preview(app: &App, x: i32, y: i32, action: &str) -> Value {
    preview_world(app.world(), x, y, action)
}
pub(super) fn preview_world(world: &World, x: i32, y: i32, action: &str) -> Value {
    let state = world.resource::<State>().unwrap();
    let s = at(world, x, y).map(|e| world.get::<Structure>(e).unwrap());
    let error = if tile(x, y).is_err() {
        Some("Outside the grid.")
    } else if !["place", "rotate", "remove", "inspect"].contains(&action) {
        Some("Choose Build, Inspect, Rotate or Remove.")
    } else if action != "inspect" && state.completion_tick.is_some() {
        Some("Complete. Restart to build a new factory.")
    } else if action != "inspect" && state.diagnostic.is_some() {
        Some("Simulation stopped. Restart to recover.")
    } else {
        match action {
            "place" if s.is_some() => {
                Some("Occupied. Inspect, rotate or remove this structure first.")
            }
            "place" if (state.selection.kind == Kind::Extractor) != ((x, y) == (1, 3)) => Some(
                "Extractors require the ore deposit (1,3). Conveyors and processors require ground.",
            ),
            "rotate" | "remove" if s.is_none() => Some("Empty tile. Choose a structure."),
            "rotate" | "remove" if s.is_some_and(|s| s.kind == Kind::Delivery) => {
                Some("Delivery is fixed; it cannot be rotated or removed.")
            }
            _ => None,
        }
    };
    let facing = if action == "rotate" {
        s.map(|s| s.facing.clockwise())
            .unwrap_or(state.selection.facing)
    } else {
        state.selection.facing
    };
    let ore = s.map_or(0, |s| s.slots.items().filter(|i| *i == Item::Ore).count());
    let plate = s.map_or(0, |s| s.slots.items().filter(|i| *i == Item::Plate).count());
    let detail = error.map(str::to_owned).unwrap_or_else(|| match action {
        "place" => format!(
            "Build {:?} at ({x},{y}), facing {facing:?}. Arrows mark output; blue marks input.",
            state.selection.kind
        ),
        "rotate" => format!("Rotate clockwise to {facing:?}; items and work are preserved."),
        "remove" => format!(
            "Remove ({x},{y}). Discards {ore} ore and {plate} plates, including in-process ore."
        ),
        _ => format!("Inspect ({x},{y}) and pin its live details."),
    });
    json!({"x":x,"y":y,"action":action,"valid":error.is_none(),"label":if error.is_some(){"Cannot perform action"}else{match action{"place"=>"Build preview","rotate"=>"Rotation preview","remove"=>"Removal preview",_=>"Inspect tile"}},"detail":detail,"facing":facing,"discarded_ore":if action=="remove"{ore}else{0},"discarded_plate":if action=="remove"{plate}else{0}})
}

pub(super) fn connection(world: &World, s: &Structure, reason: &str) -> Value {
    let target = transport::target(s);
    let destination = target
        .and_then(|(x, y)| at(world, x, y))
        .map(|e| world.get::<Structure>(e).unwrap());
    // Geometry is meaningful even before the source has an item.
    let code = if reason == "complete" {
        "complete"
    } else if s.output().is_none() {
        "no_output"
    } else if destination.is_none() {
        "disconnected"
    } else if destination.is_some_and(|d| !d.inputs().contains(&s.facing.opposite())) {
        "wrong_facing"
    } else {
        match reason {
            "rejected_item_type" => "wrong_type",
            "full_destination" => "full",
            "contention" => "contended",
            "empty_source" => "empty",
            _ => "ready",
        }
    };
    let target_text = target.map_or_else(|| "none".into(), |(x, y)| format!("({x},{y})"));
    let mut contenders: Vec<_> = world
        .iter::<Structure>()
        .filter(|(_, other)| {
            transport::target(other) == target
                && other.slots.output.is_some()
                && destination.is_some_and(|d| {
                    d.inputs().contains(&other.facing.opposite())
                        && !(d.kind == Kind::Processor && other.slots.output != Some(Item::Ore)
                            || d.kind == Kind::Delivery && other.slots.output != Some(Item::Plate))
                })
        })
        .map(|(_, s)| json!({"x":s.x,"y":s.y}))
        .collect();
    contenders.sort_by_key(|v| (v["y"].as_i64().unwrap(), v["x"].as_i64().unwrap()));
    let winner = contenders
        .first()
        .map_or_else(|| "unknown".into(), |v| format!("({},{})", v["x"], v["y"]));
    let (label, detail, remedy) = match code {
        "disconnected" => (
            "Disconnected",
            format!("Output faces {target_text}, which has no receiving structure."),
            "Build a receiving neighbor or rotate the output toward the route.",
        ),
        "wrong_facing" => (
            "Wrong facing",
            format!("The structure at {target_text} has no input on this touching face."),
            "Rotate the receiver so its blue input faces this output, or reroute the source.",
        ),
        "wrong_type" => (
            "Wrong item type",
            format!(
                "The item cannot enter {target_text}; processors accept ore and delivery accepts plates."
            ),
            "Route ore through a processor before delivery; route plates away from processor inputs.",
        ),
        "full" => (
            "Destination full",
            format!("The receiving slot at {target_text} is occupied at this tick boundary."),
            "Inspect the downstream route and clear its blockage. A slot emptied this tick accepts again next tick.",
        ),
        "contended" => (
            "Contended input",
            format!(
                "Source {winner} wins the empty slot at {target_text} this tick (top row first, then leftmost)."
            ),
            "Wait for a later tick or reroute competing feeds to separate inputs; priority is fixed.",
        ),
        "empty" => (
            "Connected; waiting for item",
            format!("Output connects to {target_text}; this source has no outgoing item."),
            "Check the upstream supply or machine work progress.",
        ),
        "complete" => (
            "Complete",
            "Ten plates delivered; all simulation is frozen.".into(),
            "Restart for a new run.",
        ),
        "no_output" => (
            "Delivery input only",
            "Accepts plates from the west; no output or buffer.".into(),
            "Connect a plate route to the west input.",
        ),
        _ => (
            "Ready to transfer",
            format!("The outgoing item can enter {target_text} on the next tick."),
            "Advance a tick or resume.",
        ),
    };
    json!({"code":code,"label":label,"detail":detail,"remedy":remedy,"target":target.map(|(x,y)|json!({"x":x,"y":y})),"contenders":contenders})
}
pub(super) fn enrich(world: &World, s: &Structure, value: &mut Value) {
    let reason = value["transport"]["reason"]
        .as_str()
        .unwrap_or("empty_source");
    let route = connection(world, s, reason);
    let state = world.resource::<State>().unwrap();
    let (status, label, detail, remedy) = if state.completion_tick.is_some() {
        (
            "complete",
            "Complete",
            "Ten plates delivered. Factory frozen.",
            "Restart for a new run.",
        )
    } else if state.diagnostic.is_some() {
        (
            "stopped",
            "Stopped",
            "A simulation diagnostic stopped this run.",
            "Restart for a new run.",
        )
    } else if s.kind == Kind::Processor && s.slots.in_process.is_some() && s.remaining > 0 {
        (
            "working",
            "Working",
            "Converting one ore into one plate in 120 work ticks.",
            "Follow the work progress and output connection.",
        )
    } else if s.slots.output.is_some() && !matches!(route["code"].as_str(), Some("ready")) {
        (
            "output_blocked",
            "Output blocked",
            "The outgoing item cannot transfer at this boundary.",
            "Read the connection cause below and repair that route.",
        )
    } else if s.kind == Kind::Extractor {
        (
            "working",
            "Working",
            "Extracting one ore every 60 eligible ticks.",
            "Keep the output clear so extraction can continue.",
        )
    } else if s.kind == Kind::Processor && s.slots.in_process.is_some() {
        (
            "output_blocked",
            "Output blocked",
            "Finished ore is waiting for the plate output slot.",
            "Clear the plate output route to release the finished batch.",
        )
    } else if s.kind == Kind::Processor && s.slots.input.is_some() {
        (
            "working",
            "Ready to process",
            "One ore is queued for a new 120-tick batch.",
            "Advance a tick or resume.",
        )
    } else if s.kind == Kind::Processor {
        (
            "starved",
            "Starved",
            "No ore is available to start a batch.",
            "Connect ore supply to the blue input opposite the output arrow.",
        )
    } else if s.slots.output.is_some() {
        (
            "working",
            "Working",
            "An item is ready to move.",
            "Advance a tick or resume.",
        )
    } else {
        (
            "starved",
            "Waiting for supply",
            "No incoming item is available.",
            "Inspect the upstream route; delivery requires plates.",
        )
    };
    value["explanation"] = json!({"status":status,"label":label,"detail":detail,"remedy":remedy});
    let incoming: Vec<_> = s.inputs().into_iter().map(|face| {
        let (dx,dy)=match face {Facing::N=>(0,-1),Facing::E=>(1,0),Facing::S=>(0,1),Facing::W=>(-1,0)};
        let (x,y)=(s.x+dx,s.y+dy);
        let source=at(world,x,y).map(|e|world.get::<Structure>(e).unwrap());
        let connected=source.is_some_and(|source|transport::target(source)==Some((s.x,s.y)));
        json!({"face":face,"x":x,"y":y,"connected":connected,"detail":if source.is_none(){format!("Input {face:?}: no source at ({x},{y}). Build an upstream output here.")}else if !connected{format!("Input {face:?}: neighbor ({x},{y}) does not output here. Rotate or reroute it.")}else{format!("Input {face:?}: connected from ({x},{y}); inspect that source for supply.")}})
    }).collect();
    if status == "starved" && !incoming.is_empty() {
        value["explanation"]["remedy"] = json!(
            incoming
                .iter()
                .map(|v| v["detail"].as_str().unwrap())
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    value["input_connections"] = json!(incoming);
    value["connection"] = route;
    value["inventory"]=json!([("input",s.slots.input,usize::from(s.kind==Kind::Processor)),("in_process",s.slots.in_process,usize::from(s.kind==Kind::Processor)),("output",s.slots.output,usize::from(s.kind!=Kind::Delivery))].into_iter().filter(|(_,_,capacity)|*capacity>0).map(|(slot,item,capacity)|json!({"slot":slot,"item":item,"count":usize::from(item.is_some()),"capacity":capacity})).collect::<Vec<_>>());
    value["recipe"] = match s.kind {
        Kind::Extractor => {
            json!({"label":"Deposit to 1 ore / 60 eligible ticks","elapsed":s.progress,"total":60})
        }
        Kind::Processor => {
            json!({"label":"1 ore to 1 plate / 120 work ticks","elapsed":if s.slots.in_process.is_some(){120-s.remaining}else{0},"total":120})
        }
        _ => Value::Null,
    };
}
pub(super) fn state_ui(world: &World, value: &mut Value) {
    let state = world.resource::<State>().unwrap();
    value["preview"] = state.hover.map_or(Value::Null, |(x, y)| {
        preview_world(world, x, y, &state.preview_action)
    });
    value["inspected"] = state
        .inspected
        .map_or(Value::Null, |(x, y)| inspect_tile(world, x, y));
    value["ui"] = json!({"objective":format!("Deliver plates: {}/10",state.delivered),"onboarding":"Build an extractor on (1,3). Connect its output to a processor's rear input, then carry plates to the west input of delivery (10,3). Blue marks accept items; yellow arrows send them. Inspect a tile to diagnose it.","legend":"Green: working. Amber: waiting for supply. Red: blocked output. Blue marks: inputs. Yellow arrows: output."});
}

#[cfg(test)]
mod tests {
    use super::*;
    fn detail(app: &App, x: i32, y: i32) -> Value {
        inspect_tile(app.world(), x, y)["structure"].clone()
    }
    #[test]
    fn queries_previews_and_rendering_are_read_only() {
        let mut app = build_transport_fixture("contention").unwrap();
        pointer(&mut app, 80., 112., "hover").unwrap();
        let before = status(&app);
        let recording = app.world().resource::<Recording>().unwrap().records.clone();
        for action in ["place", "rotate", "remove", "inspect", "unknown"] {
            for _ in 0..5 {
                let _ = preview(&app, 2, 3, action);
                let _ = interface(&app);
                let _ = render_image(app.world()).unwrap();
            }
        }
        assert_eq!(status(&app), before);
        assert_eq!(
            app.world().resource::<Recording>().unwrap().records,
            recording
        );
        assert!(transport::conserved(app.world()));
    }
    #[test]
    fn route_explanations_match_current_plans_and_repairs() {
        for (fixture, x, y, code) in [
            ("disconnected", 0, 0, "disconnected"),
            ("ports", 2, 0, "wrong_facing"),
            ("ports", 5, 0, "wrong_type"),
            ("snapshot", 2, 2, "full"),
            ("contention", 2, 3, "contended"),
        ] {
            let app = build_transport_fixture(fixture).unwrap();
            let d = detail(&app, x, y);
            assert_eq!(d["connection"]["code"], code);
            assert_eq!(d["explanation"]["status"], "output_blocked");
            assert!(!d["connection"]["remedy"].as_str().unwrap().is_empty());
        }
        let mut app = build_transport_fixture("ports").unwrap();
        player_command(&mut app, r#"{"op":"rotate","x":3,"y":0}"#).unwrap();
        assert_eq!(detail(&app, 2, 0)["connection"]["code"], "ready");
        app.advance_fixed(1);
        assert!(transport::conserved(app.world()));
    }
    #[test]
    fn contenders_and_winner_do_not_depend_on_allocation_order() {
        let mut app = build_transport_fixture("contention").unwrap();
        let before = detail(&app, 2, 3)["connection"].clone();
        let old: Vec<_> = app
            .world()
            .iter::<Structure>()
            .map(|(e, s)| (e, s.clone()))
            .collect();
        for (e, _) in &old {
            app.world_mut().despawn(*e);
        }
        for (_, s) in old.into_iter().rev() {
            app.world_mut().spawn_with((s,));
        }
        assert_eq!(detail(&app, 2, 3)["connection"], before);
        assert!(
            before["detail"]
                .as_str()
                .unwrap()
                .contains("Source (3,2) wins")
        );
    }
    #[test]
    fn placement_rotation_removal_previews_match_operation_rules() {
        let mut app = build_game();
        assert_eq!(preview(&app, 1, 3, "place")["valid"], false);
        assert_eq!(preview(&app, 0, 0, "place")["valid"], true);
        assert_eq!(preview(&app, 10, 3, "rotate")["valid"], false);
        assert_eq!(preview(&app, -1, 0, "inspect")["valid"], false);
        player_command(
            &mut app,
            r#"{"op":"select","kind":"extractor","facing":"N"}"#,
        )
        .unwrap();
        assert_eq!(preview(&app, 1, 3, "place")["valid"], true);
        assert_eq!(preview(&app, 0, 0, "place")["valid"], false);
        let app = build_production_fixture("processor_full").unwrap();
        let p = preview(&app, 5, 3, "remove");
        assert_eq!(p["discarded_ore"], 2);
        assert_eq!(p["discarded_plate"], 1);
        assert_eq!(preview(&app, 5, 3, "rotate")["facing"], "S");
        assert_eq!(detail(&app, 5, 3)["inventory"].as_array().unwrap().len(), 3);
    }
    #[test]
    fn working_starved_and_finished_batch_have_distinct_explanations() {
        let mut app = build_production_fixture("processor_input").unwrap();
        assert_eq!(detail(&app, 5, 3)["explanation"]["status"], "working");
        app.advance_fixed(1);
        assert_eq!(detail(&app, 5, 3)["recipe"]["elapsed"], 0);
        app.advance_fixed(60);
        assert_eq!(detail(&app, 5, 3)["recipe"]["elapsed"], 60);
        let mut app = build_game();
        player_command(
            &mut app,
            r#"{"op":"place","kind":"processor","x":5,"y":3,"facing":"E"}"#,
        )
        .unwrap();
        assert_eq!(detail(&app, 5, 3)["explanation"]["status"], "starved");
        let app = build_production_fixture("processor_full").unwrap();
        assert_eq!(
            detail(&app, 5, 3)["explanation"]["status"],
            "output_blocked"
        );
    }
}
