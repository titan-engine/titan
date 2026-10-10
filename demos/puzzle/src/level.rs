//! Validated, immutable push-puzzle levels and their dependency-free text parser.
//!
//! Version 1 uses the following sections, in order:
//! ```text
//! puzzle 1
//! id first-push
//! title First Push
//! grid
//! #####
//! #...#
//! #...#
//! #####
//! objects
//! player hero 1 1
//! block crate 2 1
//! target goal 3 1
//! end
//! ```
//! Coordinates are zero-based: x increases right, y increases down. Grid rows
//! contain only `#` (wall) and `.` (floor); targets are separate objects, never
//! terrain. The grid must be a closed rectangle, 3..=64 cells on each axis.
//! Blank lines are ignored outside the grid, but count toward diagnostic line
//! numbers. There are no comments. Headers and object records use whitespace
//! separators; titles may contain spaces. IDs start with an ASCII letter and
//! continue with ASCII letters, digits, `_`, or `-`. Object IDs share one
//! namespace; the level ID has its own namespace. Object order is preserved.
//!
//! Only a block and a target may share a cell. There must be exactly one player
//! and a positive, equal number of blocks and targets, with at least one target
//! initially uncovered. Validation does not prove solvability.

use core::fmt;
use std::{
    collections::{HashMap, HashSet},
    error::Error,
};

/// A zero-based grid coordinate, with positive y pointing down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cell {
    /// Horizontal coordinate.
    pub x: i32,
    /// Vertical coordinate.
    pub y: i32,
}

impl Cell {
    /// Creates a coordinate; bounds are checked by the level, not this value.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// An authored object with an ID stable across simulation and presentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    /// Unique authored ID within the level's object namespace.
    pub id: String,
    /// Initial position in the level.
    pub position: Cell,
}

/// Immutable initial state whose structural invariants have been validated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level {
    id: String,
    title: String,
    width: usize,
    height: usize,
    walls: Vec<bool>,
    player: Object,
    blocks: Vec<Object>,
    targets: Vec<Object>,
}

impl Level {
    /// The stable authored level ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The human-readable title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Width in grid cells, including boundary walls.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in grid cells, including boundary walls.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Whether a cell is solid. Every position outside the grid is solid too.
    pub fn is_wall(&self, cell: Cell) -> bool {
        if cell.x < 0 || cell.y < 0 {
            return true;
        }
        let (x, y) = (cell.x as usize, cell.y as usize);
        x >= self.width || y >= self.height || self.walls[y * self.width + x]
    }

    /// The single player's authored initial state.
    pub fn player(&self) -> &Object {
        &self.player
    }

    /// Blocks in declaration order.
    pub fn blocks(&self) -> &[Object] {
        &self.blocks
    }

    /// Targets in declaration order, independent of floor cells and blocks.
    pub fn targets(&self) -> &[Object] {
        &self.targets
    }

