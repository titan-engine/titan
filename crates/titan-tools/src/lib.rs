//! Shared dispatch, validation, help, and response formatting for game-owned
//! Titan tooling.
//!
//! A game defines its commands as a static slice of [`Command`] values and
//! supplies a handler for each command. [`Tool::dispatch`] then provides the
//! versioned entry point used by Titan's generic launcher. The library does
//! not know a game's commands, document formats, result shapes, or domain
//! error codes.

use core::fmt::{self, Write as FmtWrite};
use std::{
    ffi::{OsStr, OsString},
    io::{self, Write},
};

/// The supported tooling protocol version.
pub const PROTOCOL_VERSION: u32 = 1;
/// The version of JSON envelopes emitted by command mode.
pub const SCHEMA_VERSION: u32 = 1;

/// A game command handler.
///
/// The command has already passed the argument validation described by its
/// [`Command::arguments`] before this function is called.
pub type CommandHandler = for<'a> fn(&Arguments<'a>) -> Result<CommandResult, ToolError>;

/// An optional editor handler.
///
/// Editor mode is deliberately a one-shot CLI invocation. It has no JSON
/// session protocol. The handler may return a game-defined error, which the
/// library writes as human-readable text to standard error.
pub type EditorHandler = fn(&[OsString]) -> Result<(), ToolError>;

/// The two representations a game supplies for a successful command.
///
/// `json` is placed in the versioned envelope selected by `--json`; `human`
/// is written to standard output for an ordinary command invocation. The
/// library never tries to infer a human rendering from game-owned data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandResult {
    json: JsonValue,
    human: String,
}

impl CommandResult {
    /// Creates a command result with a human-readable message and JSON data.
    pub fn new(human: impl Into<String>, json: JsonValue) -> Self {
        Self {
            json,
            human: human.into(),
        }
    }

    /// Returns the structured JSON result data.
    pub fn json(&self) -> &JsonValue {
        &self.json
    }

    /// Returns the human-readable result text.
    pub fn human(&self) -> &str {
        &self.human
    }
}

/// A positional argument described by a command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Argument {
    name: &'static str,
    description: &'static str,
    required: bool,
}

impl Argument {
    /// Defines a required positional argument.
    pub const fn required(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            required: true,
        }
    }

    /// Defines an optional positional argument.
    pub const fn optional(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            required: false,
        }
    }

    /// Returns the argument's display name.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Returns the human and JSON help description.
    pub const fn description(self) -> &'static str {
        self.description
    }

    /// Returns whether the argument must be supplied.
    pub const fn is_required(self) -> bool {
        self.required
    }
}

/// A game-owned command definition.
#[derive(Clone, Copy, Debug)]
pub struct Command {
    name: &'static str,
    description: &'static str,
    arguments: &'static [Argument],
    handler: CommandHandler,
}

impl Command {
    /// Creates a command definition.
    pub const fn new(
        name: &'static str,
        description: &'static str,
        arguments: &'static [Argument],
        handler: CommandHandler,
    ) -> Self {
        Self {
            name,
            description,
            arguments,
            handler,
        }
    }

    /// Returns the command name.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Returns the command description used by both help formats.
    pub const fn description(self) -> &'static str {
        self.description
    }

    /// Returns the positional argument descriptions.
    pub const fn arguments(self) -> &'static [Argument] {
        self.arguments
    }

    /// Returns the canonical command usage fragment.
    pub fn usage(self) -> String {
        let mut usage = self.name.to_owned();
        for argument in self.arguments {
            usage.push(' ');
            if !argument.required {
                usage.push('[');
            }
            usage.push('<');
            usage.push_str(argument.name);
            usage.push('>');
            if !argument.required {
                usage.push(']');
            }
        }
        usage
    }
}

/// A reusable game tooling entry point.
///
/// Games normally construct one value with [`Tool::new`] and call
/// [`Tool::run`] from their `titan-tools` binary. The command descriptions are
/// also available through [`Tool::help_text`] and [`Tool::help_json`], so a
/// game does not maintain separate human and machine help lists.
#[derive(Clone, Copy, Debug)]
pub struct Tool {
    name: &'static str,
    description: &'static str,
    commands: &'static [Command],
    editor: Option<EditorHandler>,
}

impl Tool {
    /// Creates a command-line-only tool with no editor implementation.
    pub const fn new(
        name: &'static str,
        description: &'static str,
        commands: &'static [Command],
    ) -> Self {
        Self {
            name,
            description,
            commands,
            editor: None,
        }
    }

