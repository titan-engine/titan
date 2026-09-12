use core::{
    error::Error,
    fmt,
    str::{FromStr, SplitWhitespace},
};
use std::collections::HashSet;

const FORMAT_VERSION: u32 = 1;

const HEADER_RECORD: &str = "platformer-level";

/// A level spawn position.
///
/// Values in a [`Level`] are finite because the parser validates them before
/// constructing the level. This record is returned by value from [`Level::spawn`]
/// so callers cannot mutate a level through it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnPoint {
    /// Horizontal position. Positive values point right.
    pub x: f32,
    /// Vertical position. Positive values point up.
    pub y: f32,
}

/// A platform rectangle in a [`Level`].
///
/// The position is the rectangle's lower-left corner. Platform IDs are
/// nonzero and unique within a level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Platform {
    /// Stable platform identifier.
    pub id: u64,
    /// Horizontal position of the lower-left corner.
    pub x: f32,
    /// Vertical position of the lower-left corner.
    pub y: f32,
    /// Rectangle width.
    pub width: f32,
    /// Rectangle height.
    pub height: f32,
}

/// A completely validated platformer level.
///
/// Levels can only be created by parsing a valid document. The platform slice
/// is read-only and sorted by ID, so callers cannot invalidate the level's
/// uniqueness, geometry, or ordering invariants through this API.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    next_id: u64,
    spawn: SpawnPoint,
    platforms: Vec<Platform>,
}

impl Level {
    /// Returns the next platform ID reserved by this level.
    pub const fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Returns the spawn position.
    pub const fn spawn(&self) -> SpawnPoint {
        self.spawn
    }

    /// Returns all platforms sorted by ascending ID.
    pub fn platforms(&self) -> &[Platform] {
        &self.platforms
    }
}

/// A parsing error with owned line and record context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    line: usize,
    record: String,
    message: String,
}

impl ParseError {
    fn new(line: usize, record: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            line,
            record: record.into(),
            message: message.into(),
        }
    }

    /// Returns the one-based source line associated with this error.
    pub const fn line(&self) -> usize {
        self.line
    }

    /// Returns the record name associated with this error.
    pub fn record(&self) -> &str {
        &self.record
    }

    /// Returns the specific error description.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "line {}: {} record: {}",
            self.line, self.record, self.message
        )
    }
}

impl Error for ParseError {}

impl FromStr for Level {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut header_seen = false;
        let mut last_line = 0;
        let mut next_id = None;
        let mut spawn = None;
        let mut platforms = Vec::new();
        let mut platform_ids = HashSet::new();

        for (line_index, raw_line) in input.lines().enumerate() {
            let line = line_index + 1;
            last_line = line;
            let trimmed = raw_line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let mut fields = trimmed.split_whitespace();
            let record = fields.next().expect("trimmed lines contain a record");

            if !header_seen {
                header_seen = true;
                if record != HEADER_RECORD {
                    return Err(ParseError::new(
                        line,
                        record,
                        "the first nonblank line must be the platformer-level header",
                    ));
                }

                let version = parse_field::<u32>(&mut fields, line, record, "version")?;
                ensure_no_extra_fields(&mut fields, line, record)?;
                if version != FORMAT_VERSION {
                    return Err(ParseError::new(
                        line,
                        record,
                        format!("unsupported version {version}; expected {FORMAT_VERSION}"),
                    ));
                }
                continue;
            }

            match record {
                HEADER_RECORD => {
                    return Err(ParseError::new(
                        line,
                        record,
                        "the header may appear only once and must be first",
                    ));
                }
                "next-id" => {
                    if next_id.is_some() {
                        return Err(ParseError::new(line, record, "duplicate next-id record"));
                    }
                    let value = parse_field::<u64>(&mut fields, line, record, "counter")?;
                    ensure_no_extra_fields(&mut fields, line, record)?;
                    if value == 0 {
                        return Err(ParseError::new(line, record, "counter must be nonzero"));
                    }
                    next_id = Some((value, line));
                }
                "spawn" => {
                    if spawn.is_some() {
                        return Err(ParseError::new(line, record, "duplicate spawn record"));
                    }
                    let x = parse_finite_f32(&mut fields, line, record, "x")?;
                    let y = parse_finite_f32(&mut fields, line, record, "y")?;
                    ensure_no_extra_fields(&mut fields, line, record)?;
                    spawn = Some(SpawnPoint { x, y });
                }
                "platform" => {
                    let id = parse_field::<u64>(&mut fields, line, record, "ID")?;
                    let x = parse_finite_f32(&mut fields, line, record, "x")?;
                    let y = parse_finite_f32(&mut fields, line, record, "y")?;
                    let width = parse_finite_f32(&mut fields, line, record, "width")?;
                    let height = parse_finite_f32(&mut fields, line, record, "height")?;
                    ensure_no_extra_fields(&mut fields, line, record)?;

                    if id == 0 {
                        return Err(ParseError::new(line, record, "platform ID must be nonzero"));
                    }
                    if !platform_ids.insert(id) {
                        return Err(ParseError::new(
                            line,
                            record,
                            format!("duplicate platform ID {id}"),
                        ));
                    }
                    if width <= 0.0 {
                        return Err(ParseError::new(
                            line,
                            record,
                            "width must be strictly positive",
                        ));
                    }
                    if height <= 0.0 {
                        return Err(ParseError::new(
                            line,
                            record,
                            "height must be strictly positive",
                        ));
                    }
                    if !(x + width).is_finite() {
                        return Err(ParseError::new(line, record, "x plus width must be finite"));
                    }
                    if !(y + height).is_finite() {
                        return Err(ParseError::new(
                            line,
                            record,
                            "y plus height must be finite",
                        ));
                    }

                    platforms.push(Platform {
                        id,
                        x,
                        y,
                        width,
                        height,
                    });
                }
                _ => {
                    return Err(ParseError::new(line, record, "unknown record"));
                }
            }
        }