    /// Parses and validates version 1 text without performing any I/O.
    ///
    /// `source` is a filename or other descriptive source label used verbatim
    /// in errors. Locations are one-based physical lines and character columns.
    ///
    /// # Errors
    ///
    /// Returns a located error for unsupported syntax, malformed terrain, or
    /// invalid initial object state. Solvability is not checked.
    pub fn parse(source: &str, text: &str) -> Result<Self, LevelError> {
        let mut input = Input::new(source, text);
        let (line, header) = input.required("`puzzle 1` header")?;
        let fields = tokens(header);
        expect_fields(source, line, header, &fields, 2)?;
        expect_keyword(source, line, &fields[0], "puzzle")?;
        if fields[1].text != "1" {
            return Err(input.error(
                line,
                fields[1].column,
                "expected supported format `puzzle 1`",
            ));
        }

        let (line, header) = input.required("`id <level-id>` header")?;
        let fields = tokens(header);
        expect_fields(source, line, header, &fields, 2)?;
        expect_keyword(source, line, &fields[0], "id")?;
        validate_id(source, line, &fields[1])?;
        let id = fields[1].text.to_owned();

        let (line, header) = input.required("`title <title>` header")?;
        let fields = tokens(header);
        let Some(keyword) = fields.first() else {
            unreachable!("required skips blank lines");
        };
        expect_keyword(source, line, keyword, "title")?;
        let title = header.trim_start()["title".len()..].trim();
        if title.is_empty() {
            return Err(input.error(line, header.chars().count() + 1, "title must not be empty"));
        }
        let title = title.to_owned();
        input.marker("grid")?;

        let mut rows: Vec<(usize, &str)> = Vec::new();
        let objects_line;
        loop {
            let Some((line, row)) = input.raw() else {
                return Err(input.eof("expected `objects` after the grid"));
            };
            if row.trim() == "objects" {
                objects_line = line;
                break;
            }
            if rows.len() == 64 {
                return Err(input.error(line, 1, "grid height must be 3..=64"));
            }
            let width = row.chars().count();
            if !(3..=64).contains(&width) {
                return Err(input.error(line, 1, "grid row width must be 3..=64"));
            }
            if let Some((_, first)) = rows.first()
                && width != first.chars().count()
            {
                return Err(input.error(
                    line,
                    width.min(first.chars().count()) + 1,
                    "grid rows must have equal widths",
                ));
            }
            for (index, tile) in row.chars().enumerate() {
                if tile != '#' && tile != '.' {
                    return Err(input.error(
                        line,
                        index + 1,
                        "grid accepts only `#` (wall) and `.` (floor)",
                    ));
                }
            }
            rows.push((line, row));
        }
        if rows.len() < 3 {
            return Err(input.error(objects_line, 1, "grid height must be 3..=64"));
        }
        let width = rows[0].1.len();
        let height = rows.len();
        let mut walls = Vec::with_capacity(width * height);
        for (y, (line, row)) in rows.iter().enumerate() {
            for (x, tile) in row.chars().enumerate() {
                if (x == 0 || y == 0 || x + 1 == width || y + 1 == height) && tile != '#' {
                    return Err(input.error(
                        *line,
                        x + 1,
                        "grid boundary must be closed with `#` walls",
                    ));
                }
                walls.push(tile == '#');
            }
        }

        let mut player = None;
        let mut blocks = Vec::new();
        let mut targets = Vec::new();
        let mut ids = HashSet::new();
        let mut occupied: HashMap<Cell, Vec<&str>> = HashMap::new();
        loop {
            let (line, record) = input.required("object record or `end`")?;
            let fields = tokens(record);
            if fields[0].text == "end" {
                expect_fields(source, line, record, &fields, 1)?;
                break;
            }
            expect_fields(source, line, record, &fields, 4)?;
            let kind = fields[0].text;
            if !matches!(kind, "player" | "block" | "target") {
                return Err(input.error(
                    line,
                    fields[0].column,
                    "expected `player`, `block`, `target`, or `end`",
                ));
            }
            validate_id(source, line, &fields[1])?;
            if !ids.insert(fields[1].text) {
                return Err(input.error(
                    line,
                    fields[1].column,
                    format!("duplicate object ID `{}`", fields[1].text),
                ));
            }
            let mut coordinates = [0; 2];
            for (axis, field) in fields[2..].iter().enumerate() {
                coordinates[axis] = field.text.parse::<i32>().map_err(|_| {
                    input.error(
                        line,
                        field.column,
                        "coordinate must be a signed 32-bit integer",
                    )
                })?;
                let bound = if axis == 0 { width } else { height };
                if coordinates[axis] < 0 || coordinates[axis] as usize >= bound {
                    return Err(input.error(
                        line,
                        field.column,
                        format!("coordinate outside grid; expected 0..{}", bound - 1),
                    ));
                }
            }
            let position = Cell::new(coordinates[0], coordinates[1]);
            if walls[position.y as usize * width + position.x as usize] {
                return Err(input.error(
                    line,
                    fields[2].column,
                    "object must be placed on floor, not a wall",
                ));
            }
            if kind == "player" && player.is_some() {
                return Err(input.error(
                    line,
                    fields[0].column,
                    "exactly one player is required; a player is already declared",
                ));
            }
            let occupants = occupied.entry(position).or_default();
            for previous in occupants.iter() {
                if !matches!((kind, *previous), ("block", "target") | ("target", "block")) {
                    return Err(input.error(
                        line,
                        fields[2].column,
                        format!(
                            "{kind} overlaps {previous}; only a block and a target may overlap"
                        ),
                    ));
                }
            }
            occupants.push(kind);
            let object = Object {
                id: fields[1].text.to_owned(),
                position,
            };
            match kind {
                "player" => player = Some(object),
                "block" => blocks.push(object),
                "target" => targets.push(object),
                _ => unreachable!("object kind was validated"),
            }
        }
        if let Some((line, record)) = input.nonblank() {
            return Err(input.error(
                line,
                tokens(record)[0].column,
                "unexpected content after `end`",
            ));
        }
        let player = player.ok_or_else(|| {
            input.error(
                objects_line,
                1,
                "exactly one player is required; add a `player <id> <x> <y>` record",
            )
        })?;
        if blocks.is_empty() || blocks.len() != targets.len() {
            return Err(input.error(objects_line, 1, format!("need a positive equal number of blocks and targets; found {} blocks and {} targets", blocks.len(), targets.len())));
        }
        let block_cells: HashSet<_> = blocks.iter().map(|block| block.position).collect();
        if targets
            .iter()
            .all(|target| block_cells.contains(&target.position))
        {
            return Err(input.error(
                objects_line,
                1,
                "initial state is already solved; leave at least one target uncovered",
            ));
        }
        Ok(Self {
            id,
            title,
            width,
            height,
            walls,
            player,
            blocks,
            targets,
        })
    }
}

