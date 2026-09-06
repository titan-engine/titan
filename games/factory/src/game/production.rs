//! Production runs on post-transfer slots, after the completion check.
use super::*;

pub(super) fn produce(s: &mut Structure, extracted: &mut u64) -> Result<(), String> {
    match s.kind {
        Kind::Extractor if s.slots.output.is_none() => {
            s.progress += 1;
            if s.progress == 60 {
                *extracted = extracted
                    .checked_add(1)
                    .ok_or("COUNTER_OVERFLOW: extracted")?;
                s.slots.output = Some(Item::Ore);
                s.progress = 0;
            }
        }
        Kind::Processor => {
            if s.slots.in_process.is_some() {
                if s.remaining > 0 {
                    s.remaining -= 1;
                }
                if s.remaining == 0 && s.slots.output.is_none() {
                    s.slots.in_process = None;
                    s.slots.output = Some(Item::Plate);
                }
            }
            if s.slots.in_process.is_none() && s.slots.input == Some(Item::Ore) {
                s.slots.input = None;
                s.slots.in_process = Some(Item::Ore);
                s.remaining = 120;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn machine_status(state: &State, s: &Structure) -> &'static str {
    if state.completion_tick.is_some() {
        return "complete";
    }
    if state.diagnostic.is_some() {
        return "stopped";
    }
    if !state.production_enabled && matches!(s.kind, Kind::Extractor | Kind::Processor) {
        return "production_disabled";
    }
    match s.kind {
        Kind::Extractor if s.slots.output.is_some() => "output_blocked",
        Kind::Extractor => "extracting",
        Kind::Processor if s.slots.in_process.is_some() && s.remaining == 0 => {
            "finished_batch_blocked"
        }
        Kind::Processor if s.slots.in_process.is_some() => "processing",
        Kind::Processor if s.slots.output.is_some() => "output_blocked",
        Kind::Processor => "waiting_for_ore",
        Kind::Conveyor => "transport",
        Kind::Delivery => "receiving",
    }
}

/// Startup-only semantic fixtures. Seeding is never a player operation.
/// All processors are at (5,3), extractors at the deposit (1,3), facing east.
pub fn build_production_fixture(name: &str) -> Result<App, String> {
    let mut app = build_game();
    let (kind, x) = match name {
        "isolated_extractor" | "extractor_blocked" => (Kind::Extractor, 1),
        "processor_input" | "processor_blocked" | "processor_full" => (Kind::Processor, 5),
        _ => return Err("UNKNOWN_PRODUCTION_FIXTURE: use isolated_extractor, extractor_blocked, processor_input, processor_blocked or processor_full".into()),
    };
    apply(
        &mut app,
        Operation::Place {
            kind,
            x,
            y: 3,
            facing: Facing::E,
        },
    )?;
    let entity = at(app.world(), x, 3).unwrap();
    let s = app.world_mut().get_mut::<Structure>(entity).unwrap();
    if kind == Kind::Processor {
        s.slots.input = Some(Item::Ore);
    }
    if matches!(name, "processor_blocked" | "processor_full") {
        s.slots.output = Some(Item::Plate);
    }
    if name == "processor_full" {
        s.slots.in_process = Some(Item::Ore);
    }
    if name == "extractor_blocked" {
        s.slots.output = Some(Item::Ore);
    }
    let seeded = s.slots.items().count() as u64;
    app.world_mut().resource_mut::<State>().unwrap().seeded = seeded;
    assert!(transport::conserved(app.world()));
    app.refresh_extracted();
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn machine(app: &App, x: i32) -> Structure {
        app.world()
            .get::<Structure>(at(app.world(), x, 3).unwrap())
            .unwrap()
            .clone()
    }
    fn place(app: &mut App, kind: Kind, x: i32) {
        apply(
            app,
            Operation::Place {
                kind,
                x,
                y: 3,
                facing: Facing::E,
            },
        )
        .unwrap();
    }
    fn step(app: &mut App, n: u64) {
        for _ in 0..n {
            app.advance_fixed(1);
            assert!(transport::conserved(app.world()));
        }
    }
    fn route(app: &mut App) {
        place(app, Kind::Extractor, 1);
        for x in 2..=9 {
            place(
                app,
                if x == 5 {
                    Kind::Processor
                } else {
                    Kind::Conveyor
                },
                x,
            );
        }
    }
    #[test]
    fn independent_reference_route_trace_and_completion_freeze() {
        let mut app = build_game();
        route(&mut app);
        for tick in 1..=1269 {
            step(&mut app, 1);
            let e = machine(&app, 1);
            let p = machine(&app, 5);
            match tick {
                59 => assert_eq!(e.progress, 59),
                60 | 120 => assert_eq!(e.slots.output, Some(Item::Ore)),
                61..=63 => assert_eq!(
                    machine(&app, tick as i32 - 59).slots.output,
                    Some(Item::Ore)
                ),
                64 => assert_eq!((p.remaining, p.slots.in_process), (120, Some(Item::Ore))),
                124 => assert_eq!((p.remaining, p.slots.input), (60, Some(Item::Ore))),
                184 => assert_eq!(
                    (p.remaining, p.slots.output, p.slots.in_process),
                    (120, Some(Item::Plate), Some(Item::Ore))
                ),
                185..=188 => assert_eq!(
                    machine(&app, tick as i32 - 179).slots.output,
                    Some(Item::Plate)
                ),
                _ => {}
            }
            let state = app.world().resource::<State>().unwrap();
            let expected = if tick < 189 {
                0
            } else {
                1 + (tick - 189) / 120
            };
            assert_eq!(state.delivered, expected, "tick {tick}");
            if tick == 1268 {
                assert_eq!(p.remaining, 116);
            }
            if tick == 1269 {
                assert_eq!(p.remaining, 116);
                assert_eq!(state.completion_tick, Some(tick));
            }
        }
        let mut before = state_value(&app);
        step(&mut app, 40);
        let mut after = state_value(&app);
        before.as_object_mut().unwrap().remove("frame");
        after.as_object_mut().unwrap().remove("frame");
        assert_eq!(before, after);
        for text in [
            r#"{"op":"rotate","x":5,"y":3}"#,
            r#"{"op":"remove","x":1,"y":3}"#,
            r#"{"op":"place","kind":"conveyor","x":0,"y":0,"facing":"E"}"#,
        ] {
            assert!(
                player_command(&mut app, text)
                    .unwrap_err()
                    .starts_with("COMPLETE")
            );
        }
        restart(&mut app);
        route(&mut app);
        step(&mut app, 1269);
        assert_eq!(
            app.world().resource::<State>().unwrap().completion_tick,
            Some(1269)
        );
    }
    #[test]
    fn extractor_pauses_and_refills_on_departure_tick() {
        let mut app = build_production_fixture("isolated_extractor").unwrap();
        step(&mut app, 180);
        assert_eq!(machine(&app, 1).progress, 0);
        assert_eq!(app.world().resource::<State>().unwrap().extracted, 1);
        place(&mut app, Kind::Conveyor, 2);
        step(&mut app, 1);
        assert_eq!(machine(&app, 1).progress, 1);
        step(&mut app, 59);
        assert_eq!(machine(&app, 1).slots.output, Some(Item::Ore));
        assert_eq!(app.world().resource::<State>().unwrap().extracted, 2);
    }
    #[test]
    fn processor_starts_without_work_and_retains_blocked_batch_and_backlog() {
        let mut app = build_production_fixture("processor_input").unwrap();
        step(&mut app, 1);
        assert_eq!(machine(&app, 5).remaining, 120);
        step(&mut app, 119);
        assert_eq!(machine(&app, 5).remaining, 1);
        step(&mut app, 1);
        assert_eq!(machine(&app, 5).slots.output, Some(Item::Plate));
        let mut app = build_production_fixture("processor_blocked").unwrap();
        step(&mut app, 121);
        assert_eq!(machine(&app, 5).remaining, 0);
        assert_eq!(machine(&app, 5).slots.in_process, Some(Item::Ore));
        // An independently seeded queued ore behind the finished batch.
        let e = at(app.world(), 5, 3).unwrap();
        app.world_mut().get_mut::<Structure>(e).unwrap().slots.input = Some(Item::Ore);
        app.world_mut().resource_mut::<State>().unwrap().seeded += 1;
        place(&mut app, Kind::Conveyor, 6);
        step(&mut app, 1);
        let p = machine(&app, 5);
        assert_eq!(
            (
                p.remaining,
                p.slots.input,
                p.slots.in_process,
                p.slots.output
            ),
            (120, None, Some(Item::Ore), Some(Item::Plate))
        );
        assert_eq!(machine(&app, 6).slots.output, Some(Item::Plate));
    }
    #[test]
    fn full_machine_rotation_removal_rejected_edits_and_restart() {
        let mut app = build_production_fixture("processor_full").unwrap();
        apply(&mut app, Operation::Rotate { x: 5, y: 3 }).unwrap();
        let p = machine(&app, 5);
        assert_eq!(p.facing, Facing::S);
        assert_eq!(p.slots.items().count(), 3);
        assert_eq!(p.remaining, 0);
        let before = status(&app);
        for text in [
            r#"{"op":"remove","x":10,"y":3}"#,
            r#"{"op":"place","kind":"conveyor","x":5,"y":3,"facing":"E"}"#,
            r#"{"op":"seed","x":5,"y":3}"#,
        ] {
            assert!(player_command(&mut app, text).is_err());
            assert_eq!(status(&app), before);
        }
        let result = apply(&mut app, Operation::Remove { x: 5, y: 3 }).unwrap();
        assert_eq!(result["discarded_ore"], 2);
        assert_eq!(result["discarded_plate"], 1);
        assert!(transport::conserved(app.world()));
        let mut app = build_production_fixture("processor_full").unwrap();
        step(&mut app, 200);
        restart(&mut app);
        assert_eq!(app.world().iter::<Structure>().count(), 1);
        let s = app.world().resource::<State>().unwrap();
        assert_eq!(
            (
                s.tick,
                s.seeded,
                s.extracted,
                s.delivered,
                s.discarded_ore,
                s.discarded_plate
            ),
            (0, 0, 0, 0, 0, 0)
        );
        assert!(s.production_enabled);
        assert!(s.completion_tick.is_none());
    }
    #[test]
    fn starvation_and_rotation_preserve_work() {
        let mut app = build_game();
        place(&mut app, Kind::Processor, 5);
        step(&mut app, 200);
        assert_eq!(machine(&app, 5).remaining, 0);
        assert_eq!(machine(&app, 5).slots.items().count(), 0);
        let mut app = build_production_fixture("processor_input").unwrap();
        step(&mut app, 40);
        apply(&mut app, Operation::Rotate { x: 5, y: 3 }).unwrap();
        assert_eq!(machine(&app, 5).remaining, 81);
        step(&mut app, 81);
        assert_eq!(machine(&app, 5).slots.output, Some(Item::Plate));
        let mut app = build_transport_fixture("ports").unwrap();
        step(&mut app, 200);
        assert_eq!(machine(&app, 2).slots.input, Some(Item::Ore));
        assert_eq!(machine(&app, 1).progress, 0);
        restart(&mut app);
        place(&mut app, Kind::Extractor, 1);
        step(&mut app, 60);
        assert_eq!(machine(&app, 1).slots.output, Some(Item::Ore));
    }
    #[test]
    fn overflow_stops_atomically_and_restart_recovers() {
        let mut app = build_production_fixture("isolated_extractor").unwrap();
        step(&mut app, 59);
        let state = app.world_mut().resource_mut::<State>().unwrap();
        state.extracted = u64::MAX;
        state.discarded_ore = u64::MAX;
        let error = player_command(&mut app, r#"{"op":"advance","ticks":1}"#).unwrap_err();
        assert!(error.contains("COUNTER_OVERFLOW"));
        assert_eq!(machine(&app, 1).progress, 59);
        assert_eq!(machine(&app, 1).slots.output, None);
        assert!(transport::conserved(app.world()));
        assert_eq!(app.world().resource::<State>().unwrap().tick, 59);
        app.advance_fixed(20);
        assert_eq!(app.world().resource::<State>().unwrap().tick, 59);
        restart(&mut app);
        assert!(
            app.world()
                .resource::<State>()
                .unwrap()
                .diagnostic
                .is_none()
        );
        app.world_mut().resource_mut::<State>().unwrap().tick = u64::MAX;
        app.advance_fixed(1);
        assert!(
            app.world()
                .resource::<State>()
                .unwrap()
                .diagnostic
                .as_ref()
                .unwrap()
                .contains("game tick")
        );
    }
}