    /// Adds a one-shot editor handler to a tool.
    pub const fn with_editor(self, editor: EditorHandler) -> Self {
        Self {
            editor: Some(editor),
            ..self
        }
    }

    /// Returns the tool name.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Returns the tool description.
    pub const fn description(self) -> &'static str {
        self.description
    }

    /// Returns the command definitions owned by the game.
    pub const fn commands(self) -> &'static [Command] {
        self.commands
    }

    /// Runs the versioned entry point using the process standard streams.
    ///
    /// The returned value is suitable for passing to [`std::process::exit`].
    pub fn run<I>(self, arguments: I) -> i32
    where
        I: IntoIterator<Item = OsString>,
    {
        let stdout = io::stdout();
        let stderr = io::stderr();
        let mut stdout = stdout.lock();
        let mut stderr = stderr.lock();
        self.dispatch(arguments, &mut stdout, &mut stderr)
    }

    /// Dispatches a tooling invocation to caller-provided streams.
    ///
    /// This is useful for embedding a tool or for testing a game handler. A
    /// successful command returns `0`; all tool-level failures return `1`.
    /// Unsupported protocol versions are rejected before command lookup or
    /// handler invocation.
    pub fn dispatch<I>(self, arguments: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32
    where
        I: IntoIterator<Item = OsString>,
    {
        let arguments: Vec<OsString> = arguments.into_iter().collect();
        let Some(protocol) = arguments.first() else {
            return emit_human_error(
                stderr,
                &ToolError::new(
                    "invalid_invocation",
                    "expected --titan-protocol 1 command|editor",
                ),
            );
        };
        if protocol.as_os_str() != "--titan-protocol" {
            return emit_human_error(
                stderr,
                &ToolError::new(
                    "invalid_invocation",
                    format!(
                        "expected --titan-protocol, found {:?}",
                        protocol.to_string_lossy()
                    ),
                ),
            );
        }

        let Some(version_argument) = arguments.get(1) else {
            return emit_human_error(
                stderr,
                &ToolError::new(
                    "invalid_invocation",
                    "missing protocol version after --titan-protocol",
                ),
            );
        };
        let version = match version_argument
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        {
            Some(version) => version,
            None => {
                return emit_human_error(
                    stderr,
                    &ToolError::new(
                        "invalid_protocol_version",
                        format!(
                            "protocol version must be an unsigned integer, found {version_argument:?}"
                        ),
                    ),
                );
            }
        };

        // Keep this check before reading a command name or calling any game
        // handler. The launcher and a tool must agree on the entry contract.
        if version != PROTOCOL_VERSION {
            return emit_human_error(
                stderr,
                &ToolError::new(
                    "unsupported_protocol_version",
                    format!(
                        "unsupported tooling protocol version {version}; expected {PROTOCOL_VERSION}"
                    ),
                ),
            );
        }

        let Some(mode_argument) = arguments.get(2) else {
            return emit_human_error(
                stderr,
                &ToolError::new(
                    "invalid_invocation",
                    "missing tooling mode; expected command or editor",
                ),
            );
        };
        let mode = if mode_argument.as_os_str() == "command" {
            InvocationMode::Command
        } else if mode_argument.as_os_str() == "editor" {
            InvocationMode::Editor
        } else {
            return emit_human_error(
                stderr,
                &ToolError::new(
                    "invalid_invocation",
                    format!(
                        "unsupported tooling mode {:?}; expected command or editor",
                        mode_argument.to_string_lossy()
                    ),
                ),
            );
        };

        let remaining = arguments.get(3..).unwrap_or(&[]);
        match mode {
            InvocationMode::Command => self.dispatch_command(remaining, stdout, stderr),
            InvocationMode::Editor => self.dispatch_editor(remaining, stdout, stderr),
        }
    }

    /// Renders human-readable help from the command definitions.
    pub fn help_text(self) -> String {
        let mut text = String::new();
        text.push_str(self.name);
        text.push_str("\n\n");
        text.push_str(self.description);
        text.push_str(
            "\n\nUsage:\n    titan-tools --titan-protocol 1 command [--json] [--help] <command> [arguments]\n    titan-tools --titan-protocol 1 editor [editor arguments]\n\nCommands:\n",
        );
        if self.commands.is_empty() {
            text.push_str("    (none)\n");
        } else {
            for command in self.commands {
                text.push_str("    ");
                text.push_str(&command.usage());
                text.push_str("    ");
                text.push_str(command.description);
                text.push('\n');
            }
        }
        text.push_str("\nOptions:\n    --help    Show this help or command-specific help\n    --json    Return one versioned JSON response (command mode only)\n    --        Stop option parsing; pass following values literally\n");
        if self.editor.is_none() {
            text.push_str("\nEditor: this CLI-only tool does not provide an editor.\n");
        }
        text
    }

    /// Builds machine-readable help from the same definitions as
    /// [`Tool::help_text`].
    pub fn help_json(self) -> JsonValue {
        let commands = self.commands.iter().map(|command| {
            let arguments = command.arguments.iter().map(|argument| {
                JsonValue::object([
                    ("name", JsonValue::string(argument.name)),
                    ("description", JsonValue::string(argument.description)),
                    ("required", JsonValue::bool(argument.required)),
                ])
            });
            JsonValue::object([
                ("name", JsonValue::string(command.name)),
                ("description", JsonValue::string(command.description)),
                ("usage", JsonValue::string(command.usage())),
                ("arguments", JsonValue::array(arguments)),
            ])
        });

        JsonValue::object([
            ("tool", JsonValue::string(self.name)),
            ("description", JsonValue::string(self.description)),
            (
                "protocol_version",
                JsonValue::unsigned(PROTOCOL_VERSION as u64),
            ),
            ("editor", JsonValue::bool(self.editor.is_some())),
            ("commands", JsonValue::array(commands)),
        ])
    }

    fn dispatch_command(
        self,
        arguments: &[OsString],
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> i32 {
        let mut json = false;
        let mut help = false;
        let mut options = true;
        let mut operands: Vec<&OsString> = Vec::with_capacity(arguments.len());
        for argument in arguments {
            if options && argument.as_os_str() == "--" {
                options = false;
            } else if options && argument.as_os_str() == "--json" {
                json = true;
            } else if options && argument.as_os_str() == "--help" {
                help = true;
            } else {
                operands.push(argument);
            }
        }

        if help {
            match operands.as_slice() {
                [] => {
                    return if json {
                        emit_json_value(self.help_json(), stdout, stderr)
                    } else {
                        emit_human_text(stdout, stderr, &self.help_text())
                    };
                }
                [command_name] => {
                    let Some(command) = self.command(command_name.as_os_str()) else {
                        return self.command_failure(
                            json,
                            stdout,
                            stderr,
                            ToolError::new(
                                "unknown_command",
                                format!("unknown command {:?}", command_name.to_string_lossy()),
                            ),
                        );
                    };
                    return if json {
                        emit_json_value(self.command_help_json(command), stdout, stderr)
                    } else {
                        emit_human_text(stdout, stderr, &self.command_help_text(command))
                    };
                }
                _ => {
                    return self.command_failure(
                        json,
                        stdout,
                        stderr,
                        ToolError::invalid_arguments(
                            "--help accepts no arguments or one command name",
                        ),
                    );
                }
            }
        }

        if operands.is_empty() {
            return if json {
                emit_json_value(self.help_json(), stdout, stderr)
            } else {
                emit_human_text(stdout, stderr, &self.help_text())
            };
        }

        let command_name = operands[0].as_os_str();
        let Some(command) = self.command(command_name) else {
            return self.command_failure(
                json,
                stdout,
                stderr,
                ToolError::new(
                    "unknown_command",
                    format!("unknown command {:?}", command_name.to_string_lossy()),
                ),
            );
        };
        let values = &operands[1..];
        if let Err(error) = validate_arguments(command, values) {
            return self.command_failure(json, stdout, stderr, error);
        }

        let parsed = Arguments { command, values };
        match (command.handler)(&parsed) {
            Ok(result) if json => emit_json_success(result, stdout, stderr),
            Ok(result) => emit_human_text(stdout, stderr, result.human()),
            Err(error) => self.command_failure(json, stdout, stderr, error),
        }
    }

    fn dispatch_editor(
        self,
        arguments: &[OsString],
        _stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> i32 {
        let Some(editor) = self.editor else {
            return emit_human_error(
                stderr,
                &ToolError::new(
                    "unsupported_editor",
                    "this CLI-only tool does not provide an editor",
                ),
            );
        };

        match editor(arguments) {
            Ok(()) => 0,
            Err(error) => emit_human_error(stderr, &error),
        }
    }

    fn command(self, name: &OsStr) -> Option<&'static Command> {
        let name = name.to_str()?;
        self.commands.iter().find(|command| command.name == name)
    }

    fn command_help_text(self, command: &Command) -> String {
        let mut text = String::new();
        text.push_str(command.name);
        text.push_str("\n\n");
        text.push_str(command.description);
        text.push_str("\n\nUsage:\n    titan-tools --titan-protocol 1 command ");
        text.push_str(&command.usage());
        text.push('\n');
        if !command.arguments.is_empty() {
            text.push_str("\nArguments:\n");
            for argument in command.arguments {
                text.push_str("    <");
                text.push_str(argument.name);
                text.push_str(">    ");
                text.push_str(argument.description);
                if argument.required {
                    text.push_str(" (required)");
                } else {
                    text.push_str(" (optional)");
                }
                text.push('\n');
            }
        }
        text.push_str("\nOptions:\n    --help    Show this help\n    --json    Return one versioned JSON response\n    --        Stop option parsing; pass following values literally\n");
        text
    }

    fn command_help_json(self, command: &Command) -> JsonValue {
        JsonValue::object([
            ("tool", JsonValue::string(self.name)),
            ("command", JsonValue::string(command.name)),
            ("description", JsonValue::string(command.description)),
            ("usage", JsonValue::string(command.usage())),
            (
                "arguments",
                JsonValue::array(command.arguments.iter().map(|argument| {
                    JsonValue::object([
                        ("name", JsonValue::string(argument.name)),
                        ("description", JsonValue::string(argument.description)),
                        ("required", JsonValue::bool(argument.required)),
                    ])
                })),
            ),
        ])
    }

    fn command_failure(
        self,
        json: bool,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
        error: ToolError,
    ) -> i32 {
        if json {
            emit_json_error(stdout, stderr, &error)
        } else {
            emit_human_error(stderr, &error)
        }
    }
}

fn emit_json_success(result: CommandResult, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let CommandResult { json, .. } = result;
    emit_json_value(json, stdout, stderr)
}

fn emit_json_value(result: JsonValue, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let envelope = JsonValue::object([
        ("schema_version", JsonValue::unsigned(SCHEMA_VERSION as u64)),
        ("ok", JsonValue::bool(true)),
        ("result", result),
    ]);
    emit_json_line(stdout, stderr, &envelope, 0)
}

#[derive(Clone, Copy)]
enum InvocationMode {
    Command,
    Editor,
}

/// Validated command arguments passed to a game handler.
pub struct Arguments<'a> {
    command: &'static Command,
    values: &'a [&'a OsString],
}

