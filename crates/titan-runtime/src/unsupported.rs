use crate::{ExitReason, InitError, RunError, WindowConfig};

pub(crate) struct Application;

impl Application {
    pub(crate) fn new(_config: WindowConfig) -> Result<Self, InitError> {
        Err(InitError::UnsupportedPlatform)
    }

    pub(crate) fn run(&mut self) -> Result<ExitReason, RunError> {
        Err(RunError::UnsupportedPlatform)
    }
}
