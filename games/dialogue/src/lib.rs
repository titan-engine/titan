//! Game-owned dialogue validation and its Titan tooling definition.

use core::{error::Error, fmt};
use std::{fs, path::Path};

use titan_tools::{Argument, Arguments, Command, CommandResult, JsonValue, Tool, ToolError};

static CHECK_ARGUMENTS: [Argument; 1] =
    [Argument::required("file", "the dialogue file to validate")];

static COMMANDS: [Command; 1] = [Command::new(
    "check",
    "Validate a dialogue file",
    &CHECK_ARGUMENTS,
    check_command,
)];

/// Returns this example game's reusable tooling entry point.
pub fn tool() -> Tool {
    Tool::new(
        "dialogue",
        "Validate small speaker-and-text dialogue documents",
        &COMMANDS,
    )
}

/// The successful result of checking a dialogue document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialogueReport {
    /// Number of valid records in the document.
    pub records: usize,
}

/// A malformed dialogue document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DialogueError {
    /// The document had no records.
    EmptyDocument,
    /// A record did not contain nonempty speaker and text fields.
    MalformedRecord {
        /// One-based source line.
        line: usize,
        /// Reason the record is malformed.
        reason: &'static str,
    },
}

impl fmt::Display for DialogueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDocument => formatter.write_str("document must contain at least one record"),
            Self::MalformedRecord { line, reason } => {
                write!(formatter, "line {line}: {reason}")
            }
        }
    }
}

impl Error for DialogueError {}

/// Checks a dialogue document's `speaker: text` records.
///
/// Every line is one record. A record must contain a colon, and the trimmed
/// speaker and text on either side must both be nonempty. A colon in the text
/// is allowed. Empty documents and blank lines are rejected.
pub fn check_document(input: &str) -> Result<DialogueReport, DialogueError> {
    let mut records = 0;
    for (index, raw_line) in input.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.trim().is_empty() {
            return Err(DialogueError::MalformedRecord {
                line: line_number,
                reason: "record must not be empty",
            });
        }

        let Some((speaker, text)) = line.split_once(':') else {
            return Err(DialogueError::MalformedRecord {
                line: line_number,
                reason: "expected a speaker: text record",
            });
        };
        if speaker.trim().is_empty() {
            return Err(DialogueError::MalformedRecord {
                line: line_number,
                reason: "speaker must not be empty",
            });
        }
        if text.trim().is_empty() {
            return Err(DialogueError::MalformedRecord {
                line: line_number,
                reason: "text must not be empty",
            });
        }
        records += 1;
    }

    if records == 0 {
        return Err(DialogueError::EmptyDocument);
    }
    Ok(DialogueReport { records })
}

fn check_command(arguments: &Arguments<'_>) -> Result<CommandResult, ToolError> {
    let file = arguments.required("file")?;
    let input = fs::read_to_string(Path::new(file)).map_err(|error| {
        ToolError::new(
            "file_error",
            format!(
                "could not read dialogue file {:?}: {error}",
                file.to_string_lossy()
            ),
        )
    })?;
    let report = check_document(&input)
        .map_err(|error| ToolError::new("invalid_document", error.to_string()))?;

    Ok(CommandResult::new(
        format!("valid dialogue: {} record(s)", report.records),
        JsonValue::object([("records", JsonValue::unsigned(report.records as u64))]),
    ))
}
