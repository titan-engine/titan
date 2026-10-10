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
/// Method registration happens at plugin finish and is independent of the
/// order relative to [`RemotePlugin`]. Condition capture is installed for
/// schedules present when this plugin's finish hook runs. If a later plugin's
/// finish hook creates/replaces schedules, or schedules are added afterward,
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
