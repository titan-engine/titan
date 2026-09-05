//! Browser adapter for the same procedural RPG used by native acceptance tests.
//!
//! JavaScript owns the same-origin message boundary. Each synchronous `handle`
//! call owns the application exclusively; no simulation tick runs between calls.

use titan::{
    App, Startup,
    inspection::{BrowserSession, InspectionConfig},
};
use titan_protocol::{CaptureResult, ProtocolError};
use wasm_bindgen::prelude::*;

#[path = "../../../examples/support/procedural_rpg.rs"]
pub mod game;

/// An isolated, paused game instance. Controls require an explicit `true`.
#[wasm_bindgen]
pub struct BrowserRuntime {
    session: BrowserSession,
}

#[wasm_bindgen]
impl BrowserRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new(enable_control: bool) -> Self {
        Self::from_app(game::build_game(), enable_control)
    }

    /// Compatibility path: replace the player and generate the reference tree.
    pub fn with_player_png(enable_control: bool, bytes: &[u8]) -> Result<Self, JsValue> {
        Ok(Self::from_app(player_png_app(bytes)?, enable_control))
    }

    /// Decode the complete pair before creating a world.
    pub fn with_pngs(enable_control: bool, player: &[u8], tree: &[u8]) -> Result<Self, JsValue> {
        Ok(Self::from_app(pngs_app(player, tree)?, enable_control))
    }

    /// Executes one request at a safe point and returns its JSON response envelope.
    pub fn handle(&mut self, request_json: &str) -> String {
        self.session.handle(request_json)
    }
    /// Accept synchronously; the returned Promise owns completion, not the player.
    #[cfg(target_arch = "wasm32")]
    pub fn dispatch(&mut self, request_json: &str) -> titan::inspection::BrowserPromise {
        titan::inspection::response_promise(self.session.capture_timeout(), || {
            self.session.dispatch_json(request_json)
        })
    }
}

impl BrowserRuntime {
    fn from_app(mut app: App, enable_control: bool) -> Self {
        app.update_schedule(Startup);
        let config = InspectionConfig::controlled("procedural-rpg-browser", "procedural-rpg");
        let inspector = game::inspector_with_capture(config, capture);
        Self {
            session: BrowserSession::new(app, inspector, enable_control),
        }
    }
}

/// Headless adapter for exercising the same live session under actual WASM.
/// GPU presentation is verified separately through BrowserPlayer.
#[wasm_bindgen]
pub struct BrowserLiveRuntime {
    session: game::live::RpgSession,
}

#[wasm_bindgen]
impl BrowserLiveRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            session: live_session(),
        }
    }
    /// Compatibility path: replace the player and generate the reference tree.
    pub fn with_player_png(bytes: &[u8]) -> Result<Self, JsValue> {
        Ok(Self {
            session: live_session_from_app(player_png_app(bytes)?),
        })
    }
    pub fn with_pngs(player: &[u8], tree: &[u8]) -> Result<Self, JsValue> {
        Ok(Self {
            session: live_session_from_app(pngs_app(player, tree)?),
        })
    }
    pub fn handle(&mut self, request_json: &str) -> String {
        self.session.handle_json(request_json)
    }
    /// Accept synchronously; the returned Promise owns completion, not the player.
    #[cfg(target_arch = "wasm32")]
    pub fn dispatch(&mut self, request_json: &str) -> titan::inspection::BrowserPromise {
        titan::inspection::response_promise(self.session.capture_timeout(), || {
            self.session.dispatch_json(request_json)
        })
    }
    pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), JsValue> {
        self.session
            .set_action(name, pressed)
            .map_err(|error| JsValue::from_str(&error))
    }
    pub fn journal_open(&self) -> bool {
        self.session.journal_open()
    }
    pub fn journal_key(&mut self, key: &str) -> bool {
        self.session.journal_key(key)
    }
    /// Headless gestures use logical framebuffer coordinates.
    pub fn journal_pointer(&mut self, x: i32, y: i32, pressed: bool) -> bool {
        self.session.journal_pointer(Some((x, y)), pressed)
    }
    pub fn cancel_journal_input(&mut self) {
        self.session.cancel_journal_input();
    }
    pub fn paused(&self) -> bool {
        self.session.paused()
    }
    pub fn clock_epoch(&self) -> String {
        self.session.clock_epoch().to_string()
    }
    pub fn tick(&mut self) {
        self.session.tick();
    }
    pub fn resume(&mut self) {
        self.session.resume();
    }
    pub fn pause(&mut self) {
        self.session.pause();
    }
    pub fn set_control_enabled(&mut self, enabled: bool) {
        self.session.set_control_enabled(enabled);
    }
    pub fn load_recording(&mut self, json: &str) -> Result<(), JsValue> {
        self.session
            .load_replay(parse_recording_json(json)?)
            .map_err(|error| JsValue::from_str(&error.message))
    }
    pub fn playback_active(&self) -> bool {
        self.session.replay_active()
    }
    pub fn playback_status(&self) -> String {
        self.session.replay_status().to_string()
    }
    pub fn step_playback(&mut self) -> Result<(), JsValue> {
        self.session
            .step_replay()
            .map_err(|error| JsValue::from_str(&error.message))
    }
    pub fn restart_playback(&mut self) -> Result<(), JsValue> {
        self.session
            .restart_replay()
            .map_err(|error| JsValue::from_str(&error.message))
    }
    pub fn exit_playback(&mut self) -> Result<(), JsValue> {
        self.session
            .stop_replay()
            .map_err(|error| JsValue::from_str(&error.message))
    }
}