impl<'a> Arguments<'a> {
    /// Returns the number of supplied positional values.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns whether no positional values were supplied.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns a positional value by zero-based index.
    pub fn positional(&self, index: usize) -> Option<&OsStr> {
        self.values.get(index).map(|value| value.as_os_str())
    }

    /// Returns a value by its command-defined argument name.
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        let index = self
            .command
            .arguments
            .iter()
            .position(|argument| argument.name == name)?;
        self.positional(index)
    }

    /// Returns a required value by name.
    ///
    /// Dispatch validates required positional arguments before invoking a
    /// handler. This method reports the standard validation error when the
    /// named value is absent.
    pub fn required(&self, name: &str) -> Result<&OsStr, ToolError> {
        self.get(name).ok_or_else(|| {
            ToolError::invalid_arguments(format!("missing required argument <{name}>"))
        })
    }
}

/// A game-defined result or domain failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolError {
    code: String,
    message: String,
}

impl ToolError {
    /// Creates an error with a game-owned machine-readable code and message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// Creates the standard command-line argument validation error.
    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self::new("invalid_arguments", message)
    }

    /// Returns the machine-readable error code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl core::error::Error for ToolError {}

/// A small dependency-free JSON value used for tool result data and envelopes.
///
/// Constructors only accept JSON-safe scalar values. Objects preserve the
/// insertion order supplied by the game, and all strings are escaped by the
/// serializer. This keeps the library usable by standalone games without a
/// reflection or code-generation system.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonValue(JsonValueKind);

