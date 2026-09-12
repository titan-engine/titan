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
/// Values stored in a [`Level`] are always finite. The constructor and editing
/// operations validate spawn coordinates before storing them. This record is
/// returned by value from [`Level::spawn`] so callers cannot mutate a level
/// through it.
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

/// A partial replacement for a platform's geometry.
///
/// A `None` field keeps the corresponding value already stored on the platform.
/// The complete resulting rectangle is validated before an update is applied.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlatformGeometryUpdate {
    /// Replacement horizontal position of the lower-left corner.
    pub x: Option<f32>,
    /// Replacement vertical position of the lower-left corner.
    pub y: Option<f32>,
    /// Replacement rectangle width.
    pub width: Option<f32>,
    /// Replacement rectangle height.
    pub height: Option<f32>,
}

/// An error from an in-memory level editing operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditError {
    /// The spawn position contains a non-finite coordinate.
    InvalidSpawn,
    /// The platform has non-finite coordinates, non-positive dimensions, or a
    /// non-finite right or top bound.
    InvalidPlatformGeometry,
    /// No platform with the requested ID exists.
    PlatformNotFound {
        /// The ID that was not found.
        id: u64,
    },
    /// The next platform ID is `u64::MAX`, so assigning it would wrap the
    /// counter and cannot be performed.
    PlatformIdExhausted,
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpawn => formatter.write_str("spawn coordinates must be finite"),
            Self::InvalidPlatformGeometry => formatter.write_str(
                "platform geometry must have finite coordinates, positive dimensions, and finite bounds",
            ),
            Self::PlatformNotFound { id } => write!(formatter, "platform ID {id} was not found"),
            Self::PlatformIdExhausted => {
                formatter.write_str("the platform ID counter is exhausted")
            }
        }
    }
}

impl Error for EditError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GeometryError {
    NonFiniteX,
    NonFiniteY,
    NonFiniteWidth,
    NonFiniteHeight,
    NonPositiveWidth,
    NonPositiveHeight,
    NonFiniteRightBound,
    NonFiniteTopBound,
}

impl GeometryError {
    const fn parse_message(self) -> &'static str {
        match self {
            Self::NonFiniteX => "x must be finite",
            Self::NonFiniteY => "y must be finite",
            Self::NonFiniteWidth => "width must be finite",
            Self::NonFiniteHeight => "height must be finite",
            Self::NonPositiveWidth => "width must be strictly positive",
            Self::NonPositiveHeight => "height must be strictly positive",
            Self::NonFiniteRightBound => "x plus width must be finite",
            Self::NonFiniteTopBound => "y plus height must be finite",
        }
    }
}

fn validate_spawn(spawn: SpawnPoint) -> Result<(), EditError> {
    if spawn.x.is_finite() && spawn.y.is_finite() {
        Ok(())
    } else {
        Err(EditError::InvalidSpawn)
    }
}

fn validate_platform_geometry(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Result<(), GeometryError> {
    if !x.is_finite() {
        return Err(GeometryError::NonFiniteX);
    }
    if !y.is_finite() {
        return Err(GeometryError::NonFiniteY);
    }
    if !width.is_finite() {
        return Err(GeometryError::NonFiniteWidth);
    }
    if !height.is_finite() {
        return Err(GeometryError::NonFiniteHeight);
    }
    if width <= 0.0 {
        return Err(GeometryError::NonPositiveWidth);
    }
    if height <= 0.0 {
        return Err(GeometryError::NonPositiveHeight);
    }
    if !(x + width).is_finite() {
        return Err(GeometryError::NonFiniteRightBound);
    }
    if !(y + height).is_finite() {
        return Err(GeometryError::NonFiniteTopBound);
    }
    Ok(())
}

/// A completely validated platformer level.
///
/// Levels can be created by parsing a valid document or with [`Level::new`].
/// The platform slice is read-only and sorted by ID, so callers cannot
/// invalidate the level's uniqueness, geometry, or ordering invariants through
/// this API.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    next_id: u64,
    spawn: SpawnPoint,
    platforms: Vec<Platform>,
}

impl Level {
    /// Creates an empty level with the supplied spawn position.
    ///
    /// The first platform receives ID `1`. A non-finite spawn coordinate is
    /// rejected without constructing a level.
    pub fn new(spawn: SpawnPoint) -> Result<Self, EditError> {
        validate_spawn(spawn)?;
        Ok(Self {
            next_id: 1,
            spawn,
            platforms: Vec::new(),
        })
    }

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

