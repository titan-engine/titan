//! Bounded snapshot-backed recordings of consumed digital input.
//!
//! Snapshots remain opaque game-owned JSON: games validate and restore them,
//! execute the simulation, and compare their final state and pixels. These
//! helpers own button edges, file bounds, recording caps and playback cursors;
//! they never advance an App or change its host clock. Analog input is rejected.
use crate::input::{ActionValue, InputFrame, InputTracker};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    io::{self, Write},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecordedButtons {
    pub active: Vec<String>,
    pub pressed: Vec<String>,
    pub released: Vec<String>,
}

impl RecordedButtons {
    /// Capture the exact consumed edges, including a held start with no press.
    /// The schema must name every action used by this digital input source.
    pub fn capture<A: Clone + Ord>(
        input: &InputFrame<A>,
        schema: &[(A, &str)],
    ) -> Result<Self, String> {
        validate_schema(schema)?;
        if input.active_actions().any(|(action, value)| {
            value != ActionValue::PRESSED || !schema.iter().any(|(known, _)| known == action)
        }) {
            return Err("recording requires mapped digital button values".into());
        }
        if input
            .released_actions()
            .any(|action| !schema.iter().any(|(known, _)| known == action))
        {
            return Err("recording requires mapped release edges".into());
        }
        Ok(Self {
            active: schema
                .iter()
                .filter(|(a, _)| input.is_active(a))
                .map(|(_, name)| (*name).into())
                .collect(),
            pressed: schema
                .iter()
                .filter(|(a, _)| input.just_pressed(a))
                .map(|(_, name)| (*name).into())
                .collect(),
            released: schema
                .iter()
                .filter(|(a, _)| input.just_released(a))
                .map(|(_, name)| (*name).into())
                .collect(),
        })
    }

    /// Decode each frame independently. Host samplers may release and repress
    /// between ticks, so adjacent frames need not imply continuous held input.
    pub fn decode<A: Clone + Ord>(&self, schema: &[(A, &str)]) -> Result<InputFrame<A>, String> {
        validate_schema(schema)?;
        let decode = |names: &[String]| -> Result<Vec<A>, String> {
            if names.len() > schema.len() {
                return Err("too many actions in recorded frame".into());
            }
            let mut actions = BTreeSet::new();
            for name in names {
                let action = schema
                    .iter()
                    .find(|(_, known)| name == known)
                    .map(|(a, _)| a.clone())
                    .ok_or_else(|| format!("unknown recorded action: {name}"))?;
                if !actions.insert(action) {
                    return Err("duplicate recorded action".into());
                }
            }
            Ok(actions.into_iter().collect())
        };
        let active = decode(&self.active)?;
        let pressed = decode(&self.pressed)?;
        let released = decode(&self.released)?;
        if pressed.iter().any(|a| !active.contains(a))
            || released.iter().any(|a| active.contains(a))
        {
            return Err("recorded edges conflict with active actions".into());
        }
        let mut tracker = InputTracker::new();
        tracker.sample(
            active
                .iter()
                .filter(|a| !pressed.contains(a))
                .cloned()
                .chain(released)
                .map(|a| (a, ActionValue::PRESSED)),
        );
        Ok(tracker.sample(active.into_iter().map(|a| (a, ActionValue::PRESSED))))
    }
}

