#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::ToString;
use core::{mem, time::Duration};

use bevy_app::{App, First, Plugin};
use bevy_diagnostic::{FrameCount, FrameCountPlugin};
use bevy_ecs::prelude::*;
use bevy_remote::{
    error_codes, BrpError, BrpResult, RemoteLast, RemoteMethodSystemId, RemoteMethods,
    RemotePlugin, RemoteSystems,
};
use bevy_time::{Time, TimePlugin, TimeSystems, TimeUpdateStrategy, Virtual};
use serde::Deserialize;
use serde_json::{json, Value};

#[cfg(feature = "render")]
mod screenshot;

/// Adds Titan's time-control methods and, with `render`, screenshot methods to BRP.
///
/// Add alongside `RemotePlugin::default()`, `TimePlugin`, and `FrameCountPlugin`
/// (the latter two are included in Bevy's standard plugin groups). Registration
/// happens in [`Plugin::finish`], so plugin order does not affect method registration.
#[derive(Default)]
pub struct TitanRemotePlugin;

/// JSON-RPC application error returned when a step is already in progress.
pub const STEP_IN_PROGRESS: i16 = -32000;

impl Plugin for TitanRemotePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StepState>()
            .add_systems(First, begin_step_frame.before(TimeSystems))
            .add_systems(
                RemoteLast,
                finish_step_frame.before(RemoteSystems::ProcessRequests),
            );
    }

    fn finish(&self, app: &mut App) {
        assert!(
            app.is_plugin_added::<RemotePlugin>(),
            "TitanRemotePlugin requires RemotePlugin"
        );
        assert!(
            app.is_plugin_added::<TimePlugin>(),
            "TitanRemotePlugin requires TimePlugin"
        );
        assert!(
            app.is_plugin_added::<FrameCountPlugin>(),
            "TitanRemotePlugin requires FrameCountPlugin"
        );
        let handlers = [
            ("titan.status", app.world_mut().register_system(status)),
            ("titan.pause", app.world_mut().register_system(pause)),
            ("titan.resume", app.world_mut().register_system(resume)),
            ("titan.step", app.world_mut().register_system(step)),
        ];
        for (name, handler) in handlers {
            app.world_mut()
                .resource_mut::<RemoteMethods>()
                .insert(name, RemoteMethodSystemId::Instant(handler));
        }
        #[cfg(feature = "render")]
        screenshot::register(app);
    }
}

#[derive(Resource, Default)]
struct StepState {
    active: Option<Steps>,
    frame_started: bool,
}

struct Steps {
    remaining: u32,
    dt: Duration,
    previous_strategy: TimeUpdateStrategy,
    previous_speed: f64,
    previous_max_delta: Duration,
}

fn invalid_params(message: impl ToString) -> BrpError {
    BrpError {
        code: error_codes::INVALID_PARAMS,
        message: message.to_string(),
        data: None,
    }
}

fn no_params(params: Option<Value>) -> BrpResult<()> {
    match params {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Object(map)) if map.is_empty() => Ok(()),
        _ => Err(invalid_params("This method takes no parameters")),
    }
}

fn status_value(world: &World) -> Value {
    json!({
        "paused": world.resource::<Time<Virtual>>().is_paused(),
        "frame": world.resource::<FrameCount>().0,
        "pending_steps": world.resource::<StepState>().active.as_ref().map_or(0, |steps| steps.remaining),
    })
}

fn status(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    no_params(params)?;
    Ok(status_value(world))
}

fn cancel_steps(world: &mut World) {
    let steps = world.resource_mut::<StepState>().active.take();
    if let Some(steps) = steps {
        world.insert_resource(steps.previous_strategy);
        let mut time = world.resource_mut::<Time<Virtual>>();
        time.set_relative_speed_f64(steps.previous_speed);
        time.set_max_delta(steps.previous_max_delta);
    }
    world.resource_mut::<StepState>().frame_started = false;
}

fn pause(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    no_params(params)?;
    cancel_steps(world);
    world.resource_mut::<Time<Virtual>>().pause();
    Ok(status_value(world))
}

fn resume(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    no_params(params)?;
    cancel_steps(world);
    world.resource_mut::<Time<Virtual>>().unpause();
    Ok(status_value(world))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepParams {
    frames: u32,
    #[serde(default = "default_dt")]
    dt_secs: f32,
}

fn default_dt() -> f32 {
    1.0 / 60.0
}

fn step(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params = params
        .filter(Value::is_object)
        .ok_or_else(|| invalid_params("Expected an object { frames, dt_secs? }"))?;
    let params: StepParams = serde_json::from_value(params).map_err(invalid_params)?;
    // Bound the amount of fixed-update work one frame can request.
    if !params.dt_secs.is_finite() || params.dt_secs <= 0.0 || params.dt_secs > 1.0 {
        return Err(invalid_params("dt_secs must be finite and in (0, 1]"));
    }
    let dt = Duration::from_secs_f32(params.dt_secs);
    if dt.is_zero() {
        return Err(invalid_params("dt_secs must be at least one nanosecond"));
    }
    if world.resource::<StepState>().active.is_some() {
        return Err(BrpError {
            code: STEP_IN_PROGRESS,
            message: "A step is already pending; pause or resume to cancel it".to_string(),
            data: None,
        });
    }
    let target_frame = world.resource::<FrameCount>().0.wrapping_add(params.frames);
    if params.frames == 0 {
        world.resource_mut::<Time<Virtual>>().pause();
    } else {
        let previous_strategy = mem::replace(
            &mut *world.resource_mut::<TimeUpdateStrategy>(),
            TimeUpdateStrategy::ManualDuration(dt),
        );
        let time = world.resource::<Time<Virtual>>();
        let steps = Steps {
            remaining: params.frames,
            dt,
            previous_strategy,
            previous_speed: time.relative_speed_f64(),
            previous_max_delta: time.max_delta(),
        };
        world.resource_mut::<StepState>().active = Some(steps);
    }
    Ok(json!({ "target_frame": target_frame }))
}

fn begin_step_frame(
    mut state: ResMut<StepState>,
    mut time: ResMut<Time<Virtual>>,
    mut strategy: ResMut<TimeUpdateStrategy>,
) {
    if let Some(steps) = &state.active {
        *strategy = TimeUpdateStrategy::ManualDuration(steps.dt);
        time.set_relative_speed(1.0);
        time.set_max_delta(steps.dt);
        time.unpause();
        state.frame_started = true;
    }
}

fn finish_step_frame(world: &mut World) {
    let mut state = world.resource_mut::<StepState>();
    if !mem::take(&mut state.frame_started) {
        return;
    }
    let Some(steps) = state.active.as_mut() else {
        return;
    };
    steps.remaining -= 1;
    if steps.remaining == 0 {
        cancel_steps(world);
        world.resource_mut::<Time<Virtual>>().pause();
    }
}

#[cfg(test)]
mod tests;
