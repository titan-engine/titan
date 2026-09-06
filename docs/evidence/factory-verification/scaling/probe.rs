//! Disposable public-API probe, copied into a pinned source archive by measure.py.
use serde_json::{Value, json};
use std::{fs, time::Instant};
use titan::{App, Startup};
use titan_factory::game;
fn state(a: &App) -> Value {
    serde_json::from_str(&game::status(a)).unwrap()
}
fn check(s: &Value) {
    let resident = s["structures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            ["input", "in_process", "output"]
                .iter()
                .filter(|k| !b["slots"][**k].is_null())
                .count() as u64
        })
        .sum::<u64>();
    let n = |k: &str| s[k].as_u64().unwrap();
    assert_eq!(
        n("seeded") + n("extracted"),
        resident + n("delivered") + n("discarded_ore") + n("discarded_plate")
    );
    assert!(s["diagnostic"].is_null());
}
fn main() {
    let fixture: Value =
        serde_json::from_str(&fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap())
            .unwrap();
    let mut traces = Vec::new();
    let mut samples = Vec::new();
    for repeat in 0..3 {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        let start = Instant::now();
        for op in fixture["operations"].as_array().unwrap() {
            game::player_command(&mut app, &op.to_string()).unwrap();
            check(&state(&app));
        }
        let setup_seconds = start.elapsed().as_secs_f64();
        let initial = state(&app);
        let mut advance_seconds = 0.0;
        for tick in 0..600 {
            let now = Instant::now();
            game::player_command(&mut app, r#"{"op":"advance","ticks":1}"#).unwrap();
            advance_seconds += now.elapsed().as_secs_f64();
            let s = state(&app);
            check(&s);
            assert_eq!(
                s["tick"].as_u64().unwrap(),
                initial["tick"].as_u64().unwrap() + tick + 1
            );
            let full = s.to_string();
            if repeat == 0 {
                traces.push(full);
            } else {
                assert_eq!(traces[tick as usize], full);
            }
        }
        let final_state = state(&app);
        if fixture["name"] == "reference_active" {
            assert_eq!(final_state["delivered"], 9);
            assert_eq!(final_state["tick"], 1200);
        } else if fixture["name"] == "long_active" || fixture["name"] == "dense_active" {
            let plates = final_state["structures"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|b| b["slots"]["output"] == "plate")
                .count();
            assert_eq!(plates, 9);
            assert_eq!(final_state["delivered"], 0);
            // The first plate has traversed the 42-hop output route to its disconnected end.
            let tail = final_state["structures"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["x"] == 0 && b["y"] == 7)
                .unwrap();
            assert_eq!(tail["slots"]["output"], "plate");
        } else {
            // Long warmup fills every transport slot; work and inventory stay stalled.
            for key in [
                "structures",
                "extracted",
                "delivered",
                "discarded_ore",
                "discarded_plate",
            ] {
                assert_eq!(initial[key], final_state[key]);
            }
        }
        let now = Instant::now();
        for _ in 0..20 {
            assert_eq!(state(&app), final_state);
        }
        let inspection_seconds = now.elapsed().as_secs_f64();
        let now = Instant::now();
        let mut checksum = None;
        for _ in 0..10 {
            let image = game::render_image(app.world()).unwrap();
            let c = game::image_checksum(&image);
            if let Some(previous) = checksum {
                assert_eq!(c, previous);
            }
            checksum = Some(c);
        }
        let capture_seconds = now.elapsed().as_secs_f64();
        assert_eq!(state(&app), final_state);
        samples.push(json!({"repeat":repeat,"setup_and_warmup_seconds":setup_seconds,"advance_600_one_tick_commands_seconds":advance_seconds,"inspect_20_with_parse_and_equality_seconds":inspection_seconds,"software_render_10_with_checksum_seconds":capture_seconds,"software_checksum":format!("{:016x}",checksum.unwrap()),"initial":initial,"final":final_state}));
    }
    println!(
        "{}",
        json!({"name":fixture["name"],"samples":samples,"semantic":{"independent_slot_conservation_every_measured_tick":true,"full_state_exact_equality_at_every_measured_tick_across_three_runs":true,"inspection_and_capture_preserve_full_state":true,"production_or_stall_assertions":true},"ticks_per_sample":600})
    );
}
