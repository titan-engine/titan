//! Actual-WASM semantic adapter; the same game runs without GPU dependencies.
use crate::game;
use titan::{
    Startup,
    inspection::{BrowserSession, InspectionConfig},
};
use wasm_bindgen::prelude::*;

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
        let mut config = InspectionConfig::controlled("collection-room-browser", "browser");
        config.mutation_enabled = enable_control;
        let inspector = game::configured_inspector(config);
        Self {
            session: BrowserSession::new(app, inspector, enable_control),
        }
    }
    pub fn handle(&mut self, request_json: &str) -> String {
        self.session.handle(request_json)
    }
}
