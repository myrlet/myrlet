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

Credentials:
  MYRLET_ADMIN_TOKEN           Admin: global reads, Task coordination, Worker/system admin
  MYRLET_HOME/worker/credentials.json
                               Worker or Owner: the Hub decides the tier. Without the Admin
                               token, Hub reads and Task coordination use this credential.

Hub reads (Owner or Admin; Worker only for its own Task and direct related Tasks):
  tasks list [--limit <n>]                 Bounded newest-first Task summaries
  tasks show <id>                          One Task with its Delivery and Result
  tasks tree <id> [--direction ancestors|descendants] [--max-depth <n>] [--limit <n>]
  workers list                             Enrolled Workers, approval, and revisions
  workers show <id>                        One Worker, approval, inventory, and claimable Tasks
  events watch [--cursor <seq>] [--limit <n>]

Task coordination (Owner or Admin):
  tasks submit --file <def.json> | --title <t> --prompt <p> [--idempotency-key <k>]
               [--parent-task-id <id>] [--depends-on <id>]... [--capability <name>]...
  tasks import --file <bundle.json> [--idempotency-key <k>]
  tasks update <id> --expected-version <n> [--depends-on <id>]... [--idempotency-key <k>]
  tasks cancel <id> [--expected-version <n>] [--reason <text>]
  tasks finish <id> [--expected-version <n>]   Coordination-only Task with no Delivery

Admin (Worker and system administration):
  workers enroll [--ttl-seconds <n>]       Issue a one-time pairing code
  workers rotate <id>                      Rotate a Worker credential
  workers revoke <id>                      Revoke a Worker credential
  workers approve <id>                     Admit a pending Worker to scheduling
  workers config show <id>                 Both scheduling snapshots under review
  workers config apply <id> --revision <n> Apply the reported scheduling revision
  workers permission set <id> --tier worker|owner
                                           Change the server-stored permission tier
  policy show                              Read automatic approval and apply policies
  policy set [--auto-approve <bool>] [--auto-apply <bool>]
                                           Enable/disable automatic approval or apply

Execution-local (no Hub credential; uses the local Worker control socket):
  done <delivery-id>                       Harness finished its work
  result check <delivery-id>               Advisory Markdown feedback
  result sync <delivery-id>                Request one Result reconciliation
  result status <delivery-id>              Compare local and Hub Result content

env: MYRLET_HUB_URL (default http://127.0.0.1:4140), MYRLET_ADMIN_TOKEN (Admin),
     MYRLET_HOME (Worker credential and execution-local commands)
";

pub async fn run_with_io<W: Write, E: Write, F: Fn(&str) -> Option<String>>(
    args: &[String],
    env: F,
    stdout: &mut W,
    stderr: &mut E,
) -> ExitCode {
    match dispatch(args, &env, stdout).await {
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
    env: &F,
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
    let admin_token = env("MYRLET_ADMIN_TOKEN").filter(|token| !token.is_empty());
    let home = env("MYRLET_HOME").filter(|home| !home.trim().is_empty());
    let worker_socket = home.as_deref().map(|home| {
        commands::control_socket(home)
            .to_string_lossy()
            .into_owned()
    });
    let environment = commands::Env {
        hub_url: &hub_url,
        admin_token: admin_token.as_deref(),
        home: home.as_deref(),
        worker_socket: worker_socket.as_deref(),
    };
    commands::execute(&environment, parsed.command, parsed.json, stdout).await
}