fn validate_schema<A: Ord>(schema: &[(A, &str)]) -> Result<(), String> {
    let mut actions = BTreeSet::new();
    let mut names = BTreeSet::new();
    if schema
        .iter()
        .any(|(action, name)| name.is_empty() || !actions.insert(action) || !names.insert(name))
    {
        return Err("recording action schema must have unique actions and nonempty names".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct RecordingIdentity<'a> {
    pub game_seed: u64,
    pub action_schema: &'a str,
    pub fixed_step_nanos: u64,
    pub max_ticks: usize,
}

/// Portable digital recording envelope. Version 1 has a game-defined canonical
/// origin; version 2 requires both non-null snapshots. Parsing does not validate
/// their game-defined contents. A diagnostic export may have a missing final
/// snapshot, but parsing it for replay rejects that incomplete artifact.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRecording {
    pub format_version: u32,
    pub game_seed: u64,
    pub action_schema: String,
    pub fixed_step_nanos: u64,
    pub start_host_frame: u64,
    pub recorded_ticks: usize,
    pub max_ticks: usize,
    pub truncated: bool,
    pub invalid_reason: Option<String>,
    pub frames: Vec<RecordedButtons>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_snapshot: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_snapshot: Option<Value>,
    pub final_state: Value,
    pub final_checksum: String,
}

impl SnapshotRecording {
    /// Accept raw data or the existing query/CLI response wrappers. Bounds are
    /// checked by counting serialized bytes without allocating another buffer.
    /// Call each frame's `decode` and the game's validators before installation.
    pub fn parse(
        mut value: Value,
        identity: RecordingIdentity<'_>,
        max_bytes: usize,
        allow_legacy: bool,
    ) -> Result<Self, String> {
        struct Counter {
            used: usize,
            limit: usize,
        }
        impl Write for Counter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.used = self
                    .used
                    .checked_add(bytes.len())
                    .filter(|n| *n <= self.limit)
                    .ok_or_else(|| io::Error::other("recording exceeds byte limit"))?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        serde_json::to_writer(
            Counter {
                used: 0,
                limit: max_bytes,
            },
            &value,
        )
        .map_err(|e| e.to_string())?;
        if value.get("response").is_some() {
            value = value
                .get_mut("response")
                .unwrap()
                .get_mut("value")
                .ok_or("query response lacks recording value")?
                .take();
        } else if value.get("value").is_some() {
            value = value.get_mut("value").unwrap().take();
        }
        let recording: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if !(recording.format_version == 2 || allow_legacy && recording.format_version == 1)
            || recording.game_seed != identity.game_seed
            || recording.action_schema != identity.action_schema
            || recording.fixed_step_nanos != identity.fixed_step_nanos
            || recording.max_ticks != identity.max_ticks
        {
            return Err("unsupported recording header".into());
        }
        if (recording.format_version == 2)
            != (recording.initial_snapshot.is_some() && recording.final_snapshot.is_some())
            || recording.format_version == 1
                && (recording.initial_snapshot.is_some() || recording.final_snapshot.is_some())
        {
            return Err("recording snapshot/version mismatch".into());
        }
        if recording.truncated || recording.invalid_reason.is_some() {
            return Err(
                "recording is truncated or invalidated; exact replay is unavailable".into(),
            );
        }
        if recording.frames.len() > identity.max_ticks
            || recording.frames.len() != recording.recorded_ticks
        {
            return Err("recording frame count exceeds bounds or differs from header".into());
        }
        Ok(recording)
    }
}

/// A bounded segment beginning at a complete game-owned snapshot.
pub struct SnapshotRecorder {
    pub initial_snapshot: Value,
    pub start_host_frame: u64,
    frames: Vec<RecordedButtons>,
    truncated: bool,
    invalid_reason: Option<String>,
    max_ticks: usize,
}
impl SnapshotRecorder {
    pub fn new(initial_snapshot: Value, start_host_frame: u64, max_ticks: usize) -> Self {
        Self {
            initial_snapshot,
            start_host_frame,
            frames: Vec::new(),
            truncated: false,
            invalid_reason: None,
            max_ticks,
        }
    }
    pub fn frames(&self) -> &[RecordedButtons] {
        &self.frames
    }
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
    pub fn invalid_reason(&self) -> Option<&str> {
        self.invalid_reason.as_deref()
    }
    /// Preserve the accepted prefix; an overflow invalidates exact replay.
    pub fn push(&mut self, frame: RecordedButtons) {
        if self.frames.len() < self.max_ticks {
            self.frames.push(frame);
        } else {
            self.truncated = true;
        }
    }
    pub fn invalidate(&mut self, reason: impl Into<String>) {
        if self.invalid_reason.is_none() {
            self.invalid_reason = Some(reason.into());
        }
    }
    pub fn export(
        &self,
        identity: RecordingIdentity<'_>,
        final_state: Value,
        final_snapshot: Option<Value>,
        final_checksum: String,
    ) -> Result<SnapshotRecording, String> {
        if identity.max_ticks != self.max_ticks {
            return Err("recording identity tick limit differs from recorder".into());
        }
        Ok(SnapshotRecording {
            format_version: 2,
            game_seed: identity.game_seed,
            action_schema: identity.action_schema.into(),
            fixed_step_nanos: identity.fixed_step_nanos,
            start_host_frame: self.start_host_frame,
            recorded_ticks: self.frames.len(),
            max_ticks: self.max_ticks,
            truncated: self.truncated,
            invalid_reason: self.invalid_reason.clone(),
            frames: self.frames.clone(),
            initial_snapshot: Some(self.initial_snapshot.clone()),
            final_snapshot,
            final_state,
            final_checksum,
        })
    }
}

/// A cursor over a game-validated recording. End-of-file never yields another
/// input frame. The host owns pause and mutation policy; games verify completion.
pub struct Playback {
    recording: SnapshotRecording,
    expected_snapshot: Value,
    position: usize,
    verified: Option<bool>,
    error: Option<String>,
}
impl Playback {
    pub fn new(recording: SnapshotRecording, expected_snapshot: Value) -> Self {
        Self {
            recording,
            expected_snapshot,
            position: 0,
            verified: None,
            error: None,
        }
    }
    pub fn recording(&self) -> &SnapshotRecording {
        &self.recording
    }
    pub fn expected_snapshot(&self) -> &Value {
        &self.expected_snapshot
    }
    pub fn position(&self) -> usize {
        self.position
    }
    pub fn remaining(&self) -> usize {
        self.recording.frames.len() - self.position
    }
    pub fn complete(&self) -> bool {
        self.remaining() == 0
    }
    pub fn verified(&self) -> Option<bool> {
        self.verified
    }
    pub fn next_frame(&mut self) -> Option<&RecordedButtons> {
        let frame = self.recording.frames.get(self.position)?;
        self.position += 1;
        Some(frame)
    }
    /// Record the game's full-state/pixel comparison, only after the final tick.
    pub fn finish(&mut self, result: Result<(), String>) -> Result<(), String> {
        if !self.complete() {
            return Err("cannot verify unfinished playback".into());
        }
        if self.verified.is_some() {
            return Err("playback already verified".into());
        }
        self.verified = Some(result.is_ok());
        self.error = result.err();
        Ok(())
    }
    pub fn status(&self) -> Value {
        serde_json::json!({"active":true,"position":self.position,"total":self.recording.frames.len(),"complete":self.complete(),"verified":self.verified,"error":self.error})
    }
    pub fn inactive_status() -> Value {
        serde_json::json!({"active":false,"position":0,"total":0,"complete":false,"verified":null,"error":null})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCHEMA: [(u8, &str); 2] = [(1, "jump"), (2, "interact")];
    fn identity() -> RecordingIdentity<'static> {
        RecordingIdentity {
            game_seed: 42,
            action_schema: "test-buttons-v1",
            fixed_step_nanos: 100,
            max_ticks: 2,
        }
    }
    fn empty() -> RecordedButtons {
        RecordedButtons {
            active: vec![],
            pressed: vec![],
            released: vec![],
        }
    }
    fn recording() -> SnapshotRecording {
        let mut recorder = SnapshotRecorder::new(serde_json::json!({"level":3}), 19, 2);
        recorder.push(empty());
        recorder.push(empty());
        recorder
            .export(
                identity(),
                Value::Null,
                Some(serde_json::json!({"level":4})),
                "pixels".into(),
            )
            .unwrap()
    }
    #[test]
    fn independent_consumed_edges_roundtrip_without_inventing_start_press() {
        for frame in [
            RecordedButtons {
                active: vec!["jump".into()],
                pressed: vec![],
                released: vec![],
            },
            RecordedButtons {
                active: vec!["jump".into()],
                pressed: vec!["jump".into()],
                released: vec![],
            },
            RecordedButtons {
                active: vec![],
                pressed: vec![],
                released: vec!["jump".into()],
            },
        ] {
            assert_eq!(
                RecordedButtons::capture(&frame.decode(&SCHEMA).unwrap(), &SCHEMA).unwrap(),
                frame
            );
        }
        let mut bad = empty();
        bad.pressed.push("jump".into());
        assert!(bad.decode(&SCHEMA).is_err());
        bad.active = vec!["jump".into(), "jump".into()];
        assert!(bad.decode(&SCHEMA).is_err());
        bad.active = vec!["missing".into()];
        assert!(bad.decode(&SCHEMA).is_err());
    }
    #[test]
    fn capture_rejects_analog_unmapped_actions_and_unmapped_releases() {
        let mut tracker = InputTracker::new();
        assert!(
            RecordedButtons::capture(&tracker.sample([(1, ActionValue::axis(12))]), &SCHEMA)
                .is_err()
        );
        assert!(
            RecordedButtons::capture(&tracker.sample([(3, ActionValue::PRESSED)]), &SCHEMA)
                .is_err()
        );
        assert!(RecordedButtons::capture(&tracker.sample([]), &SCHEMA).is_err());
        assert!(
            empty()
                .decode(&[(1, "duplicate"), (2, "duplicate")])
                .is_err()
        );
    }
    #[test]
    fn bounded_buffer_and_parser_preserve_origin_and_reject_invalid_artifacts() {
        let valid = recording();
        let value = serde_json::to_value(&valid).unwrap();
        assert_eq!(
            SnapshotRecording::parse(value.clone(), identity(), 4096, false)
                .unwrap()
                .start_host_frame,
            19
        );
        assert!(SnapshotRecording::parse(value.clone(), identity(), 2, false).is_err());
        for (key, val) in [
            ("format_version", 3.into()),
            ("game_seed", 43.into()),
            ("recorded_ticks", 3.into()),
            ("initial_snapshot", Value::Null),
            ("unknown", 1.into()),
        ] {
            let mut bad = value.clone();
            bad[key] = val;
            assert!(SnapshotRecording::parse(bad, identity(), 4096, false).is_err());
        }
        let mut recorder = SnapshotRecorder::new(Value::Null, 7, 1);
        recorder.push(empty());
        recorder.push(empty());
        assert_eq!(recorder.frames.len(), 1);
        assert!(recorder.truncated);
        recorder.invalidate("external mutation");
        recorder.invalidate("later");
        assert_eq!(
            recorder.invalid_reason.as_deref(),
            Some("external mutation")
        );
        assert!(
            recorder
                .export(identity(), Value::Null, None, "x".into())
                .is_err(),
            "identity limit must match recorder"
        );
        let matching = RecordingIdentity {
            max_ticks: 1,
            ..identity()
        };
        let invalid = recorder
            .export(matching, Value::Null, None, "x".into())
            .unwrap();
        assert!(
            SnapshotRecording::parse(
                serde_json::to_value(invalid).unwrap(),
                matching,
                4096,
                false
            )
            .is_err()
        );
    }
    #[test]
    fn playback_never_overshoots_and_verification_only_follows_completion() {
        let mut playback = Playback::new(recording(), serde_json::json!({"level":4}));
        assert!(playback.finish(Ok(())).is_err());
        assert_eq!(playback.position(), 0);
        assert!(playback.next_frame().is_some());
        assert_eq!(playback.remaining(), 1);
        assert!(playback.next_frame().is_some());
        assert!(playback.complete());
        assert!(playback.next_frame().is_none());
        assert_eq!(playback.position(), 2);
        playback.finish(Err("pixels differ".into())).unwrap();
        assert_eq!(playback.verified(), Some(false));
        assert_eq!(playback.status()["error"], "pixels differ");
        assert!(playback.finish(Ok(())).is_err());
    }
}