        if !header_seen {
            return Err(ParseError::new(
                1,
                "header",
                "the document must contain a platformer-level header",
            ));
        }

        let spawn = spawn.ok_or_else(|| {
            ParseError::new(
                end_of_document_line(last_line),
                "spawn",
                "missing required spawn record",
            )
        })?;
        let (next_id, next_id_line) = next_id.ok_or_else(|| {
            ParseError::new(
                end_of_document_line(last_line),
                "next-id",
                "missing required next-id record",
            )
        })?;

        platforms.sort_unstable_by_key(|platform| platform.id);
        if let Some(maximum_id) = platforms.last().map(|platform| platform.id)
            && next_id <= maximum_id
        {
            return Err(ParseError::new(
                next_id_line,
                "next-id",
                format!(
                    "counter {next_id} must be greater than every platform ID (maximum {maximum_id})"
                ),
            ));
        }

        Ok(Self {
            next_id,
            spawn,
            platforms,
        })
    }
}

impl fmt::Display for Level {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "{HEADER_RECORD} {FORMAT_VERSION}")?;
        writeln!(formatter, "next-id {}", self.next_id)?;
        writeln!(formatter, "spawn {} {}", self.spawn.x, self.spawn.y)?;
        for platform in &self.platforms {
            writeln!(
                formatter,
                "platform {} {} {} {} {}",
                platform.id, platform.x, platform.y, platform.width, platform.height
            )?;
        }
        Ok(())
    }
}

fn parse_field<T>(
    fields: &mut SplitWhitespace<'_>,
    line: usize,
    record: &str,
    field: &str,
) -> Result<T, ParseError>
where
    T: FromStr,
{
    let value = fields
        .next()
        .ok_or_else(|| ParseError::new(line, record, format!("missing {field} field")))?;
    value
        .parse::<T>()
        .map_err(|_| ParseError::new(line, record, format!("invalid {field} value {value:?}")))
}

fn parse_finite_f32(
    fields: &mut SplitWhitespace<'_>,
    line: usize,
    record: &str,
    field: &str,
) -> Result<f32, ParseError> {
    let value = parse_field::<f32>(fields, line, record, field)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ParseError::new(
            line,
            record,
            format!("{field} must be finite"),
        ))
    }
}

fn ensure_no_extra_fields(
    fields: &mut SplitWhitespace<'_>,
    line: usize,
    record: &str,
) -> Result<(), ParseError> {
    if let Some(extra) = fields.next() {
        Err(ParseError::new(
            line,
            record,
            format!("unexpected extra field {extra:?}"),
        ))
    } else {
        Ok(())
    }
}

