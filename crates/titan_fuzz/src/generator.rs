use serde::Serialize;
use titan_test::{InputAction, InputButton, InputScript, ScriptEvent};

/// The intentionally small input distribution: independent chances to start a
/// hold for each idle button on each tick, with randomly sampled hold lengths.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Generator {
    /// Probability in `[0, 1]` of pressing each idle button on a tick.
    pub event_density: f64,
    /// Minimum number of ticks a press remains held; must be at least one.
    pub min_hold_ticks: u64,
    /// Maximum hold length (inclusive); must be at least the minimum.
    pub max_hold_ticks: u64,
}

impl Default for Generator {
    fn default() -> Self {
        Self {
            event_density: 0.1,
            min_hold_ticks: 1,
            max_hold_ticks: 30,
        }
    }
}

impl Generator {
    pub(crate) fn validate(&self) {
        assert!(
            self.event_density.is_finite() && (0.0..=1.0).contains(&self.event_density),
            "event_density must be finite and in [0, 1]"
        );
        assert!(self.min_hold_ticks > 0, "min_hold_ticks must be positive");
        assert!(
            self.max_hold_ticks >= self.min_hold_ticks,
            "max_hold_ticks must be at least min_hold_ticks"
        );
    }

    /// Regenerate a case directly from its seed, index, alphabet and tick limit.
    ///
    /// Uses a fixed `SplitMix64` algorithm, integer sampling and 53-bit density
    /// draws, not platform-dependent `usize` draws or an external RNG's defaults.
    /// Buttons are deduplicated in first-occurrence order. Holds never overlap
    /// for one button; different buttons can press simultaneously. Every press
    /// has a release strictly inside the run, so no input is generated for a
    /// zero/one-tick run. Releases precede new presses on the same tick.
    ///
    /// # Panics
    /// Panics for an invalid generator configuration.
    pub fn generate(
        &self,
        buttons: &[InputButton],
        ticks: u64,
        seed: u64,
        case_index: u64,
    ) -> InputScript {
        self.validate();
        let buttons = unique_buttons(buttons.iter().copied());
        let mut script = InputScript::default();
        if ticks < 2 || buttons.is_empty() || self.event_density == 0.0 {
            return script;
        }
        let mut seeder =
            SplitMix64(seed.wrapping_add(case_index.wrapping_mul(0x9e37_79b9_7f4a_7c15)));
        let mut rng = SplitMix64(seeder.next());
        let mut releases = vec![None; buttons.len()];
        for tick in 0..ticks {
            for (index, &button) in buttons.iter().enumerate() {
                if releases[index] == Some(tick) {
                    script.events.push(ScriptEvent {
                        tick,
                        action: InputAction::Release(button),
                    });
                    releases[index] = None;
                }
            }
            for (index, &button) in buttons.iter().enumerate() {
                let remaining = ticks - 1 - tick;
                if releases[index].is_some() || remaining < self.min_hold_ticks {
                    continue;
                }
                let draw = (rng.next() >> 11) as f64 / ((1_u64 << 53) as f64);
                if draw < self.event_density {
                    let max = self.max_hold_ticks.min(remaining);
                    let hold = self.min_hold_ticks + rng.below(max - self.min_hold_ticks + 1);
                    releases[index] = Some(tick + hold);
                    script.events.push(ScriptEvent {
                        tick,
                        action: InputAction::Press(button),
                    });
                }
            }
        }
        script
    }
}

pub(crate) fn unique_buttons(buttons: impl IntoIterator<Item = InputButton>) -> Vec<InputButton> {
    let mut unique = Vec::new();
    for button in buttons {
        if !unique.contains(&button) {
            unique.push(button);
        }
    }
    unique
}

// SplitMix64 (Steele, Lea and Flood): fixed-width wrapping arithmetic makes
// this portable and freezes the corpus independently of dependency upgrades.
// This is a simulation RNG, not suitable for secrets or cryptography.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        // Multiply-high sampling needs exactly one draw, so even sampling is
        // bounded. Its negligible rounding bias is acceptable for input fuzzing.
        ((u128::from(self.next()) * u128::from(bound)) >> 64) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::SplitMix64;

    #[test]
    fn portable_rng_known_vector() {
        let mut rng = SplitMix64(0);
        assert_eq!(rng.next(), 0xe220_a839_7b1d_cdaf);
        assert_eq!(rng.next(), 0x6e78_9e6a_a1b9_65f4);
        assert_eq!(rng.next(), 0x06c4_5d18_8009_454f);
    }
}
