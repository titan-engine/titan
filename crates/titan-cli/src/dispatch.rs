use crate::{args::CliCommand, local::LocalCommand};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use titan_protocol::{EntityId, EntityQuery, InputValue, PageRequest, Request};

/// The only CLI command classification boundary. Every variant must explicitly
/// select local execution, discovery, or a typed inspection request.
pub(crate) enum CommandRoute<'a> {
    Local(LocalCommand<'a>),
    Remote(Option<Request>),
}

pub(crate) type LocalError = (String, String);

pub(crate) fn classify(command: &CliCommand) -> Result<CommandRoute<'_>, LocalError> {
    fn object<T: serde::de::DeserializeOwned>(
        value: &str,
    ) -> Result<BTreeMap<String, T>, LocalError> {
        serde_json::from_str(value).map_err(|error| {
            (
                "invalid_value".into(),
                format!("expected a JSON object: {error}"),
            )
        })
    }
    fn arguments(
        inline: &str,
        file: &Option<PathBuf>,
    ) -> Result<BTreeMap<String, serde_json::Value>, LocalError> {
        let Some(path) = file else {
            return object(inline);
        };
        const LIMIT: u64 = 1024 * 1024;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        // Opening a FIFO must not block before we can reject its file type.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let input = options.open(path).map_err(|error| {
            (
                "invalid_value".into(),
                format!("cannot open arguments file: {error}"),
            )
        })?;
        let metadata = input.metadata().map_err(|error| {
            (
                "invalid_value".into(),
                format!("cannot inspect arguments file: {error}"),
            )
        })?;
        if !metadata.is_file() || metadata.len() > LIMIT {
            return Err((
                "invalid_value".into(),
                "arguments file must be a regular file of at most 1 MiB".into(),
            ));
        }
        let mut bytes = Vec::new();
        input
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                (
                    "invalid_value".into(),
                    format!("cannot read arguments file: {error}"),
                )
            })?;
        if bytes.len() as u64 > LIMIT {
            return Err((
                "invalid_value".into(),
                "arguments file exceeds 1 MiB".into(),
            ));
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            (
                "invalid_value".into(),
                "arguments file must contain UTF-8 JSON".into(),
            )
        })?;
        object(text)
    }
    let request = match command {
        CliCommand::Capabilities => Request::Capabilities,
        CliCommand::Status => Request::Status,
        CliCommand::Entities {
            name,
            components,
            cursor,
            limit,
        } => Request::Entities {
            query: EntityQuery {
                name: name.clone(),
                with_components: components.clone(),
            },
            page: PageRequest {
                cursor: cursor.clone(),
                limit: *limit,
            },
        },
        CliCommand::Entity { index, generation } => Request::Entity {
            entity: EntityId {
                index: *index,
                generation: *generation,
            },
        },
        CliCommand::SetField {
            index,
            generation,
            component,
            field,
            value,
        } => Request::SetField {
            entity: EntityId {
                index: *index,
                generation: *generation,
            },
            component: component.clone(),
            field: field.clone(),
            value: serde_json::from_str(value).map_err(|error| {
                (
                    "invalid_value".into(),
                    format!("expected a JSON value: {error}"),
                )
            })?,
        },
        CliCommand::Commands => Request::Commands,
        CliCommand::Queries => Request::Queries,
        CliCommand::Query {
            name,
            arguments: inline,
            arguments_file,
        } => Request::Query {
            name: name.clone(),
            arguments: arguments(inline, arguments_file)?,
        },
        CliCommand::Step { frames } => Request::Step { frames: *frames },
        CliCommand::Input { frame, actions } => Request::InjectInput {
            frame: *frame,
            actions: object::<InputValue>(actions)?,
        },
        CliCommand::Invoke {
            name,
            arguments: inline,
            arguments_file,
        } => Request::Invoke {
            name: name.clone(),
            arguments: arguments(inline, arguments_file)?,
        },
        CliCommand::Capture => Request::Capture,
        CliCommand::Instances => return Ok(CommandRoute::Remote(None)),
        CliCommand::Info => return Ok(CommandRoute::Local(LocalCommand::Info)),
        CliCommand::Check => return Ok(CommandRoute::Local(LocalCommand::Check)),
        CliCommand::Test => return Ok(CommandRoute::Local(LocalCommand::Test)),
        CliCommand::RunExample { name } => {
            return Ok(CommandRoute::Local(LocalCommand::RunExample { name }));
        }
        CliCommand::CompareImages {
            expected,
            actual,
            output,
            exact,
            maximum_channel_error,
            minimum_ssim,
            maximum_linear_rmse,
        } => {
            return Ok(CommandRoute::Local(LocalCommand::CompareImages {
                expected,
                actual,
                output,
                exact: *exact,
                maximum_channel_error: *maximum_channel_error,
                minimum_ssim: *minimum_ssim,
                maximum_linear_rmse: *maximum_linear_rmse,
            }));
        }
    };
    Ok(CommandRoute::Remote(Some(request)))
}

