//! Actual-WASM semantic adapter; the same game runs without GPU dependencies.
use crate::game;
use titan::{
    Startup,
    inspection::{BrowserSession, InspectionConfig},
};
use wasm_bindgen::prelude::*;

/// Isolated deterministic fixtures, compiled only into the acceptance build.
#[cfg(feature = "movement-acceptance")]
#[wasm_bindgen]
pub fn movement_acceptance() -> String {
    crate::acceptance::run().to_string()
}

#[wasm_bindgen]
pub struct BrowserRuntime {
    session: BrowserSession,
}

#[wasm_bindgen]
impl BrowserRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new(enable_control: bool) -> Self {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        app.refresh_extracted();
        let mut config = InspectionConfig::controlled("adventure-browser", "browser");
        config.mutation_enabled = enable_control;
        let inspector = game::configured_inspector(config);
        Self {
            session: BrowserSession::new(app, inspector, enable_control),
        }
    }
    /// Accept at a safe point; completion releases the runtime borrow before awaiting.
    #[cfg(target_arch = "wasm32")]
    pub fn dispatch(&mut self, request_json: &str) -> titan::inspection::BrowserPromise {
        titan::inspection::response_promise(self.session.capture_timeout(), || {
            self.session.dispatch_json(request_json)
        })
    }

    pub fn handle(&mut self, request_json: &str) -> String {
        self.session.handle(request_json)
    }
}

#[cfg(feature = "movement-acceptance")]
#[wasm_bindgen]
pub fn puzzle_acceptance() -> String {
    crate::puzzle_acceptance::run().to_string()
}

#[cfg(feature = "movement-acceptance")]
#[wasm_bindgen]
pub fn block_acceptance() -> String {
    crate::block_acceptance::run().to_string()
}

#[cfg(feature = "movement-acceptance")]
#[wasm_bindgen]
pub fn sequence_acceptance() -> String {
    crate::sequence_acceptance::run().to_string()
}
