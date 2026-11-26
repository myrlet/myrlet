//! Myrlet headless CLI for humans and Owner Agents.

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    myrlet_cli::run_with_io(
        &args,
        |key| std::env::var(key).ok(),
        &mut stdout,
        &mut stderr,
    )
    .await
}
