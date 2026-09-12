use titan_runtime::{Application, WindowConfig};

fn main() -> std::process::ExitCode {
    let configuration = WindowConfig::new("Titan native window", 720.0, 480.0);
    let mut application = match Application::new(configuration) {
        Ok(application) => application,
        Err(error) => {
            eprintln!("could not start Titan native window: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    println!("Titan native window ready");
    let result = application.run();
    drop(application);
    match result {
        Ok(reason) => {
            println!("Titan native window exited: {reason}; shutdown complete");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Titan native window stopped with an error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
