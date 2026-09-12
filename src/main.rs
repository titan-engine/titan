use std::{env, process::ExitCode};

const HELP: &str = "Titan

Usage: titan [--help | --version]

Options:
    --help       Show this help message
    --version    Show the version";

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let option = args.next();

    if let Some(extra) = args.next() {
        eprintln!("error: unexpected argument {extra:?}\nTry 'titan --help' for usage.");
        return ExitCode::FAILURE;
    }

    match option {
        None => println!("{HELP}"),
        Some(option) if option == "--help" => println!("{HELP}"),
        Some(option) if option == "--version" => println!("titan {}", env!("CARGO_PKG_VERSION")),
        Some(option) => {
            eprintln!("error: unsupported argument {option:?}\nTry 'titan --help' for usage.");
            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}