fn end_of_document_line(last_line: usize) -> usize {
    last_line.saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::Level;

    fn assert_rejected(document: &str) {
        assert!(
            document.parse::<Level>().is_err(),
            "document was accepted:\n{document}"
        );
    }

    #[test]
    fn accepts_reordered_records_and_crlf() {
        let document = "\nplatformer-level 1\r\nplatform 2 448 64 128 24\r\nspawn 64 64\r\nnext-id 4\r\nplatform 1 0 0 400 32\r\nplatform 3 640 128 160 24\r\n";
        let level = document.parse::<Level>().expect("valid level");

        assert_eq!(level.next_id(), 4);
        assert_eq!(level.spawn(), super::SpawnPoint { x: 64.0, y: 64.0 });
        assert_eq!(
            level
                .platforms()
                .iter()
                .map(|platform| platform.id)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn rejects_invalid_metadata_numbers_geometry_and_ids() {
        let invalid_documents = [
            "platformer-level 0\nnext-id 1\nspawn 0 0\n",
            "platformer-level 1\nnext-id 1\nspawn 0 0\nunknown 1\n",
            "platformer-level 1 extra\nnext-id 1\nspawn 0 0\n",
            "platformer-level 1\nnext-id\nspawn 0 0\n",
            "platformer-level 1\nnext-id 1\nspawn nope 0\n",
            "platformer-level 1\nnext-id 1\nspawn NaN 0\n",
            "platformer-level 1\nnext-id 1\nspawn 0 inf\n",
            "platformer-level 1\nnext-id 2\nspawn 0 0\nplatform 1 0 0 0 1\n",
            "platformer-level 1\nnext-id 2\nspawn 0 0\nplatform 1 0 0 1 -1\n",
            "platformer-level 1\nnext-id 2\nspawn 0 0\nplatform 1 3e38 0 3e38 1\n",
            "platformer-level 1\nnext-id 2\nspawn 0 0\nplatform 1 0 3e38 1 3e38\n",
            "platformer-level 1\nnext-id 2\nspawn 0 0\nplatform 1 0 0 inf 1\n",
            "platformer-level 1\nnext-id 2\nspawn 0 0\nplatform 0 0 0 1 1\n",
            "platformer-level 1\nnext-id 3\nspawn 0 0\nplatform 2 0 0 1 1\nplatform 2 1 1 1 1\n",
            "platformer-level 1\nnext-id 2\nspawn 0 0\nplatform 2 0 0 1 1\n",
            "platformer-level 1\nnext-id 0\nspawn 0 0\n",
            "platformer-level 1\nnext-id 1\nnext-id 2\nspawn 0 0\n",
            "platformer-level 1\nnext-id 1\nspawn 0 0\nspawn 1 1\n",
            "platformer-level 1\nnext-id 2\nspawn 0 0\nplatform 1 0 0 1 1 extra\n",
        ];

        for document in invalid_documents {
            assert_rejected(document);
        }
    }

    #[test]
    fn reports_deferred_counter_errors_at_their_source_line() {
        let document = "platformer-level 1\nnext-id 2\nspawn 0 0\nplatform 2 0 0 1 1\n";
        let error = document.parse::<Level>().expect_err("invalid counter");

        assert_eq!(error.line(), 2);
        assert_eq!(error.record(), "next-id");
    }

    #[test]
    fn writes_sorted_levels_and_preserves_float_meaning_on_roundtrip() {
        let document = "platformer-level 1\nnext-id 10\nspawn -0 1.5\nplatform 9 -0 2.5 0.25 3\nplatform 2 448 64 128 24\n";
        let level = document.parse::<Level>().expect("valid level");
        let written = level.to_string();
        let expected = "platformer-level 1\nnext-id 10\nspawn -0 1.5\nplatform 2 448 64 128 24\nplatform 9 -0 2.5 0.25 3\n";
        assert_eq!(written, expected);

        let round_tripped = written.parse::<Level>().expect("written level");
        assert_eq!(round_tripped.next_id(), level.next_id());
        assert_eq!(round_tripped.spawn().x.to_bits(), level.spawn().x.to_bits());
        assert_eq!(round_tripped.spawn().y.to_bits(), level.spawn().y.to_bits());
        assert_eq!(round_tripped.platforms().len(), level.platforms().len());
        for (actual, expected) in round_tripped.platforms().iter().zip(level.platforms()) {
            assert_eq!(actual.id, expected.id);
            assert_eq!(actual.x.to_bits(), expected.x.to_bits());
            assert_eq!(actual.y.to_bits(), expected.y.to_bits());
            assert_eq!(actual.width.to_bits(), expected.width.to_bits());
            assert_eq!(actual.height.to_bits(), expected.height.to_bits());
        }
    }

    #[test]
    fn requires_header_and_required_records() {
        assert_rejected("spawn 0 0\nplatformer-level 1\nnext-id 1\n");
        assert_rejected("platformer-level 1\nnext-id 1\n");
        assert_rejected("platformer-level 1\nspawn 0 0\n");
        assert_rejected("\n\r\n");
    }
}