/// A syntax or semantic error, with a physical source location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelError {
    source: String,
    line: usize,
    column: usize,
    message: String,
}

impl LevelError {
    /// Filename or source label supplied to [`Level::parse`].
    pub fn source(&self) -> &str {
        &self.source
    }
    /// One-based physical line; missing input points to the next line at EOF.
    pub fn line(&self) -> usize {
        self.line
    }
    /// One-based character column, not a byte offset.
    pub fn column(&self) -> usize {
        self.column
    }
    /// Actionable explanation of the failed validation.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for LevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.source, self.line, self.column, self.message
        )
    }
}

impl Error for LevelError {}

fn error(source: &str, line: usize, column: usize, message: impl Into<String>) -> LevelError {
    LevelError {
        source: source.to_owned(),
        line,
        column,
        message: message.into(),
    }
}

struct Input<'a> {
    source: &'a str,
    lines: std::iter::Enumerate<std::str::Lines<'a>>,
    last_line: usize,
}

impl<'a> Input<'a> {
    fn new(source: &'a str, text: &'a str) -> Self {
        Self {
            source,
            lines: text.lines().enumerate(),
            last_line: 0,
        }
    }
    fn error(&self, line: usize, column: usize, message: impl Into<String>) -> LevelError {
        error(self.source, line, column, message)
    }
    fn eof(&self, message: impl Into<String>) -> LevelError {
        self.error(self.last_line + 1, 1, message)
    }
    fn raw(&mut self) -> Option<(usize, &'a str)> {
        let (index, text) = self.lines.next()?;
        self.last_line = index + 1;
        Some((index + 1, text))
    }
    fn nonblank(&mut self) -> Option<(usize, &'a str)> {
        while let Some((line, text)) = self.raw() {
            if !text.trim().is_empty() {
                return Some((line, text));
            }
        }
        None
    }
    fn required(&mut self, expected: &str) -> Result<(usize, &'a str), LevelError> {
        self.nonblank()
            .ok_or_else(|| self.eof(format!("expected {expected}")))
    }
    fn marker(&mut self, expected: &str) -> Result<(), LevelError> {
        let (line, text) = self.required(expected)?;
        let fields = tokens(text);
        expect_fields(self.source, line, text, &fields, 1)?;
        expect_keyword(self.source, line, &fields[0], expected)
    }
}

