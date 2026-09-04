use std::time::Duration;

/// Deterministic time associated with the fixed-update schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedTime {
    delta: Duration,
    tick: u64,
}

impl FixedTime {
    /// Creates a fixed clock with the given update frequency.
    ///
    /// # Panics
    ///
    /// Panics when `updates_per_second` is zero.
    pub fn from_hz(updates_per_second: u32) -> Self {
        assert!(
            updates_per_second > 0,
            "fixed update frequency must be non-zero"
        );
        Self::from_duration(Duration::from_secs_f64(1.0 / f64::from(updates_per_second)))
    }

    /// Creates a fixed clock with an explicit step duration.
    ///
    /// # Panics
    ///
    /// Panics when `delta` is zero.
    pub fn from_duration(delta: Duration) -> Self {
        assert!(!delta.is_zero(), "fixed update duration must be non-zero");
        Self { delta, tick: 0 }
    }

    /// Returns the duration represented by one fixed update.
    pub const fn delta(self) -> Duration {
        self.delta
    }

    /// Returns the number of completed fixed updates.
    pub const fn tick(self) -> u64 {
        self.tick
    }

    pub(crate) fn complete_tick(&mut self) {
        self.tick = self
            .tick
            .checked_add(1)
            .expect("fixed tick counter overflowed");
    }
}

impl Default for FixedTime {
    fn default() -> Self {
        Self::from_hz(60)
    }
}