#[derive(Clone, Debug, Eq, PartialEq)]
enum JsonValueKind {
    Null,
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// Creates a JSON null.
    pub const fn null() -> Self {
        Self(JsonValueKind::Null)
    }

    /// Creates a JSON boolean.
    pub const fn bool(value: bool) -> Self {
        Self(JsonValueKind::Bool(value))
    }

    /// Creates a signed JSON integer.
    pub const fn number(value: i64) -> Self {
        Self(JsonValueKind::Signed(value))
    }

    /// Creates an unsigned JSON integer.
    pub const fn unsigned(value: u64) -> Self {
        Self(JsonValueKind::Unsigned(value))
    }

    /// Creates a JSON string.
    pub fn string(value: impl Into<String>) -> Self {
        Self(JsonValueKind::String(value.into()))
    }

    /// Creates a JSON array.
    pub fn array<I>(values: I) -> Self
    where
        I: IntoIterator<Item = Self>,
    {
        Self(JsonValueKind::Array(values.into_iter().collect()))
    }

    /// Creates a JSON object from key/value pairs.
    pub fn object<I, K>(values: I) -> Self
    where
        I: IntoIterator<Item = (K, Self)>,
        K: Into<String>,
    {
        Self(JsonValueKind::Object(
            values
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        ))
    }

