#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

extern crate alloc;

mod protocol;
pub mod schedules;

use bevy_app::{App, Plugin};
use bevy_remote::{RemoteMethodSystemId, RemoteMethods, RemotePlugin};

/// Registers read-only schedule inspection methods alongside [`RemotePlugin`].
///
/// Method registration happens at plugin finish and is independent of the
/// order relative to [`RemotePlugin`]. Condition capture is installed for
/// schedules present when this plugin's finish hook runs, preserving valid
/// captures from schedules already observed and built. If a later plugin's
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
        schedules::observe_existing(app.world_mut());
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