impl Default for BrowserLiveRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Replay an exported recording in a fresh headless game, with a bounded input.
#[wasm_bindgen]
pub fn verify_recording_json(recording_json: &str) -> Result<String, JsValue> {
    if recording_json.len() > 2 * 1024 * 1024 {
        return Err(JsValue::from_str("recording exceeds the 2 MiB size bound"));
    }
    let result = serde_json::from_str(recording_json)
        .map_err(|error| error.to_string())
        .and_then(game::live::verify_recording)
        .and_then(|value| serde_json::to_string(&value).map_err(|error| error.to_string()));
    result.map_err(|error| JsValue::from_str(&error))
}

/// Compatibility verifier using a supplied player and the generated reference tree.
#[wasm_bindgen]
pub fn verify_recording_json_with_player_png(
    recording_json: &str,
    bytes: &[u8],
) -> Result<String, JsValue> {
    let recording = parse_recording_json(recording_json)?;
    let image =
        game::assets::decode_player_png(bytes).map_err(|error| JsValue::from_str(&error))?;
    game::live::verify_recording_with_player(recording, image)
        .map(|value| value.to_string())
        .map_err(|error| JsValue::from_str(&error))
}

/// Verify a fresh replay against both supplied startup sprites.
#[wasm_bindgen]
pub fn verify_recording_json_with_pngs(
    recording_json: &str,
    player: &[u8],
    tree: &[u8],
) -> Result<String, JsValue> {
    let recording = parse_recording_json(recording_json)?;
    let images =
        game::assets::decode_images(player, tree).map_err(|error| JsValue::from_str(&error))?;
    game::live::verify_recording_with_images(recording, images)
        .map(|value| value.to_string())
        .map_err(|error| JsValue::from_str(&error))
}

fn pngs_app(player: &[u8], tree: &[u8]) -> Result<App, JsValue> {
    let images =
        game::assets::decode_images(player, tree).map_err(|error| JsValue::from_str(&error))?;
    Ok(game::build_game_with_images(images))
}

fn parse_recording_json(json: &str) -> Result<serde_json::Value, JsValue> {
    if json.len() > 2 * 1024 * 1024 {
        return Err(JsValue::from_str("recording exceeds the 2 MiB size bound"));
    }
    serde_json::from_str(json).map_err(|error| JsValue::from_str(&error.to_string()))
}

fn player_png_app(bytes: &[u8]) -> Result<App, JsValue> {
    let image =
        game::assets::decode_player_png(bytes).map_err(|error| JsValue::from_str(&error))?;
    Ok(game::build_game_with_player(image))
}

fn live_session() -> game::live::RpgSession {
    live_session_from_app(game::build_game())
}

fn live_session_from_app(mut app: App) -> game::live::RpgSession {
    app.update_schedule(Startup);
    let mut config = InspectionConfig::controlled("rpg-live-browser", "procedural-rpg");
    config.run_mode = titan_protocol::RunMode::Browser;
    let inspector = game::inspector_with_capture(config, capture);
    game::live::RpgSession::new(app, inspector, false)
}

