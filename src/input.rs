//! Deterministic logical input frames and recordings.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::Hash;

/// Updates a physical button and returns its logical action's combined state.
///
/// The caller owns the bindings and held set; clear the set on focus loss and
/// reset the game's input sampler. Releasing one alias leaves the action pressed
/// while another alias is held. Unmapped buttons are ignored. Bindings must stay
/// stable while buttons are held; clear the set before changing them.
/// This helper has no dependency on a window library or game action schema.
pub fn update_button_alias<K: Copy + Eq + Hash, A: PartialEq>(
    held: &mut HashSet<K>,
    key: K,
    pressed: bool,
    action_for_key: impl Fn(K) -> Option<A>,
) -> Option<(A, bool)> {
    let action = action_for_key(key)?;
    if pressed {
        held.insert(key);
    } else {
        held.remove(&key);
    }
    let active = held
        .iter()
        .any(|key| action_for_key(*key).as_ref() == Some(&action));
    Some((action, active))
}

/// A deterministic signed action value.
///
/// Buttons use zero or [`PRESSED`](Self::PRESSED). Analog actions can use the
/// complete `i16` range without introducing platform-dependent floating-point
/// normalization into recordings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionValue(i16);

impl ActionValue {
    pub const RELEASED: Self = Self(0);
    pub const PRESSED: Self = Self(i16::MAX);

    pub const fn axis(value: i16) -> Self {
        Self(value)
    }

    pub const fn raw(self) -> i16 {
        self.0
    }

    pub const fn is_active(self) -> bool {
        self.0 != 0
    }
}

/// Logical action state consumed by one fixed simulation tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputFrame<A: Ord> {
    values: BTreeMap<A, ActionValue>,
    pressed: BTreeSet<A>,
    released: BTreeSet<A>,
}

impl<A: Ord> Default for InputFrame<A> {
    fn default() -> Self {
        Self {
            values: BTreeMap::new(),
            pressed: BTreeSet::new(),
            released: BTreeSet::new(),
        }
    }
}

impl<A: Ord> InputFrame<A> {
    pub fn value(&self, action: &A) -> ActionValue {
        self.values.get(action).copied().unwrap_or_default()
    }

    pub fn is_active(&self, action: &A) -> bool {
        self.value(action).is_active()
    }

    pub fn just_pressed(&self, action: &A) -> bool {
        self.pressed.contains(action)
    }

    pub fn just_released(&self, action: &A) -> bool {
        self.released.contains(action)
    }

    pub fn active_actions(&self) -> impl Iterator<Item = (&A, ActionValue)> {
        self.values.iter().map(|(action, value)| (action, *value))
    }
}

/// Converts sampled logical action values into transition-aware input frames.
pub struct InputTracker<A: Ord> {
    previous: BTreeMap<A, ActionValue>,
}

impl<A: Ord> Default for InputTracker<A> {
    fn default() -> Self {
        Self {
            previous: BTreeMap::new(),
        }
    }
}

impl<A: Clone + Ord> InputTracker<A> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Produces the next frame from the complete set of currently active
    /// actions. Duplicate actions use the final supplied value.
    pub fn sample(&mut self, values: impl IntoIterator<Item = (A, ActionValue)>) -> InputFrame<A> {
        let values: BTreeMap<_, _> = values
            .into_iter()
            .filter(|(_, value)| value.is_active())
            .collect();
        let pressed = values
            .keys()
            .filter(|action| !self.previous.contains_key(*action))
            .cloned()
            .collect();
        let released = self
            .previous
            .keys()
            .filter(|action| !values.contains_key(*action))
            .cloned()
            .collect();
        self.previous.clone_from(&values);
        InputFrame {
            values,
            pressed,
            released,
        }
    }
}

/// Metadata required to interpret a deterministic input recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordingHeader {
    pub format_version: u32,
    pub fixed_step_nanos: u64,
    pub game_seed: u64,
    pub action_schema_hash: u64,
}

impl RecordingHeader {
    pub const CURRENT_FORMAT_VERSION: u32 = 1;

    pub const fn new(fixed_step_nanos: u64, game_seed: u64, action_schema_hash: u64) -> Self {
        Self {
            format_version: Self::CURRENT_FORMAT_VERSION,
            fixed_step_nanos,
            game_seed,
            action_schema_hash,
        }
    }
}

/// A sequence of logical input frames for exact fixed-tick replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputRecording<A: Ord> {
    header: RecordingHeader,
    frames: Vec<InputFrame<A>>,
}

impl<A: Ord> InputRecording<A> {
    pub const fn new(header: RecordingHeader) -> Self {
        Self {
            header,
            frames: Vec::new(),
        }
    }

    pub const fn header(&self) -> RecordingHeader {
        self.header
    }

    pub fn push(&mut self, frame: InputFrame<A>) {
        self.frames.push(frame);
    }

    pub fn frames(&self) -> &[InputFrame<A>] {
        &self.frames
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{ActionValue, InputRecording, InputTracker, RecordingHeader};

    #[test]
    fn physical_aliases_repeat_release_and_focus_reset() {
        let mut held = std::collections::HashSet::new();
        let mapping = |key| match key {
            1 | 2 => Some("move"),
            3 => Some("jump"),
            _ => None,
        };
        let mut update =
            |key, pressed| super::update_button_alias(&mut held, key, pressed, mapping);
        assert_eq!(update(1, true), Some(("move", true)));
        assert_eq!(update(1, true), Some(("move", true)));
        assert_eq!(update(2, true), Some(("move", true)));
        assert_eq!(update(3, true), Some(("jump", true)));
        assert_eq!(update(1, false), Some(("move", true)));
        assert_eq!(update(2, false), Some(("move", false)));
        assert_eq!(update(9, true), None);
        assert_eq!(held.len(), 1);
        held.clear();
        assert_eq!(
            super::update_button_alias(&mut held, 3, false, mapping),
            Some(("jump", false))
        );
        assert!(held.is_empty());
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Action {
        Move,
        Interact,
    }

    #[test]
    fn tracker_records_edges_and_analog_values() {
        let mut tracker = InputTracker::new();
        let first = tracker.sample([
            (Action::Move, ActionValue::axis(-12_000)),
            (Action::Interact, ActionValue::PRESSED),
        ]);
        let held = tracker.sample([(Action::Move, ActionValue::axis(-8_000))]);
        let released = tracker.sample([]);

        assert!(first.just_pressed(&Action::Move));
        assert!(first.just_pressed(&Action::Interact));
        assert_eq!(first.value(&Action::Move).raw(), -12_000);
        assert!(!held.just_pressed(&Action::Move));
        assert!(held.just_released(&Action::Interact));
        assert!(released.just_released(&Action::Move));
    }

    #[test]
    fn recording_preserves_interpretation_metadata_and_frames() {
        let header = RecordingHeader::new(16_666_667, 42, 0x1234);
        let mut recording = InputRecording::new(header);
        recording.push(InputTracker::new().sample([(Action::Move, ActionValue::PRESSED)]));

        assert_eq!(recording.header(), header);
        assert_eq!(recording.len(), 1);
        assert!(recording.frames()[0].is_active(&Action::Move));
    }
}