    /// Adds a platform and returns its newly allocated ID.
    ///
    /// The current next-ID counter is assigned, then advanced. IDs are never
    /// reused after deletion. Geometry is validated before the level changes;
    /// the operation fails with [`EditError::PlatformIdExhausted`] when the
    /// counter is `u64::MAX` rather than wrapping it.
    pub fn add_platform(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Result<u64, EditError> {
        validate_platform_geometry(x, y, width, height)
            .map_err(|_| EditError::InvalidPlatformGeometry)?;
        let id = self.next_id;
        let next_id = id.checked_add(1).ok_or(EditError::PlatformIdExhausted)?;
        self.platforms.push(Platform {
            id,
            x,
            y,
            width,
            height,
        });
        self.next_id = next_id;
        Ok(id)
    }

    /// Removes the platform with `id`.
    ///
    /// The level is unchanged when the ID is unknown.
    pub fn remove_platform(&mut self, id: u64) -> Result<(), EditError> {
        let index = self
            .platforms
            .binary_search_by_key(&id, |platform| platform.id)
            .map_err(|_| EditError::PlatformNotFound { id })?;
        self.platforms.remove(index);
        Ok(())
    }

    /// Applies a partial geometry update to the platform with `id`.
    ///
    /// Fields set to `None` preserve their existing values. The complete
    /// resulting rectangle is validated before mutation, so an invalid update
    /// leaves the level unchanged.
    pub fn update_platform_geometry(
        &mut self,
        id: u64,
        update: PlatformGeometryUpdate,
    ) -> Result<(), EditError> {
        let index = self
            .platforms
            .binary_search_by_key(&id, |platform| platform.id)
            .map_err(|_| EditError::PlatformNotFound { id })?;
        let current = self.platforms[index];
        let x = update.x.unwrap_or(current.x);
        let y = update.y.unwrap_or(current.y);
        let width = update.width.unwrap_or(current.width);
        let height = update.height.unwrap_or(current.height);
        validate_platform_geometry(x, y, width, height)
            .map_err(|_| EditError::InvalidPlatformGeometry)?;
        self.platforms[index] = Platform {
            id: current.id,
            x,
            y,
            width,
            height,
        };
        Ok(())
    }

    /// Replaces the level's spawn position.
    ///
    /// A non-finite coordinate is rejected without changing the existing
    /// position.
    pub fn set_spawn(&mut self, spawn: SpawnPoint) -> Result<(), EditError> {
        validate_spawn(spawn)?;
        self.spawn = spawn;
        Ok(())
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
                    if let Err(error) = validate_platform_geometry(x, y, width, height) {
                        return Err(ParseError::new(line, record, error.parse_message()));
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
    use super::{EditError, Level, Platform, PlatformGeometryUpdate, SpawnPoint};

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

    #[test]
    fn editing_validates_changes_before_mutation_and_preserves_unspecified_fields() {
        assert_eq!(
            Level::new(SpawnPoint {
                x: f32::NAN,
                y: 0.0,
            }),
            Err(EditError::InvalidSpawn)
        );

        let mut level = Level::new(SpawnPoint { x: 0.0, y: 0.0 }).expect("valid spawn");
        let id = level
            .add_platform(10.0, 20.0, 30.0, 40.0)
            .expect("valid platform");
        let snapshot = level.clone();

        assert_eq!(
            level.add_platform(0.0, 0.0, 0.0, 1.0),
            Err(EditError::InvalidPlatformGeometry)
        );
        assert_eq!(level, snapshot);

        assert_eq!(
            level.update_platform_geometry(
                id,
                PlatformGeometryUpdate {
                    x: Some(f32::MAX),
                    width: Some(f32::MAX),
                    ..Default::default()
                },
            ),
            Err(EditError::InvalidPlatformGeometry)
        );
        assert_eq!(level, snapshot);

        assert_eq!(
            level.update_platform_geometry(99, PlatformGeometryUpdate::default()),
            Err(EditError::PlatformNotFound { id: 99 })
        );
        assert_eq!(level, snapshot);

        level
            .update_platform_geometry(
                id,
                PlatformGeometryUpdate {
                    x: Some(11.0),
                    height: Some(41.0),
                    ..Default::default()
                },
            )
            .expect("valid partial update");
        assert_eq!(
            level.platforms(),
            &[Platform {
                id,
                x: 11.0,
                y: 20.0,
                width: 30.0,
                height: 41.0,
            }]
        );

        let snapshot = level.clone();
        assert_eq!(
            level.set_spawn(SpawnPoint {
                x: 0.0,
                y: f32::INFINITY,
            }),
            Err(EditError::InvalidSpawn)
        );
        assert_eq!(level, snapshot);

        level
            .set_spawn(SpawnPoint { x: 5.0, y: 6.0 })
            .expect("valid spawn update");
        assert_eq!(level.spawn(), SpawnPoint { x: 5.0, y: 6.0 });
    }

    #[test]
    fn editing_never_reuses_ids_or_wraps_the_counter() {
        let mut level = Level::new(SpawnPoint { x: 0.0, y: 0.0 }).expect("valid level");
        let first_id = level
            .add_platform(0.0, 0.0, 1.0, 1.0)
            .expect("first platform");
        level.remove_platform(first_id).expect("existing platform");
        let second_id = level
            .add_platform(1.0, 1.0, 1.0, 1.0)
            .expect("second platform");
        assert_eq!((first_id, second_id), (1, 2));

        let document = format!(
            "platformer-level 1\nnext-id {}\nspawn 0 0\nplatform 1 0 0 1 1\n",
            u64::MAX - 1
        );
        let mut boundary_level = document.parse::<Level>().expect("valid boundary level");
        let allocated_id = boundary_level
            .add_platform(1.0, 1.0, 1.0, 1.0)
            .expect("last non-wrapping platform ID");
        assert_eq!(allocated_id, u64::MAX - 1);
        assert_eq!(boundary_level.next_id(), u64::MAX);
        boundary_level
            .remove_platform(allocated_id)
            .expect("allocated platform");

        let snapshot = boundary_level.clone();
        assert_eq!(
            boundary_level.add_platform(2.0, 2.0, 1.0, 1.0),
            Err(EditError::PlatformIdExhausted)
        );
        assert_eq!(boundary_level, snapshot);
    }
}
