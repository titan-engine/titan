//! Stdio entry point for the local Titan MCP sidecar.

use std::{
    env,
    io::{self, Write},
    process::ExitCode,
};
use titan_mcp::{client::Client, protocol};

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
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--url" => url = args.next().ok_or("--url requires a URL")?,
            "--help" | "-h" => {
                writeln!(io::stdout().lock(), "titan_mcp [--url http://127.0.0.1:15702]\nMCP over stdio for a local BRP game. TITAN_BRP_URL sets the default URL.").map_err(|e| e.to_string())?;
                return Ok(());
            }
            _ => return Err(format!("Unknown argument {arg}; use --help")),
        }
    }
    let client = Client::new(&url)?;
    protocol::serve(&client, io::stdin().lock(), io::stdout().lock()).map_err(|e| e.to_string())
}