struct Token<'a> {
    text: &'a str,
    column: usize,
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut offset = 0;
    text.split_whitespace()
        .map(|token| {
            let start = offset + text[offset..].find(token).expect("token belongs to text");
            offset = start + token.len();
            Token {
                text: token,
                column: text[..start].chars().count() + 1,
            }
        })
        .collect()
}

fn expect_fields(
    source: &str,
    line: usize,
    text: &str,
    fields: &[Token<'_>],
    count: usize,
) -> Result<(), LevelError> {
    if fields.len() != count {
        let column = fields
            .get(count)
            .map_or(text.chars().count() + 1, |field| field.column);
        return Err(error(
            source,
            line,
            column,
            format!("expected {count} fields, found {}", fields.len()),
        ));
    }
    Ok(())
}

fn expect_keyword(
    source: &str,
    line: usize,
    field: &Token<'_>,
    expected: &str,
) -> Result<(), LevelError> {
    if field.text != expected {
        return Err(error(
            source,
            line,
            field.column,
            format!("expected `{expected}`"),
        ));
    }
    Ok(())
}

fn validate_id(source: &str, line: usize, field: &Token<'_>) -> Result<(), LevelError> {
    for (index, ch) in field.text.chars().enumerate() {
        if !(ch.is_ascii_alphabetic()
            || (index > 0 && (ch.is_ascii_digit() || ch == '_' || ch == '-')))
        {
            return Err(error(source, line, field.column + index, "ID must start with an ASCII letter and contain only ASCII letters, digits, `_`, or `-`"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "puzzle 1\nid first-push\ntitle First Push\ngrid\n#####\n#...#\n#...#\n#####\nobjects\nplayer hero 1 1\nblock crate 2 1\ntarget goal 3 1\nend\n";

    fn rejected(text: &str, line: usize, column: usize, message: &str) {
        let err = Level::parse("broken.puzzle", text).unwrap_err();
        assert_eq!(
            (err.source(), err.line(), err.column()),
            ("broken.puzzle", line, column)
        );
        assert!(err.message().contains(message), "{err}");
        assert!(err
            .to_string()
            .starts_with(&format!("broken.puzzle:{line}:{column}: ")));
    }

    #[test]
    fn parses_metadata_terrain_and_stable_objects() {
        let level = Level::parse("memory", VALID).unwrap();
        assert_eq!(level.id(), "first-push");
        assert_eq!(level.title(), "First Push");
        assert_eq!((level.width(), level.height()), (5, 4));
        assert_eq!(
            level.player(),
            &Object {
                id: "hero".into(),
                position: Cell::new(1, 1)
            }
        );
        assert_eq!(level.blocks()[0].id, "crate");
        assert_eq!(level.targets()[0].id, "goal");
        assert!(!level.is_wall(level.targets()[0].position));
        for cell in [
            Cell::new(0, 0),
            Cell::new(-1, 1),
            Cell::new(1, -1),
            Cell::new(5, 1),
            Cell::new(1, 4),
            Cell::new(i32::MAX, i32::MAX),
        ] {
            assert!(level.is_wall(cell));
        }
        assert_eq!(level, level.clone());
    }

    #[test]
    fn preserves_physical_lines_and_accepts_crlf() {
        let text = format!(
            "\n{}",
            VALID
                .replace("objects\n", "objects\n\n")
                .replace("crate 2", "crate -1")
        );
        rejected(&text, 13, 13, "outside grid");
        assert_eq!(
            Level::parse("unix", VALID).unwrap(),
            Level::parse("windows", &VALID.replace('\n', "\r\n")).unwrap()
        );
    }

    #[test]
    fn diagnoses_headers_and_record_syntax() {
        rejected(
            &VALID.replace("puzzle 1", "puzzle 2"),
            1,
            8,
            "supported format",
        );
        rejected(
            &VALID.replace("id first-push", "name first-push"),
            2,
            1,
            "expected `id`",
        );
        rejected(&VALID.replace("title First Push", "title"), 3, 6, "empty");
        rejected(
            &VALID.replace("crate 2 1", "crate 2"),
            11,
            14,
            "expected 4 fields",
        );
        rejected(
            &VALID.replace("crate 2 1", "crate 2 1 extra"),
            11,
            17,
            "expected 4 fields",
        );
        rejected(
            &VALID.replace("block crate", "crate crate"),
            11,
            1,
            "expected `player`",
        );
        rejected(&VALID.replace("crate 2", "crate two"), 11, 13, "integer");
        rejected(
            &VALID.replace("crate 2", "crate 2147483648"),
            11,
            13,
            "integer",
        );
        rejected(&format!("{VALID}extra\n"), 14, 1, "after `end`");
        rejected(
            VALID.trim_end_matches("end\n"),
            13,
            1,
            "object record or `end`",
        );
        rejected("", 1, 1, "header");
    }

    #[test]
    fn validates_ids_in_separate_level_and_object_namespaces() {
        rejected(&VALID.replace("first-push", "9bad"), 2, 4, "ID must start");
        rejected(&VALID.replace("crate", "cr@te"), 11, 9, "ID must start");
        rejected(&VALID.replace("crate", "créte"), 11, 9, "ID must start");
        rejected(
            &VALID.replace("target goal", "target crate"),
            12,
            8,
            "duplicate object ID",
        );
        assert!(Level::parse("memory", &VALID.replace("hero", "first-push")).is_ok());
    }

    #[test]
    fn validates_grid_shape_tiles_and_closed_boundary() {
        rejected(
            &VALID.replace("#...#\n#...#", "#..#\n#...#"),
            6,
            5,
            "equal widths",
        );
        rejected(
            &VALID.replace("#...#\n#...#", "#.@.#\n#...#"),
            6,
            3,
            "grid accepts only",
        );
        rejected(
            &VALID.replace("#####\n#...#", "##.##\n#...#"),
            5,
            3,
            "boundary",
        );
        rejected(
            &VALID.replace("#...#\n#...#", "#...#\n....#"),
            7,
            1,
            "boundary",
        );
        rejected(
            &VALID.replace("#####\nobjects", "####.\nobjects"),
            8,
            5,
            "boundary",
        );
        rejected(&VALID.replace("#...#\n", ""), 7, 1, "height");
        rejected(&VALID.replace("#####\n#...#", "##\n#...#"), 5, 1, "width");
        rejected(
            &VALID.replace("#####\n#...#", &format!("{}\n#...#", "#".repeat(65))),
            5,
            1,
            "width",
        );
        rejected(
            "puzzle 1\nid test\ntitle Test\ngrid\n#####\n",
            6,
            1,
            "expected `objects`",
        );
    }

    #[test]
    fn validates_object_bounds_walls_and_overlaps() {
        rejected(
            &VALID.replace("crate 2 1", "crate 5 1"),
            11,
            13,
            "outside grid",
        );
        rejected(
            &VALID.replace("crate 2 1", "crate 2 -1"),
            11,
            15,
            "outside grid",
        );
        rejected(
            &VALID.replace("crate 2 1", "crate 0 1"),
            11,
            13,
            "not a wall",
        );
        rejected(
            &VALID.replace("crate 2 1", "crate 1 1"),
            11,
            13,
            "block overlaps player",
        );
        rejected(
            &VALID.replace("goal 3 1", "goal 1 1"),
            12,
            13,
            "target overlaps player",
        );
        rejected(
            &VALID.replace("end\n", "block other 2 1\nend\n"),
            13,
            13,
            "block overlaps block",
        );
        rejected(
            &VALID.replace("end\n", "target other 3 1\nend\n"),
            13,
            14,
            "target overlaps target",
        );
    }

    #[test]
    fn validates_counts_and_unsolved_initial_state() {
        rejected(
            &VALID.replace("player hero 1 1\n", ""),
            9,
            1,
            "add a `player",
        );
        rejected(
            &VALID.replace("end\n", "player other 1 2\nend\n"),
            13,
            1,
            "already declared",
        );
        rejected(
            &VALID.replace("block crate 2 1\n", ""),
            9,
            1,
            "positive equal",
        );
        rejected(
            &VALID.replace("target goal 3 1\n", ""),
            9,
            1,
            "positive equal",
        );
        rejected(
            &VALID.replace("block crate 2 1\ntarget goal 3 1\n", ""),
            9,
            1,
            "positive equal",
        );
        rejected(
            &VALID.replace("goal 3 1", "goal 2 1"),
            9,
            1,
            "already solved",
        );
    }

    #[test]
    fn permits_block_on_target_in_either_declaration_order() {
        let text = VALID.replace("end\n", "block second 2 2\ntarget covered 2 2\nend\n");
        let level = Level::parse("memory", &text).unwrap();
        assert_eq!(level.blocks()[1].position, level.targets()[1].position);
        assert_eq!(level.blocks()[1].id, "second");
        assert!(Level::parse(
            "memory",
            &text.replace(
                "block second 2 2\ntarget covered 2 2",
                "target covered 2 2\nblock second 2 2"
            )
        )
        .is_ok());
    }

    #[test]
    fn parses_embedded_starters_with_distinct_ids() {
        let sources = [
            include_str!("../levels/01-first-push.puzzle"),
            include_str!("../levels/02-turn-the-corner.puzzle"),
            include_str!("../levels/03-two-deliveries.puzzle"),
            include_str!("../levels/04-around-the-wall.puzzle"),
        ];
        let mut ids = HashSet::new();
        for source in sources {
            let level = Level::parse("embedded starter", source).unwrap();
            assert!(ids.insert(level.id().to_owned()));
        }
    }

    #[test]
    fn supports_minimum_axis_and_unicode_character_columns() {
        let narrow = "puzzle 1\nid narrow\ntitle Narrow\ngrid\n###\n#.#\n#.#\n#.#\n###\nobjects\nplayer hero 1 1\nblock crate 1 2\ntarget goal 1 3\nend\n";
        assert_eq!(Level::parse("narrow", narrow).unwrap().width(), 3);
        let unicode = VALID.replace("title First Push", "title Première poussée");
        assert_eq!(
            Level::parse("unicode", &unicode).unwrap().title(),
            "Première poussée"
        );
        rejected(
            &VALID.replace("block crate 2 1", "\u{3000}block crate 5 1"),
            11,
            14,
            "outside grid",
        );
    }

    #[test]
    fn supports_maximum_grid_and_rejects_excess_height() {
        let rows = std::iter::once("#".repeat(64))
            .chain(std::iter::repeat_n(format!("#{}#", ".".repeat(62)), 62))
            .chain(std::iter::once("#".repeat(64)))
            .collect::<Vec<_>>()
            .join("\n");
        let text = format!("puzzle 1\nid large\ntitle Large\ngrid\n{rows}\nobjects\nplayer hero 1 1\nblock crate 2 1\ntarget goal 3 1\nend\n");
        let level = Level::parse("large", &text).unwrap();
        assert_eq!((level.width(), level.height()), (64, 64));
        rejected(
            &text.replace("\nobjects", &format!("\n{}\nobjects", "#".repeat(64))),
            69,
            1,
            "height",
        );
    }
}
