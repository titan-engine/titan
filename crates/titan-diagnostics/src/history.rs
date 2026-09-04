use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use titan_protocol::{InputValue, Request, RequestEnvelope, ResponseEnvelope, ResponseOutcome};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub sequence: u64,
    pub request: RequestEnvelope,
    pub response: ResponseEnvelope,
    pub elapsed_us: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputEvent {
    pub sequence: u64,
    pub request_id: String,
    pub target_frame: u64,
    pub actions: BTreeMap<String, InputValue>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HistorySnapshot {
    pub requests: Vec<HistoryEntry>,
    pub accepted_inputs: Vec<InputEvent>,
    pub dropped_entries: u64,
}
/// Oldest-first ring bounded by entry count and serialized entry bytes.
/// Failed input injection remains in requests, but is excluded from accepted_inputs.
pub struct RequestHistory {
    entries: VecDeque<(HistoryEntry, usize)>,
    capacity: usize,
    max_bytes: usize,
    bytes: usize,
    next_sequence: u64,
    dropped: u64,
}
impl Default for RequestHistory {
    fn default() -> Self {
        Self::new(64, 1024 * 1024)
    }
}
impl RequestHistory {
    pub fn new(capacity: usize, max_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity,
            max_bytes,
            bytes: 0,
            next_sequence: 0,
            dropped: 0,
        }
    }
    pub fn record(
        &mut self,
        request: &RequestEnvelope,
        response: &ResponseEnvelope,
        elapsed_us: u64,
    ) -> Result<bool, serde_json::Error> {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let entry = HistoryEntry {
            sequence,
            request: request.clone(),
            response: response.clone(),
            elapsed_us,
        };
        let size = serde_json::to_vec(&entry)?.len();
        if self.capacity == 0 || size > self.max_bytes {
            self.dropped = self.dropped.saturating_add(1);
            return Ok(false);
        }
        while self.entries.len() >= self.capacity || self.bytes > self.max_bytes - size {
            let (_, old_size) = self
                .entries
                .pop_front()
                .expect("nonempty oversized history");
            self.bytes -= old_size;
            self.dropped = self.dropped.saturating_add(1);
        }
        self.bytes += size;
        self.entries.push_back((entry, size));
        Ok(true)
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn serialized_bytes(&self) -> usize {
        self.bytes
    }
    pub fn snapshot(&self) -> HistorySnapshot {
        let requests: Vec<_> = self
            .entries
            .iter()
            .map(|(entry, _)| entry.clone())
            .collect();
        let accepted_inputs = requests
            .iter()
            .filter_map(
                |entry| match (&entry.request.request, &entry.response.outcome) {
                    (Request::InjectInput { frame, actions }, ResponseOutcome::Success { .. }) => {
                        Some(InputEvent {
                            sequence: entry.sequence,
                            request_id: entry.request.request_id.clone(),
                            target_frame: *frame,
                            actions: actions.clone(),
                        })
                    }
                    _ => None,
                },
            )
            .collect();
        HistorySnapshot {
            requests,
            accepted_inputs,
            dropped_entries: self.dropped,
        }
    }
}
