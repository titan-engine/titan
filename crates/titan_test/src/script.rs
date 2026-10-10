use bevy_input::{
    gamepad::{GamepadAxis, GamepadButton},
    keyboard::KeyCode,
    mouse::MouseButton,
};
use serde::{Deserialize, Serialize};

/// The current script format version. Version 1 remains supported.
pub const SCRIPT_VERSION: u32 = 2;

/// A stable virtual gamepad identifier, independent of Bevy entity IDs.
///
/// Slots are local to a [`crate::Sim`]. Scripts may use any slot number; the
/// first input for a slot connects it automatically. Serializes as a plain u32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GamepadSlot(pub u32);

impl GamepadSlot {
    /// Identify a button on this gamepad for press, release, or tap.
    pub fn button(self, button: GamepadButton) -> InputButton {
        InputButton::Gamepad { slot: self, button }
    }
}

/// A physical keyboard key, mouse button, or virtual gamepad button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum InputButton {
    /// A physical keyboard key (independent of keyboard layout).
    Key(KeyCode),
    /// A mouse button.
    Mouse(MouseButton),
    /// A button on a virtual gamepad (version 2).
    Gamepad {
        /// Stable gamepad slot, connected automatically on first use.
        slot: GamepadSlot,
        /// The physical gamepad button.
        button: GamepadButton,
    },
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum InputAction {
    /// Press and hold until released.
    Press(InputButton),
    /// Release a held button.
    Release(InputButton),
    /// Press now and release before the next tick, without advancing playback.
    Tap(InputButton),
    /// Connect a neutral gamepad without sending button or axis input (version 2).
    /// Connecting an existing slot is a no-op.
    ConnectGamepad {
        /// Stable gamepad slot.
        slot: GamepadSlot,
    },
    /// Set a held raw axis value, filtered by Bevy's gamepad settings (version 2).
    SetAxis {
        /// Stable gamepad slot, connected automatically on first use.
        slot: GamepadSlot,
        /// The gamepad axis.
        axis: GamepadAxis,
        /// Finite raw value in -1.0..=1.0; persists until changed.
        value: f32,
    },
    /// Set a raw analog button value, including triggers (version 2).
    SetButtonValue {
        /// Stable gamepad slot, connected automatically on first use.
        slot: GamepadSlot,
        /// The gamepad button.
        button: GamepadButton,
        /// Finite raw value in 0.0..=1.0; persists until changed.
        value: f32,
    },
    /// Add a raw mouse delta for this update only (version 2).
    MouseMotion {
        /// Finite horizontal delta in pixels.
        x: f32,
        /// Finite vertical delta in pixels.
        y: f32,
    },
}

impl InputAction {
    pub(crate) fn validate(self, version: u32) {
        if version == 1 {
            assert!(
                matches!(
                    self,
                    Self::Press(InputButton::Key(_) | InputButton::Mouse(_))
                        | Self::Release(InputButton::Key(_) | InputButton::Mouse(_))
                        | Self::Tap(InputButton::Key(_) | InputButton::Mouse(_))
                ),
                "gamepad and mouse-motion actions require script version 2"
            );
        }
        match self {
            Self::SetAxis { value, .. } => assert!(
                value.is_finite() && (-1.0..=1.0).contains(&value),
                "gamepad axis value must be finite and in -1.0..=1.0"
            ),
            Self::SetButtonValue { value, .. } => assert!(
                value.is_finite() && (0.0..=1.0).contains(&value),
                "gamepad button value must be finite and in 0.0..=1.0"
            ),
            Self::MouseMotion { x, y } => {
                assert!(x.is_finite() && y.is_finite(), "mouse delta must be finite");
            }
            _ => {}
        }
    }
}

/// An input action on an absolute, zero-based tick index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputScript {
    /// Format version: 1 (keyboard/mouse buttons) or [`SCRIPT_VERSION`].
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
    /// Validate version compatibility and every event without playing the script.
    ///
    /// Supports versions 1 (keyboard/mouse buttons) and 2 (all input actions).
    /// Events are checked even if they would be outside a playback interval.
    /// Consumers should call this rather than compare against [`SCRIPT_VERSION`],
    /// which identifies the current format, not all supported formats.
    ///
    /// # Panics
    /// Panics for unsupported versions, version 2 actions in a version 1 script,
    /// nonfinite mouse deltas, or analog values outside their documented ranges.
    /// [`crate::Sim::run_script`] uses this same validation before playback.
    ///
    /// ```
    /// use titan_test::InputScript;
    /// let script = InputScript::from_ron("(version:1,events:[])").unwrap();
    /// script.validate(); // Older supported formats remain valid.
    /// ```
    #[track_caller]
    pub fn validate(&self) {
        assert!(
            matches!(self.version, 1 | SCRIPT_VERSION),
            "unsupported script version {}",
            self.version
        );
        for event in &self.events {
            event.action.validate(self.version);
        }
    }

    /// Parse a RON script. Call [`Self::validate`] to check compatibility without
    /// playback; parsing itself only checks the syntax and field names.
    pub fn from_ron(source: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(source)
    }

    /// Serialize a script as human-readable RON.
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }
}
