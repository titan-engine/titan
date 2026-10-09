use titan_test::{InputAction, InputScript};

use crate::{Failure, Violation};

/// Returns true if a candidate failed once but disagreed on confirmation.
pub(crate) fn shrink(
    failure: &mut Failure,
    execute: impl FnMut(&InputScript, u64) -> Option<Violation>,
) -> bool {
    let mut shrinker = Shrinker {
        failure,
        execute,
        flaky: false,
    };
    let mut truncated = shrinker.failure.script.clone();
    let end = shrinker
        .failure
        .ticks
        .min(shrinker.failure.failing_tick.saturating_add(1));
    truncated.events.retain(|event| event.tick < end);
    shrinker.consider(truncated, end);

    // Delta debugging: start with halves, descend to individual events, and
    // repeat the single-event pass until nothing more can be removed.
    let mut chunk = shrinker.failure.script.events.len().div_ceil(2).max(1);
    loop {
        let mut index = 0;
        let mut removed = false;
        while index < shrinker.failure.script.events.len() && shrinker.available() {
            let mut candidate = shrinker.failure.script.clone();
            let end = (index + chunk).min(candidate.events.len());
            candidate.events.drain(index..end);
            if shrinker.consider(candidate, shrinker.failure.ticks) {
                removed = true;
            } else {
                index += chunk;
            }
        }
        if !shrinker.available() || (chunk == 1 && !removed) {
            break;
        }
        chunk = chunk.div_ceil(2);
    }

    // Shorten holds by bringing releases towards their preceding press.
    // Removed events may leave unmatched releases/presses: these are legal
    // titan_test input, and only a replay can decide whether they are needed.
    let mut index = 0;
    while index < shrinker.failure.script.events.len() && shrinker.available() {
        if let Some(duration) = hold_duration(&shrinker.failure.script, index) {
            let mut step = (duration / 2).max(1);
            while let Some(duration) = hold_duration(&shrinker.failure.script, index) {
                if duration <= 1 || !shrinker.available() {
                    break;
                }
                let mut candidate = shrinker.failure.script.clone();
                candidate.events[index].tick -= step.min(duration - 1);
                candidate.events.sort_by_key(|event| event.tick);
                if !shrinker.consider(candidate, shrinker.failure.ticks) {
                    if step == 1 {
                        break;
                    }
                    step = step.div_ceil(2);
                }
            }
        }
        index += 1;
    }

    // Move individual events earlier with progressively smaller offsets.
    // Stable sorting preserves file order when events land on the same tick.
    let mut index = 0;
    while index < shrinker.failure.script.events.len() && shrinker.available() {
        let mut step = (shrinker.failure.script.events[index].tick / 2).max(1);
        loop {
            if !shrinker.available() || index >= shrinker.failure.script.events.len() {
                break;
            }
            let tick = shrinker.failure.script.events[index].tick;
            if tick == 0 {
                break;
            }
            let mut candidate = shrinker.failure.script.clone();
            candidate.events[index].tick = tick.saturating_sub(step);
            candidate.events.sort_by_key(|event| event.tick);
            if !shrinker.consider(candidate, shrinker.failure.ticks) {
                if step == 1 {
                    break;
                }
                step = step.div_ceil(2);
            }
        }
        index += 1;
    }
    shrinker.flaky
}

fn hold_duration(script: &InputScript, index: usize) -> Option<u64> {
    let event = script.events.get(index)?;
    let InputAction::Release(button) = event.action else {
        return None;
    };
    let press = script.events[..index]
        .iter()
        .rev()
        .find(|event| match event.action {
            InputAction::Press(other) | InputAction::Release(other) | InputAction::Tap(other) => {
                other == button
            }
        })?;
    if press.action == InputAction::Press(button) {
        Some(event.tick.saturating_sub(press.tick))
    } else {
        None
    }
}

struct Shrinker<'a, E> {
    failure: &'a mut Failure,
    execute: E,
    flaky: bool,
}

impl<E: FnMut(&InputScript, u64) -> Option<Violation>> Shrinker<'_, E> {
    fn available(&self) -> bool {
        !self.flaky && self.failure.shrink_runs < self.failure.config.max_shrink_runs
    }

    fn consider(&mut self, mut candidate: InputScript, ticks: u64) -> bool {
        if !self.available() || (candidate == self.failure.script && ticks == self.failure.ticks) {
            return false;
        }
        self.failure.shrink_runs += 1;
        let Some(violation) = (self.execute)(&candidate, ticks) else {
            return false;
        };
        // An opaque payload cannot prove identity with a string panic, even
        // when that string happens to equal our opaque diagnostic text.
        if violation.name != self.failure.invariant || violation.opaque_panic.is_some() {
            return false;
        }
        // Do not trade one game panic for a different panic merely because both
        // use the implicit no_panics invariant.
        if violation.name == "no_panics" && violation.message != self.failure.message {
            return false;
        }
        if !self.available() {
            return false;
        }
        let ticks = ticks.min(violation.tick.saturating_add(1));
        candidate.events.retain(|event| event.tick < ticks);
        self.failure.shrink_runs += 1;
        if (self.execute)(&candidate, ticks).as_ref() != Some(&violation) {
            self.flaky = true;
            return false;
        }
        self.failure.script = candidate;
        self.failure.ticks = ticks;
        self.failure.failing_tick = violation.tick;
        self.failure.message = violation.message;
        true
    }
}
