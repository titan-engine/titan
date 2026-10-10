//! Embedded starter content, with known solutions for headless gameplay tests.
//!
//! Levels progress from a single push to turning, handling multiple blocks, and
//! routing around an interior wall. Each source is parsed through the same
//! validated API as externally supplied text; no filesystem access is needed.

use crate::level::Level;

/// Known solutions, in [`starter_levels`] order.
///
/// Each ASCII character is one movement action: `U` up, `D` down, `L` left,
/// `R` right. These are legal solving paths, not claims of optimality.
pub const SOLUTIONS: &[&str] = &["R", "RDRU", "URRDLLDRR", "RRDRUU"];

/// Returns fresh, validated initial states for the four embedded starter levels.
///
/// Declaration order is also progression order and matches [`SOLUTIONS`].
///
/// # Panics
///
/// Panics if a bundled source violates the level format. Such an error is a
/// content authoring bug, not a recoverable runtime loading failure.
pub fn starter_levels() -> Vec<Level> {
    [
        (
            "levels/01-first-push.puzzle",
            include_str!("../levels/01-first-push.puzzle"),
        ),
        (
            "levels/02-turn-the-corner.puzzle",
            include_str!("../levels/02-turn-the-corner.puzzle"),
        ),
        (
            "levels/03-two-deliveries.puzzle",
            include_str!("../levels/03-two-deliveries.puzzle"),
        ),
        (
            "levels/04-around-the-wall.puzzle",
            include_str!("../levels/04-around-the-wall.puzzle"),
        ),
    ]
    .into_iter()
    .map(|(source, text)| Level::parse(source, text).expect("bundled starter level must be valid"))
    .collect()
}