#[cfg(test)]
mod tests {
    use super::{CommandRoute, LocalError, classify};
    use crate::args::CliCommand;
    fn request_for(command: &CliCommand) -> Result<titan_protocol::Request, LocalError> {
        match classify(command)? {
            CommandRoute::Remote(Some(request)) => Ok(request),
            _ => panic!("expected an inspection request"),
        }
    }
    use crate::args::Cli;
    use clap::Parser;

    #[test]
    fn argument_files_are_bounded_objects_and_conflict_with_inline_arguments() {
        let path =
            std::env::temp_dir().join(format!("titan-argument-file-{}.json", std::process::id()));
        let path_str = path.to_str().unwrap();
        for command in ["query", "invoke"] {
            std::fs::write(&path, br#"{"save":{"format_version":1}}"#).unwrap();
            let cli = Cli::try_parse_from(["titan", command, "save", "--arguments-file", path_str])
                .unwrap();
            let request = request_for(&cli.command).unwrap();
            let values = match request {
                titan_protocol::Request::Query { arguments, .. }
                | titan_protocol::Request::Invoke { arguments, .. } => arguments,
                other => panic!("unexpected request {other:?}"),
            };
            assert_eq!(values["save"]["format_version"], 1);
            assert!(
                Cli::try_parse_from([
                    "titan",
                    command,
                    "save",
                    "--arguments",
                    "{}",
                    "--arguments-file",
                    path_str
                ])
                .is_err()
            );
            for invalid in [b"[]".to_vec(), vec![0xff], vec![b' '; 1024 * 1024 + 1]] {
                std::fs::write(&path, invalid).unwrap();
                assert_eq!(request_for(&cli.command).unwrap_err().0, "invalid_value");
            }
            std::fs::remove_file(&path).unwrap();
            assert_eq!(request_for(&cli.command).unwrap_err().0, "invalid_value");
        }
    }

    #[test]
    fn remote_payloads_are_typed_and_reject_invalid_json() {
        let cli = Cli::try_parse_from([
            "titan",
            "--project",
            "/tmp/game",
            "--instance",
            "one",
            "step",
            "12",
            "--timeout-ms",
            "40",
        ])
        .unwrap();
        assert_eq!(cli.project, std::path::Path::new("/tmp/game"));
        assert_eq!(cli.instance.as_deref(), Some("one"));
        assert_eq!(
            request_for(&cli.command).unwrap(),
            titan_protocol::Request::Step { frames: 12 }
        );
        for arguments in ["[1]", "null", "broken"] {
            let cli = Cli::try_parse_from(["titan", "invoke", "reset", "--arguments", arguments])
                .unwrap();
            assert_eq!(request_for(&cli.command).unwrap_err().0, "invalid_value");
        }
        let cli = Cli::try_parse_from([
            "titan",
            "input",
            "3",
            "--actions",
            r#"{"jump":{"kind":"button","value":true}}"#,
        ])
        .unwrap();
        assert!(matches!(
            request_for(&cli.command).unwrap(),
            titan_protocol::Request::InjectInput { frame: 3, .. }
        ));
        assert!(Cli::try_parse_from(["titan", "status", "--timeout-ms", "0"]).is_err());
        assert!(Cli::try_parse_from(["titan", "entities", "--limit", "0"]).is_err());
    }

    #[test]
    fn read_only_query_arguments_are_parsed_before_discovery() {
        let cli = Cli::try_parse_from([
            "titan",
            "query",
            "arena_state",
            "--arguments",
            r#"{"limit":2}"#,
        ])
        .unwrap();
        assert!(
            matches!(request_for(&cli.command).unwrap(), titan_protocol::Request::Query { name, arguments } if name == "arena_state" && arguments["limit"] == 2)
        );
        let invalid =
            Cli::try_parse_from(["titan", "query", "recording", "--arguments", "[]"]).unwrap();
        assert!(request_for(&invalid.command).is_err());
        let list = Cli::try_parse_from(["titan", "queries"]).unwrap();
        assert!(matches!(
            request_for(&list.command).unwrap(),
            titan_protocol::Request::Queries
        ));
    }

    #[test]
    fn set_field_preserves_json_value_types_and_entity_identity() {
        for value in [
            "true",
            "false",
            "42",
            "-3.5",
            r#""hello""#,
            "null",
            "[1,2]",
            r#"{"nested":true}"#,
        ] {
            let cli = Cli::try_parse_from([
                "titan",
                "set-field",
                "7",
                "3",
                "Position",
                "x",
                "--value",
                value,
            ])
            .unwrap();
            assert_eq!(
                request_for(&cli.command).unwrap(),
                titan_protocol::Request::SetField {
                    entity: titan_protocol::EntityId {
                        index: 7,
                        generation: 3
                    },
                    component: "Position".into(),
                    field: "x".into(),
                    value: serde_json::from_str(value).unwrap(),
                }
            );
        }
        for value in ["", "{", "undefined", "NaN", "1 2"] {
            let cli = Cli::try_parse_from([
                "titan",
                "set-field",
                "7",
                "3",
                "Position",
                "x",
                "--value",
                value,
            ])
            .unwrap();
            assert_eq!(request_for(&cli.command).unwrap_err().0, "invalid_value");
        }
    }
}
