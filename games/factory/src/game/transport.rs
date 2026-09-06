//! Snapshot transport is deliberately conservative: a departing item does not free
//! its destination capacity until the next tick. All ordering uses tile (y,x).
use super::*;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Item {
    Ore,
    Plate,
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(super) struct Slots {
    pub input: Option<Item>,
    pub in_process: Option<Item>,
    pub output: Option<Item>,
}
impl Slots {
    pub fn items(self) -> impl Iterator<Item = Item> {
        [self.input, self.in_process, self.output]
            .into_iter()
            .flatten()
    }
}
fn delta(facing: Facing) -> (i32, i32) {
    match facing {
        Facing::N => (0, -1),
        Facing::E => (1, 0),
        Facing::S => (0, 1),
        Facing::W => (-1, 0),
    }
}
fn target(s: &Structure) -> Option<(i32, i32)> {
    s.output().map(|f| {
        let (dx, dy) = delta(f);
        (s.x + dx, s.y + dy)
    })
}
fn snapshot(world: &World) -> Vec<(titan::Entity, Structure)> {
    let mut result: Vec<_> = world
        .iter::<Structure>()
        .map(|(e, s)| (e, s.clone()))
        .collect();
    result.sort_by_key(|(_, s)| (s.y, s.x));
    result
}
/// Plans against immutable slots, then reserves in source position order.
fn plan(
    snapshot: &[(titan::Entity, Structure)],
    delivered: u64,
) -> Vec<(&'static str, Option<usize>)> {
    let mut reserved = BTreeSet::new();
    snapshot
        .iter()
        .map(|(_, s)| {
            let Some(item) = s.slots.output else {
                return ("empty_source", None);
            };
            let Some((x, y)) = target(s) else {
                return ("missing_neighbor", None);
            };
            let Some(index) = snapshot.iter().position(|(_, d)| d.x == x && d.y == y) else {
                return ("missing_neighbor", None);
            };
            let d = &snapshot[index].1;
            if !d.inputs().contains(&s.facing.opposite()) {
                return ("mismatched_input_face", None);
            }
            if (d.kind == Kind::Processor && item != Item::Ore)
                || (d.kind == Kind::Delivery && item != Item::Plate)
            {
                return ("rejected_item_type", None);
            }
            let full = match d.kind {
                Kind::Delivery => delivered >= 10,
                Kind::Processor => d.slots.input.is_some(),
                _ => d.slots.output.is_some(),
            };
            if full {
                return ("full_destination", None);
            }
            if !reserved.insert(index) {
                return ("contention", None);
            }
            ("ready", Some(index))
        })
        .collect()
}
pub(super) fn tick(world: &mut World) {
    let state = world.resource::<State>().unwrap();
    if state.completion_tick.is_some() {
        return;
    }
    let next_tick = state.tick.checked_add(1).expect("factory tick overflow");
    let snapshot = snapshot(world);
    let plans = plan(&snapshot, state.delivered);
    let mut slots: Vec<_> = snapshot.iter().map(|(_, s)| s.slots).collect();
    let mut delivered = state.delivered;
    for (source, (_, destination)) in plans.iter().enumerate() {
        if let Some(destination) = destination {
            let item = snapshot[source].1.slots.output.unwrap();
            slots[source].output = None;
            match snapshot[*destination].1.kind {
                Kind::Delivery => {
                    delivered = delivered
                        .checked_add(1)
                        .expect("factory delivered overflow")
                }
                Kind::Processor => slots[*destination].input = Some(item),
                _ => slots[*destination].output = Some(item),
            }
        }
    }
    for (i, (entity, _)) in snapshot.iter().enumerate() {
        let s = world.get_mut::<Structure>(*entity).unwrap();
        s.slots = slots[i];
        s.last_transfer_reason = Some(if plans[i].1.is_some() {
            "transferred"
        } else {
            plans[i].0
        });
    }
    let state = world.resource_mut::<State>().unwrap();
    state.tick = next_tick;
    state.delivered = delivered;
    if delivered == 10 {
        state.completion_tick = Some(next_tick);
    }
    assert!(conserved(world), "factory item conservation violated");
}
pub(super) fn conserved(world: &World) -> bool {
    let s = world.resource::<State>().unwrap();
    let resident = world
        .iter::<Structure>()
        .map(|(_, s)| s.slots.items().count() as u128)
        .sum::<u128>();
    u128::from(s.seeded) + u128::from(s.extracted)
        == resident
            + u128::from(s.delivered)
            + u128::from(s.discarded_ore)
            + u128::from(s.discarded_plate)
}
pub(super) fn structure_value(world: &World, s: &Structure) -> Value {
    let snapshot = snapshot(world);
    let state = world.resource::<State>().unwrap();
    let plans = plan(&snapshot, state.delivered);
    let i = snapshot
        .iter()
        .position(|(_, d)| (s.x, s.y) == (d.x, d.y))
        .unwrap();
    let mut value = s.value();
    value["transport"] = json!({"reason":if state.completion_tick.is_some(){"complete"}else{plans[i].0},"target":target(s).map(|(x,y)|json!({"x":x,"y":y}))});
    value
}
pub(super) fn item_positions(s: &Structure) -> Vec<Value> {
    let (dx, dy) = delta(s.facing);
    [("input",s.slots.input,-7.),("in_process",s.slots.in_process,0.),("output",s.slots.output,if s.kind==Kind::Processor {7.}else{0.})].into_iter().filter_map(|(slot,item,offset)|item.map(|item|json!({"slot":slot,"item":item,"x":f64::from(s.x)*TILE+16.+f64::from(dx)*offset,"y":f64::from(s.y)*TILE+16.+f64::from(dy)*offset}))).collect()
}

/// Explicit seeded transport-only verification setups. This is a host startup
/// API, never a player operation or inspector command. Like `build_game`, the
/// returned app needs no Startup schedule to construct its initial world.
pub fn build_transport_fixture(name: &str) -> Result<App, String> {
    let mut app = build_game();
    let mut add = |x, y, kind, facing, item| {
        apply(&mut app, Operation::Place { kind, x, y, facing }).unwrap();
        let e = at(app.world(), x, y).unwrap();
        app.world_mut()
            .get_mut::<Structure>(e)
            .unwrap()
            .slots
            .output = item;
        app.world_mut().resource_mut::<State>().unwrap().seeded += u64::from(item.is_some());
    };
    match name {
        "single"|"snapshot"=> {
            for x in 2..=4 {add(x,2,Kind::Conveyor,Facing::E,if x==2 || (name=="snapshot" && x==3){Some(Item::Ore)}else{None});}
        }
        "contention"=>{
            add(3,2,Kind::Conveyor,Facing::S,Some(Item::Ore));
            add(2,3,Kind::Conveyor,Facing::E,Some(Item::Plate));
            add(3,3,Kind::Conveyor,Facing::E,None);
            add(4,3,Kind::Conveyor,Facing::E,None);
        }
        "cycle_partial"|"cycle_full"=>{
            for (i,(x,y,facing)) in [(2,2,Facing::E),(3,2,Facing::S),(3,3,Facing::W),(2,3,Facing::N)].into_iter().enumerate() {
                add(x,y,Kind::Conveyor,facing,if i==0 || name=="cycle_full"{Some(Item::Ore)}else{None});
            }
        }
        "disconnected"=>{
            add(0,0,Kind::Conveyor,Facing::W,Some(Item::Ore));
            add(6,5,Kind::Conveyor,Facing::E,Some(Item::Plate));
        }
        "ports"=>{
            // Head-on outputs; wrong item at processor; direct extractor input.
            add(2,0,Kind::Conveyor,Facing::E,Some(Item::Ore));
            add(3,0,Kind::Conveyor,Facing::W,None);
            add(5,0,Kind::Conveyor,Facing::E,Some(Item::Plate));
            add(6,0,Kind::Processor,Facing::E,None);
            add(1,3,Kind::Extractor,Facing::E,Some(Item::Ore));
            add(2,3,Kind::Processor,Facing::E,None);
            add(9,3,Kind::Conveyor,Facing::E,Some(Item::Plate));
        }
        _=>return Err("UNKNOWN_TRANSPORT_FIXTURE: use single, snapshot, contention, cycle_partial, cycle_full, disconnected or ports".into())
    }
    assert!(conserved(app.world()));
    app.refresh_extracted();
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn output(app: &App, x: i32, y: i32) -> Option<Item> {
        app.world()
            .get::<Structure>(at(app.world(), x, y).unwrap())
            .unwrap()
            .slots
            .output
    }
    fn reason(app: &App, x: i32, y: i32) -> String {
        inspect_tile(app.world(), x, y)["structure"]["transport"]["reason"]
            .as_str()
            .unwrap()
            .into()
    }
    fn op(app: &mut App, op: Value) {
        player_command(app, &op.to_string()).unwrap();
        assert!(conserved(app.world()));
    }
    #[test]
    fn snapshot_capacity_and_one_hop() {
        let mut app = build_transport_fixture("snapshot").unwrap();
        app.advance_fixed(1);
        assert_eq!(
            [output(&app, 2, 2), output(&app, 3, 2), output(&app, 4, 2)],
            [Some(Item::Ore), None, Some(Item::Ore)]
        );
        app.advance_fixed(1);
        assert_eq!(
            [output(&app, 2, 2), output(&app, 3, 2), output(&app, 4, 2)],
            [None, Some(Item::Ore), Some(Item::Ore)]
        );
        assert_eq!(reason(&app, 3, 2), "full_destination");
        assert_eq!(reason(&app, 4, 2), "missing_neighbor");
        app.advance_fixed(500);
        assert!(conserved(app.world()));
        let mut single = build_transport_fixture("single").unwrap();
        single.advance_fixed(1);
        assert_eq!(output(&single, 3, 2), Some(Item::Ore));
        assert_eq!(output(&single, 4, 2), None);
    }
    #[test]
    fn corner_contention_uses_source_y_then_x_not_spawn_order() {
        let mut app = build_transport_fixture("contention").unwrap();
        // Reverse allocation order while preserving physical state.
        let old = snapshot(app.world());
        for (e, _) in &old {
            app.world_mut().despawn(*e);
        }
        for (_, s) in old.into_iter().rev() {
            app.world_mut().spawn_with((s,));
        }
        assert_eq!(reason(&app, 2, 3), "contention");
        app.advance_fixed(1);
        assert_eq!(output(&app, 3, 3), Some(Item::Ore));
        assert_eq!(output(&app, 2, 3), Some(Item::Plate));
        assert_eq!(
            inspect_tile(app.world(), 2, 3)["structure"]["last_transfer_reason"],
            "contention"
        );
        app.advance_fixed(1);
        assert_eq!(output(&app, 2, 3), Some(Item::Plate));
        app.advance_fixed(1);
        assert_eq!(output(&app, 3, 3), Some(Item::Plate));
        assert!(conserved(app.world()));
    }
    #[test]
    fn cycle_jams_and_edited_hole_preserve_every_item() {
        let mut app = build_transport_fixture("cycle_partial").unwrap();
        for (x, y) in [(3, 2), (3, 3), (2, 3), (2, 2)]
            .into_iter()
            .cycle()
            .take(100)
        {
            app.advance_fixed(1);
            assert_eq!(output(&app, x, y), Some(Item::Ore));
            assert!(conserved(app.world()));
        }
        let mut app = build_transport_fixture("cycle_full").unwrap();
        app.advance_fixed(100);
        assert_eq!(reason(&app, 2, 2), "full_destination");
        op(&mut app, json!({"op":"remove","x":3,"y":2}));
        assert_eq!(app.world().resource::<State>().unwrap().discarded_ore, 1);
        op(
            &mut app,
            json!({"op":"place","kind":"conveyor","x":3,"y":2,"facing":"S"}),
        );
        app.advance_fixed(1);
        assert_eq!(output(&app, 2, 2), None);
        assert_eq!(output(&app, 3, 2), Some(Item::Ore));
        op(&mut app, json!({"op":"rotate","x":3,"y":2}));
        assert_eq!(output(&app, 3, 2), Some(Item::Ore));
        let before = status(&app);
        assert!(player_command(&mut app, r#"{"op":"remove","x":10,"y":3}"#).is_err());
        assert_eq!(before, status(&app));
        op(&mut app, json!({"op":"restart"}));
        let s = app.world().resource::<State>().unwrap();
        assert_eq!((s.tick, s.seeded, s.discarded_ore), (0, 0, 0));
    }
    #[test]
    fn ports_types_and_reason_precedence() {
        let mut app = build_transport_fixture("ports").unwrap();
        assert_eq!(reason(&app, 2, 0), "mismatched_input_face");
        assert_eq!(reason(&app, 5, 0), "rejected_item_type");
        app.advance_fixed(1);
        assert_eq!(
            inspect_tile(app.world(), 2, 3)["structure"]["slots"]["input"],
            "ore"
        );
        assert_eq!(app.world().resource::<State>().unwrap().delivered, 1);
        // Type rejection precedes occupied destination.
        let e = at(app.world(), 6, 0).unwrap();
        app.world_mut().get_mut::<Structure>(e).unwrap().slots.input = Some(Item::Ore);
        app.world_mut().resource_mut::<State>().unwrap().seeded += 1;
        assert_eq!(reason(&app, 5, 0), "rejected_item_type");
        app.world_mut().get_mut::<Structure>(e).unwrap().facing = Facing::N;
        assert_eq!(reason(&app, 5, 0), "mismatched_input_face");
        op(&mut app, json!({"op":"remove","x":6,"y":0}));
        assert_eq!(reason(&app, 5, 0), "missing_neighbor");
        let mut app = build_transport_fixture("disconnected").unwrap();
        app.advance_fixed(1000);
        assert_eq!(reason(&app, 0, 0), "missing_neighbor");
        assert_eq!(reason(&app, 6, 5), "missing_neighbor");
        assert!(conserved(app.world()));
    }
    #[test]
    fn removal_accounts_all_machine_slots_and_overflow_rejects_atomically() {
        let mut app = build_game();
        op(
            &mut app,
            json!({"op":"place","kind":"processor","x":4,"y":4,"facing":"E"}),
        );
        let e = at(app.world(), 4, 4).unwrap();
        app.world_mut().get_mut::<Structure>(e).unwrap().slots = Slots {
            input: Some(Item::Ore),
            in_process: Some(Item::Ore),
            output: Some(Item::Plate),
        };
        app.world_mut().resource_mut::<State>().unwrap().seeded = 3;
        op(&mut app, json!({"op":"remove","x":4,"y":4}));
        let s = app.world().resource::<State>().unwrap();
        assert_eq!((s.discarded_ore, s.discarded_plate), (2, 1));
        let mut app = build_transport_fixture("single").unwrap();
        app.world_mut()
            .resource_mut::<State>()
            .unwrap()
            .discarded_ore = u64::MAX;
        let before = status(&app);
        assert!(
            player_command(&mut app, r#"{"op":"remove","x":2,"y":2}"#)
                .unwrap_err()
                .contains("COUNTER_OVERFLOW")
        );
        assert_eq!(before, status(&app));
    }
    #[test]
    fn delivery_limit_freezes_tick_and_restart_resets_epoch() {
        let mut app = build_transport_fixture("ports").unwrap();
        let s = app.world_mut().resource_mut::<State>().unwrap();
        s.delivered = 9;
        s.seeded += 9;
        app.advance_fixed(1);
        assert_eq!(
            app.world().resource::<State>().unwrap().completion_tick,
            Some(1)
        );
        app.advance_fixed(100);
        assert_eq!(app.world().resource::<State>().unwrap().tick, 1);
        assert!(
            player_command(&mut app, r#"{"op":"rotate","x":2,"y":0}"#)
                .unwrap_err()
                .contains("COMPLETE")
        );
        restart(&mut app);
        assert!(conserved(app.world()));
        assert_eq!(
            app.world().resource::<State>().unwrap().completion_tick,
            None
        );
    }
    #[test]
    fn processor_can_receive_and_send_distinct_slots_in_same_snapshot() {
        let mut app = build_transport_fixture("ports").unwrap();
        let e = at(app.world(), 2, 3).unwrap();
        app.world_mut()
            .get_mut::<Structure>(e)
            .unwrap()
            .slots
            .output = Some(Item::Plate);
        app.world_mut().resource_mut::<State>().unwrap().seeded += 1;
        apply(
            &mut app,
            Operation::Place {
                kind: Kind::Conveyor,
                x: 3,
                y: 3,
                facing: Facing::E,
            },
        )
        .unwrap();
        app.advance_fixed(1);
        let processor = app.world().get::<Structure>(e).unwrap();
        assert_eq!(processor.slots.input, Some(Item::Ore));
        assert_eq!(processor.slots.output, None);
        assert_eq!(output(&app, 3, 3), Some(Item::Plate));
        assert!(conserved(app.world()));
    }
    #[test]
    fn same_row_contention_prioritizes_left_source() {
        let mut app = build_game();
        for (x, facing, item) in [
            (2, Facing::E, Some(Item::Ore)),
            (4, Facing::W, Some(Item::Plate)),
            (3, Facing::S, None),
        ] {
            apply(
                &mut app,
                Operation::Place {
                    kind: Kind::Conveyor,
                    x,
                    y: 2,
                    facing,
                },
            )
            .unwrap();
            let e = at(app.world(), x, 2).unwrap();
            app.world_mut()
                .get_mut::<Structure>(e)
                .unwrap()
                .slots
                .output = item;
        }
        app.world_mut().resource_mut::<State>().unwrap().seeded = 2;
        app.advance_fixed(1);
        assert_eq!(output(&app, 3, 2), Some(Item::Ore));
        assert_eq!(output(&app, 4, 2), Some(Item::Plate));
    }
}
