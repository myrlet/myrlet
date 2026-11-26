//! Myrlet CLI library: argument parsing, command execution, and exit codes.

use std::{io::Write, process::ExitCode};

pub mod args;
pub mod commands;
pub mod output;
/// A CLI failure, mapped to a stable process exit code.
#[derive(Debug)]
pub enum CliError {
    Usage(String),
    Runtime(String),
}

impl CliError {
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::Usage(_) => 2,
            CliError::Runtime(_) => 1,
        }
    }
}
const HELP: &str = "\
myrlet - Myrlet Hub CLI

USAGE: myrlet [--json] <group> <command> [options]
tasks: list | show | tree | create | dispatch | submit | bundle-import | cancel | finish
workers: list | show | enroll | rotate | revoke
events: watch [--cursor <seq>] [--limit <n>]
env: MYRLET_HUB_URL (default http://127.0.0.1:4140), MYRLET_ADMIN_TOKEN (required)
";

pub async fn run_with_io<W: Write, E: Write, F: Fn(&str) -> Option<String>>(
    args: &[String],
    env: F,
    stdout: &mut W,
    stderr: &mut E,
) -> ExitCode {
    match dispatch(args, env, stdout).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let message = match &error {
                CliError::Usage(message) | CliError::Runtime(message) => message,
            };
            let _ = writeln!(stderr, "error: {message}");
            ExitCode::from(error.exit_code())
        }
    }
}

async fn dispatch<W: Write, F: Fn(&str) -> Option<String>>(
    args: &[String],
    env: F,
    stdout: &mut W,
) -> Result<(), CliError> {
    let parsed = args::parse_args(args)?;
    if matches!(&parsed.command, args::Command::Help) {
        return stdout
            .write_all(HELP.as_bytes())
            .map_err(|error| CliError::Runtime(error.to_string()));
    }
    let hub_url = env("MYRLET_HUB_URL")
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| args::DEFAULT_HUB_URL.to_string());
    let token = env("MYRLET_ADMIN_TOKEN")
        .filter(|token| !token.is_empty())
        .ok_or_else(|| CliError::Usage("MYRLET_ADMIN_TOKEN is not set".to_string()))?;
    let client = myrlet_client::Client::new(hub_url, token)
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    commands::execute(&client, parsed.command, parsed.json, stdout).await
}
