#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

extern crate alloc;

mod protocol;
pub mod schedules;

use bevy_app::{App, Plugin};
use bevy_ecs::schedule::Schedules;
use bevy_remote::{RemoteMethodSystemId, RemoteMethods, RemotePlugin};

/// Registers read-only schedule inspection methods alongside [`RemotePlugin`].
///
/// Registration and condition-capture installation happen at plugin finish, so
/// plugin order does not matter. For schedules added or replaced after finish,
/// call [`schedules::observe_schedule`] before their first build.
#[derive(Default)]
pub struct InspectPlugin;

impl Plugin for InspectPlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        assert!(
            app.is_plugin_added::<RemotePlugin>(),
            "InspectPlugin requires RemotePlugin"
        );
        for (_, schedule) in app.world_mut().resource_mut::<Schedules>().iter_mut() {
            schedules::observe_schedule(schedule);
        }
        let handlers = [
            (
                "titan.schedules",
                app.world_mut().register_system(schedules::list),
            ),
            (
                "titan.systems",
                app.world_mut().register_system(schedules::systems),
            ),
            (
                "titan.ambiguities",
                app.world_mut().register_system(schedules::ambiguities),
            ),
        ];
        for (name, handler) in handlers {
            app.world_mut()
                .resource_mut::<RemoteMethods>()
                .insert(name, RemoteMethodSystemId::Instant(handler));
        }
    }
}