    /// Serializes this value as valid compact JSON.
    pub fn to_json(&self) -> String {
        let mut output = String::new();
        self.write_json(&mut output);
        output
    }

    fn write_json(&self, output: &mut String) {
        match &self.0 {
            JsonValueKind::Null => output.push_str("null"),
            JsonValueKind::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            JsonValueKind::Signed(value) => output.push_str(&value.to_string()),
            JsonValueKind::Unsigned(value) => output.push_str(&value.to_string()),
            JsonValueKind::String(value) => write_json_string(output, value),
            JsonValueKind::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    value.write_json(output);
                }
                output.push(']');
            }
            JsonValueKind::Object(fields) => {
                output.push('{');
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    write_json_string(output, key);
                    output.push(':');
                    value.write_json(output);
                }
                output.push('}');
            }
        }
    }
}

fn write_json_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character <= '\u{1f}' => {
                let _ = write!(output, "\\u{:04x}", character as u32);
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

fn validate_arguments(command: &Command, values: &[&OsString]) -> Result<(), ToolError> {
    if values.len() > command.arguments.len() {
        return Err(ToolError::invalid_arguments(format!(
            "expected at most {} argument(s) for '{}'; usage: {}",
            command.arguments.len(),
            command.name,
            command.usage()
        )));
    }
    for (index, argument) in command.arguments.iter().enumerate() {
        if argument.required && values.get(index).is_none() {
            return Err(ToolError::invalid_arguments(format!(
                "missing required argument <{}> for '{}'; usage: {}",
                argument.name,
                command.name,
                command.usage()
            )));
        }
    }
    Ok(())
}

fn emit_human_text(stdout: &mut dyn Write, stderr: &mut dyn Write, text: &str) -> i32 {
    if text.is_empty() {
        return 0;
    }
    if let Err(error) = stdout.write_all(text.as_bytes()) {
        let _ = writeln!(stderr, "error: could not write tool output: {error}");
        return 1;
    }
    if !text.ends_with('\n')
        && let Err(error) = stdout.write_all(b"\n")
    {
        let _ = writeln!(stderr, "error: could not write tool output: {error}");
        return 1;
    }
    0
}

fn emit_human_error(stderr: &mut dyn Write, error: &ToolError) -> i32 {
    let _ = writeln!(stderr, "error [{}]: {}", error.code, error.message);
    1
}

fn emit_json_error(stdout: &mut dyn Write, stderr: &mut dyn Write, error: &ToolError) -> i32 {
    let envelope = JsonValue::object([
        ("schema_version", JsonValue::unsigned(SCHEMA_VERSION as u64)),
        ("ok", JsonValue::bool(false)),
        (
            "error",
            JsonValue::object([
                ("code", JsonValue::string(error.code.clone())),
                ("message", JsonValue::string(error.message.clone())),
            ]),
        ),
    ]);
    emit_json_line(stdout, stderr, &envelope, 1)
}

fn emit_json_line(
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    value: &JsonValue,
    status: i32,
) -> i32 {
    let json = value.to_json();
    match writeln!(stdout, "{json}") {
        Ok(()) => status,
        Err(error) => {
            let _ = writeln!(stderr, "error: could not write JSON tool response: {error}");
            1
        }
    }
}
