//! Repo automation: `cargo xtask <command>`. Commands land with L-005.

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <command>\n\nNo commands are implemented yet.";

fn main() -> ExitCode {
    if let Some(command) = std::env::args().nth(1) {
        eprintln!("unknown command: {command}");
    }
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
