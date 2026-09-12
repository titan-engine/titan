use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    process::{self, Command, Stdio},
};

const HELP: &str = "Titan

Usage:
    titan [--help | --version]
    titan --project <directory> --help
    titan --project <directory> --version
    titan [--project <directory>] game [<game-arguments>...]
    titan [--project <directory>] edit [<editor-arguments>...]

Commands:
    game       Build and run the project's game tooling command
    edit       Build and run the project's editor tooling

Options:
    --project <directory>  Select the project directory (before game or edit)
    --help                 Show this help message
    --version              Show the version

Arguments after game or edit are passed to the project tool unchanged.";

const USAGE_HINT: &str = "Try 'titan --help' for usage.";

enum Invocation {
    Help,
    Version,
    Launch {
        project: Option<OsString>,
        mode: ToolMode,
        arguments: Vec<OsString>,
    },
}

#[derive(Clone, Copy)]
enum ToolMode {
    Game,
    Edit,
}

impl ToolMode {
    fn protocol_name(self) -> &'static str {
        match self {
            Self::Game => "command",
            Self::Edit => "editor",
        }
    }
}

fn main() {
    process::exit(run(env::args_os().skip(1)));
}

fn run<I>(args: I) -> i32
where
    I: Iterator<Item = OsString>,
{
    let invocation = match parse_args(args) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };

    match invocation {
        Invocation::Help => {
            println!("{HELP}");
            0
        }
        Invocation::Version => {
            println!("titan {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Invocation::Launch {
            project,
            mode,
            arguments,
        } => launch(project, mode, arguments),
    }
}

fn parse_args<I>(mut args: I) -> Result<Invocation, String>
where
    I: Iterator<Item = OsString>,
{
    let first = match args.next() {
        Some(first) => first,
        None => return Ok(Invocation::Help),
    };

    let (project, command) = if first == "--project" {
        let project = args
            .next()
            .ok_or_else(|| usage_error("--project requires a directory."))?;
        if project.is_empty() {
            return Err(usage_error("--project requires a non-empty directory."));
        }
        let command = args
            .next()
            .ok_or_else(|| usage_error("expected 'game' or 'edit' after --project <directory>."))?;
        (Some(project), command)
    } else {
        (None, first)
    };

    if command == "--help" {
        return match args.next() {
            Some(extra) => Err(unexpected_argument(&extra)),
            None => Ok(Invocation::Help),
        };
    }
    if command == "--version" {
        return match args.next() {
            Some(extra) => Err(unexpected_argument(&extra)),
            None => Ok(Invocation::Version),
        };
    }
    if command == "--project" {
        return Err(usage_error(
            "--project may be specified only once, before game or edit.",
        ));
    }

    let mode = if command == "game" {
        ToolMode::Game
    } else if command == "edit" {
        ToolMode::Edit
    } else {
        return Err(format!(
            "error: unsupported argument {command:?}\n{USAGE_HINT}"
        ));
    };

    Ok(Invocation::Launch {
        project,
        mode,
        arguments: args.collect(),
    })
}

fn unexpected_argument(argument: &OsStr) -> String {
    format!("error: unexpected argument {argument:?}\n{USAGE_HINT}")
}

fn usage_error(message: &str) -> String {
    format!("error: {message}\n{USAGE_HINT}")
}

fn launch(project: Option<OsString>, mode: ToolMode, arguments: Vec<OsString>) -> i32 {
    let current_directory = match env::current_dir() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("error: cannot determine the current directory: {error}");
            return 1;
        }
    };

    let project_directory = match project {
        Some(project) => current_directory.join(project),
        None => current_directory,
    };

    let project_metadata = match fs::metadata(&project_directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("error: project directory {project_directory:?} does not exist.");
            return 1;
        }
        Err(error) => {
            eprintln!("error: cannot access project directory {project_directory:?}: {error}");
            return 1;
        }
    };
    if !project_metadata.is_dir() {
        eprintln!("error: project path {project_directory:?} is not a directory.");
        return 1;
    }

    let manifest = project_directory.join("Cargo.toml");
    let manifest_metadata = match fs::metadata(&manifest) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("error: project {project_directory:?} has no Cargo.toml manifest.");
            return 1;
        }
        Err(error) => {
            eprintln!("error: cannot access project manifest {manifest:?}: {error}");
            return 1;
        }
    };
    if !manifest_metadata.is_file() {
        eprintln!("error: project manifest {manifest:?} is not a file.");
        return 1;
    }

    let mut cargo = Command::new("cargo");
    cargo
        .current_dir(&project_directory)
        .arg("run")
        .arg("--quiet")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--bin")
        .arg("titan-tools")
        .arg("--")
        .arg("--titan-protocol")
        .arg("1")
        .arg(mode.protocol_name())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    cargo.args(arguments);

    match cargo.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("error: could not launch Cargo in project {project_directory:?}: {error}");
            1
        }
    }
}
