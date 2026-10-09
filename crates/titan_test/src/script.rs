use bevy_input::{keyboard::KeyCode, mouse::MouseButton};
use serde::{Deserialize, Serialize};

/// The supported script format version.
pub const SCRIPT_VERSION: u32 = 1;

/// A physical keyboard key or mouse button.
///
/// Future script versions may add gamepad and touch input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputButton {
    /// A physical keyboard key (independent of keyboard layout).
    Key(KeyCode),
    /// A mouse button.
    Mouse(MouseButton),
}

impl From<KeyCode> for InputButton {
    fn from(key: KeyCode) -> Self {
        Self::Key(key)
    }
}

impl From<MouseButton> for InputButton {
    fn from(button: MouseButton) -> Self {
        Self::Mouse(button)
    }
}

/// An input transition applied before a scripted tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputAction {
    /// Press and hold until released.
    Press(InputButton),
    /// Release a held button.
    Release(InputButton),
    /// Press now and release before the next tick, without advancing playback.
    Tap(InputButton),
}

/// An input action on an absolute, zero-based tick index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptEvent {
    /// Number of completed ticks before applying this action.
    pub tick: u64,
    /// The input transition to inject.
    pub action: InputAction,
}

/// A versioned input script suitable for storing in a RON file.
///
/// Events need not be sorted. Events at the same tick execute in file order.
/// Unknown fields are rejected to catch misspelled actions or tick numbers.
/// Playback rejects unsupported versions before advancing the simulation.
///
/// ```
/// use titan_test::InputScript;
/// let script = InputScript::from_ron(r#"(
///     version: 1,
///     events: [
///         (tick: 10, action: Press(Key(Space))),
///         (tick: 11, action: Release(Key(Space))),
///         (tick: 30, action: Tap(Mouse(Left))),
///     ],
/// )"#).unwrap();
/// assert_eq!(script.events.len(), 3);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputScript {
    /// Format version; currently [`SCRIPT_VERSION`].
    pub version: u32,
    /// Input transitions to play back.
    pub events: Vec<ScriptEvent>,
}

impl Default for InputScript {
    fn default() -> Self {
        Self {
            version: SCRIPT_VERSION,
            events: Vec::new(),
        }
    }
}

impl InputScript {
    /// Parse a RON script. Version compatibility is checked at playback time.
    pub fn from_ron(source: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(source)
    }

    /// Serialize a script as human-readable RON.
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }
}