fn capture(app: &App) -> Result<CaptureResult, ProtocolError> {
    let image = game::render_image(app.world())?;
    titan_diagnostics::png_capture(&image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use titan_protocol::ErrorCode;
    use titan_protocol::{EntityId, InputValue};
    use titan_protocol::{
        Operation, Request, RequestEnvelope, Response, ResponseEnvelope, ResponseOutcome, RunMode,
    };

    fn call(runtime: &mut BrowserRuntime, request: Request) -> ResponseEnvelope {
        serde_json::from_str(
            &runtime
                .handle(&serde_json::to_string(&RequestEnvelope::new("test", request)).unwrap()),
        )
        .unwrap()
    }

    fn success(response: ResponseEnvelope) -> Response {
        match response.outcome {
            ResponseOutcome::Success { response } => response,
            other => panic!("expected success: {other:?}"),
        }
    }

    #[test]
    fn read_only_policy_covers_every_mutation_and_preserves_validation() {
        let mut runtime = BrowserRuntime::new(false);
        let Response::Capabilities(capabilities) =
            success(call(&mut runtime, Request::Capabilities))
        else {
            panic!("capabilities")
        };
        assert_eq!(
            capabilities.operations,
            [Operation::Inspect, Operation::Query, Operation::Capture]
        );
        assert_eq!(capabilities.run_mode, RunMode::Browser);
        assert!(!capabilities.mutation_enabled);
        assert_eq!(
            success(call(&mut runtime, Request::Commands)),
            Response::Commands { commands: vec![] }
        );
        for request in [
            Request::Step { frames: 1 },
            Request::InjectInput {
                frame: 1,
                actions: Default::default(),
            },
            Request::Invoke {
                name: "spawn_shard".into(),
                arguments: Default::default(),
            },
            Request::SetField {
                entity: EntityId {
                    index: 0,
                    generation: 0,
                },
                component: "Position".into(),
                field: "x".into(),
                value: 0.into(),
            },
        ] {
            let response = call(&mut runtime, request);
            assert_eq!((response.observed_frame, response.state_revision), (0, 0));
            assert!(
                matches!(response.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::MutationDisabled)
            );
        }
        let mut request = RequestEnvelope::new("mismatch", Request::Step { frames: 1 });
        request.schema_version = 999;
        let response: ResponseEnvelope =
            serde_json::from_str(&runtime.handle(&serde_json::to_string(&request).unwrap()))
                .unwrap();
        assert!(
            matches!(response.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::ProtocolMismatch)
        );
        request.schema_version = titan_protocol::SCHEMA_VERSION;
        request.target_instance = Some("wrong-instance".into());
        let response: ResponseEnvelope =
            serde_json::from_str(&runtime.handle(&serde_json::to_string(&request).unwrap()))
                .unwrap();
        assert!(
            matches!(response.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::NotFound)
        );
        for malformed in [
            "not json",
            r#"{"request_id":"invalid","request":{"type":"nope"}}"#,
        ] {
            let response: ResponseEnvelope =
                serde_json::from_str(&runtime.handle(malformed)).unwrap();
            assert_eq!((response.observed_frame, response.state_revision), (0, 0));
            assert!(
                matches!(response.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::InvalidValue)
            );
        }
    }

    #[test]
    fn browser_capture_decodes_to_exact_reference_pixels() {
        let mut runtime = BrowserRuntime::new(true);
        let Response::Capabilities(capabilities) =
            success(call(&mut runtime, Request::Capabilities))
        else {
            panic!("capabilities");
        };
        assert!(capabilities.mutation_enabled);
        assert!(capabilities.operations.contains(&Operation::Mutate));
        for operation in [Operation::Step, Operation::Invoke, Operation::InjectInput] {
            assert!(capabilities.operations.contains(&operation));
        }

        let mut frame = 0;
        for (action, ticks) in [("right", 2), ("down", 3), ("right", 6)] {
            for _ in 0..ticks {
                frame += 1;
                success(call(
                    &mut runtime,
                    Request::InjectInput {
                        frame,
                        actions: [(action.into(), InputValue::Button(true))].into(),
                    },
                ));
            }
        }
        let stepped = call(&mut runtime, Request::Step { frames: 11 });
        assert_eq!((stepped.observed_frame, stepped.state_revision), (11, 12));
        let Response::Capture(capture) = success(call(&mut runtime, Request::Capture)) else {
            panic!("capture")
        };
        assert_eq!(capture.checksum, "f7a298f62ad75c1c");
        let bytes = STANDARD
            .decode(
                capture
                    .artifact
                    .strip_prefix("data:image/png;base64,")
                    .unwrap(),
            )
            .unwrap();
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
        let info = decoder.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (160, 112));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let decoded = titan::render::Image::new(info.width, info.height, pixels).unwrap();
        assert_eq!(game::image_checksum(&decoded), 0xf7a2_98f6_2ad7_5c1c);
        success(call(
            &mut runtime,
            Request::Invoke {
                name: "spawn_shard".into(),
                arguments: [("x".into(), 0.into()), ("y".into(), 0.into())].into(),
            },
        ));
        let Response::Capture(changed) = success(call(&mut runtime, Request::Capture)) else {
            panic!("capture")
        };
        assert_ne!(changed.checksum, capture.checksum);
    }
}

#[cfg(target_arch = "wasm32")]
mod player;
#[cfg(target_arch = "wasm32")]
pub use player::BrowserPlayer;
