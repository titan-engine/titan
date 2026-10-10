//! Stdio entry point for the local Titan MCP sidecar.

mod stdio;

use std::{
    env,
    io::{self, Write},
    process::ExitCode,
};
use std::{path::PathBuf, time::Duration};
use titan_mcp::{
    client::Client,
    process::{CommandSpec, ProcessConfig, ProcessManager},
    protocol,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(io::stderr().lock(), "titan_mcp: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut url = env::var("TITAN_BRP_URL")
        .unwrap_or_else(|_| format!("http://127.0.0.1:{}", bevy_remote::http::DEFAULT_PORT));
    let mut game = None;
    let mut build = None;
    let mut directory = None;
    let mut ready = Duration::from_secs(30);
    let mut stop = Duration::from_secs(3);
    let mut build_timeout = Duration::from_secs(300);
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--url" => url = args.next().ok_or("--url requires a URL")?,
            "--game-cmd" => {
                game = Some(CommandSpec::from_json(
                    &args.next().ok_or("--game-cmd requires JSON argv")?,
                )?);
            }
            "--build-cmd" => {
                build = Some(CommandSpec::from_json(
                    &args.next().ok_or("--build-cmd requires JSON argv")?,
                )?);
            }
            "--game-dir" => {
                directory = Some(PathBuf::from(
                    args.next().ok_or("--game-dir requires a directory")?,
                ));
            }
            "--ready-timeout-secs" | "--stop-timeout-secs" | "--build-timeout-secs" => {
                let seconds: u64 = args
                    .next()
                    .ok_or_else(|| format!("{arg} requires seconds"))?
                    .parse()
                    .map_err(|_| format!("{arg} requires an integer in 1..=3600"))?;
                if !(1..=3600).contains(&seconds) {
                    return Err(format!("{arg} requires seconds in 1..=3600"));
                }
                let duration = Duration::from_secs(seconds);
                match arg.as_str() {
                    "--ready-timeout-secs" => ready = duration,
                    "--stop-timeout-secs" => stop = duration,
                    _ => build_timeout = duration,
                }
            }
            "--help" | "-h" => {
                writeln!(io::stdout().lock(), "titan_mcp [--url http://127.0.0.1:15702]\nMCP over stdio for a local BRP game. TITAN_BRP_URL sets the default URL.\nOptional process ownership (commands are JSON argv arrays, not shell strings):\n  --game-cmd '[\"cargo\",\"run\",\"-p\",\"my_game\"]'\n  --build-cmd '[\"cargo\",\"build\",\"-p\",\"my_game\"]'\n  --game-dir /absolute/path/to/workspace\n  --ready-timeout-secs 30 --stop-timeout-secs 3 --build-timeout-secs 300\nNo game is launched until launch_game or restart_game is called.").map_err(|e| e.to_string())?;
                return Ok(());
            }
            _ => return Err(format!("Unknown argument {arg}; use --help")),
        }
    }
    let client = Client::new(&url)?;
    let mut manager = if let Some(game) = game {
        let mut config = ProcessConfig::new(game);
        config.build = build;
        config.directory = directory;
        config.ready_timeout = ready;
        config.stop_timeout = stop;
        config.build_timeout = build_timeout;
        ProcessManager::new(config)?
    } else {
        if build.is_some() || directory.is_some() {
            return Err("--build-cmd and --game-dir require --game-cmd".to_owned());
        }
        ProcessManager::attached()
    };
    let cancellation = manager.cancellation();
    ctrlc::set_handler(move || cancellation.request())
        .map_err(|e| format!("Installing shutdown handler: {e}"))?;
    let input = io::BufReader::new(stdio::StdinReader::new(manager.cancellation()));
    let output = io::BufWriter::new(stdio::StdoutWriter::new(manager.cancellation()));
    let result = protocol::serve_managed(&client, &mut manager, input, output);
    if manager.cancellation().is_requested() {
        Ok(())
    } else {
        result.map_err(|e| e.to_string())
    }
}
