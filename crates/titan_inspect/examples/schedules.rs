//! Prints the three inspection responses from a headless BRP app.
#![expect(
    clippy::print_stdout,
    reason = "This example prints the inspection document"
)]

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use bevy_remote::{BrpMessage, BrpSender, RemotePlugin};
use serde_json::json;
use titan_inspect::InspectPlugin;

#[derive(Resource, Default)]
struct Counter(u32);
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct Gameplay;

fn increment(mut counter: ResMut<Counter>) {
    counter.0 += 1;
}
fn reset(mut counter: ResMut<Counter>) {
    counter.0 = 0;
}
fn report(_counter: Res<Counter>) {}
fn playing() -> bool {
    true
}

fn main() {
    let mut app = App::new();
    app.init_resource::<Counter>()
        .add_plugins((RemotePlugin::default(), InspectPlugin))
        .configure_sets(Update, Gameplay.run_if(playing))
        .add_systems(
            Update,
            (increment.in_set(Gameplay).before(report), reset, report),
        );
    app.finish();
    app.cleanup();
    app.update();
    for method in ["titan.schedules", "titan.systems", "titan.ambiguities"] {
        let (sender, receiver) = async_channel::bounded(1);
        let params = (method != "titan.schedules").then(|| json!({"schedule":"Update"}));
        app.world()
            .resource::<BrpSender>()
            .try_send(BrpMessage {
                method: method.to_owned(),
                params,
                sender,
            })
            .unwrap();
        app.update();
        let result = receiver.try_recv().unwrap().unwrap();
        println!(
            "{method}:\n{}",
            serde_json::to_string_pretty(&result).unwrap()
        );
    }
}
