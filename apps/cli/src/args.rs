//! Hand-written argument parser (no external parser crate).

use std::collections::{BTreeMap, BTreeSet};

use myrlet_protocol::{TaskDefinition, TaskTreeDirection};

use crate::CliError;

mod tasks;

/// Default Hub base URL when `MYRLET_HUB_URL` is unset.
pub const DEFAULT_HUB_URL: &str = "http://127.0.0.1:4140";

const BOOLEAN_FLAGS: &[&str] = &["--json"];

/// Parsed command line: the global `--json` flag plus the selected command.
pub struct ParsedCli {
    pub json: bool,
    pub command: Command,
}

/// One CLI subcommand with its validated options.
///
/// Grouped by destination: Hub reads, Owner writes, and execution-local Worker commands.
pub enum Command {
    Help,
    TasksList {
        limit: Option<usize>,
    },
    TasksShow {
        id: String,
    },
    TasksTree {
        id: String,
        direction: TaskTreeDirection,
        max_depth: Option<u8>,
        limit: Option<usize>,
    },
    TasksSubmit {
        file: Option<String>,
        idempotency_key: Option<String>,
        definition: Option<Box<TaskDefinition>>,
    },
    TasksImport {
        file: String,
        idempotency_key: Option<String>,
    },
    TasksUpdate {
        id: String,
        expected_version: u64,
        idempotency_key: Option<String>,
        dependencies: Option<Vec<String>>,
    },
    TasksCancel {
        id: String,
        expected_version: Option<u64>,
        idempotency_key: Option<String>,
        reason: Option<String>,
    },
    TasksFinish {
        id: String,
        expected_version: Option<u64>,
        idempotency_key: Option<String>,
    },
    WorkersList,
    WorkersShow {
        id: String,
    },
    /// Operator bootstrap: issue a one-time pairing code.
    WorkersEnroll {
        ttl_seconds: Option<u64>,
        suggested_worker_id: Option<String>,
    },
    WorkersRotate {
        id: String,
    },
    WorkersRevoke {
        id: String,
    },
    WorkersApprove {
        id: String,
    },
    WorkersConfigShow {
        id: String,
    },
    WorkersConfigApply {
        id: String,
        revision: u64,
    },
    EventsWatch {
        cursor: Option<u64>,
        limit: Option<usize>,
    },
    Done {
        delivery_id: String,
    },
    ResultCheck {
        delivery_id: String,
    },
    ResultSync {
        delivery_id: String,
    },
    ResultStatus {
        delivery_id: String,
    },
}

fn usage(message: impl Into<String>) -> CliError {
    CliError::Usage(message.into())
}

/// Parse the arguments after the executable name.
pub fn parse_args(args: &[String]) -> Result<ParsedCli, CliError> {
    let mut json = false;
    let mut tokens = Vec::with_capacity(args.len());
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "--help" | "-h" => {
                return Ok(ParsedCli {
                    json,
                    command: Command::Help,
                })
            }
            _ => tokens.push(arg.clone()),
        }
    }
    let mut scanner = Scanner {
        tokens,
        position: 0,
    };
    Ok(ParsedCli {
        json,
        command: parse_command(&mut scanner)?,
    })
}

struct Scanner {
    tokens: Vec<String>,
    position: usize,
}

impl Scanner {
    fn take_token(&mut self) -> Option<String> {
        let token = self.tokens.get(self.position).cloned();
        if token.is_some() {
            self.position += 1;
        }
        token
    }

    fn value(&mut self, flag: &str) -> Result<String, CliError> {
        self.take_token()
            .ok_or_else(|| CliError::Usage(format!("{flag} requires a value")))
    }
}

#[derive(Default)]
struct RawArgs {
    positionals: Vec<String>,
    bools: BTreeSet<String>,
    values: BTreeMap<String, Vec<String>>,
}

impl RawArgs {
    fn parse(scanner: &mut Scanner) -> Result<Self, CliError> {
        let mut raw = Self::default();
        while let Some(token) = scanner.take_token() {
            if let Some(name) = token.strip_prefix("--") {
                let flag = format!("--{name}");
                if BOOLEAN_FLAGS.contains(&flag.as_str()) {
                    raw.bools.insert(flag);
                } else {
                    let value = scanner.value(&flag)?;
                    raw.values.entry(flag).or_default().push(value);
                }
            } else {
                raw.positionals.push(token);
            }
        }
        Ok(raw)
    }

    fn positional(&mut self, usage: &str) -> Result<String, CliError> {
        if self.positionals.is_empty() {
            return Err(CliError::Usage(format!("missing argument for {usage}")));
        }
        Ok(self.positionals.remove(0))
    }

    fn single(&mut self, flag: &str) -> Option<String> {
        self.values.remove(flag).and_then(|mut values| values.pop())
    }

    fn required(&mut self, flag: &str) -> Result<String, CliError> {
        self.single(flag)
            .ok_or_else(|| CliError::Usage(format!("missing required {flag}")))
    }

    fn list(&mut self, flag: &str) -> Vec<String> {
        self.values.remove(flag).unwrap_or_default()
    }

    fn number<T: std::str::FromStr>(&mut self, flag: &str) -> Result<Option<T>, CliError> {
        match self.single(flag) {
            None => Ok(None),
            Some(raw) => raw
                .parse()
                .map(Some)
                .map_err(|_| CliError::Usage(format!("{flag} expects a number, got {raw:?}"))),
        }
    }

    fn finish(&self, command: &str) -> Result<(), CliError> {
        if let Some(extra) = self.positionals.first() {
            return Err(CliError::Usage(format!(
                "unexpected argument {extra:?} for {command}"
            )));
        }
        if let Some(flag) = self
            .values
            .keys()
            .next()
            .or_else(|| self.bools.iter().next())
        {
            return Err(CliError::Usage(format!(
                "unexpected argument {flag:?} for {command}"
            )));
        }
        Ok(())
    }
}

fn parse_command(scanner: &mut Scanner) -> Result<Command, CliError> {
    match scanner.take_token().as_deref() {
        Some("tasks") => tasks::parse(scanner),
        Some("workers") => parse_workers(scanner),
        Some("events") => parse_events(scanner),
        Some("done") => {
            let mut raw = RawArgs::parse(scanner)?;
            let delivery_id = raw.positional("done <delivery-id>")?;
            raw.finish("done")?;
            Ok(Command::Done { delivery_id })
        }
        Some("result") => parse_result(scanner),
        Some(other) => Err(CliError::Usage(format!("unknown command {other:?}"))),
        None => Err(CliError::Usage("missing command".to_string())),
    }
}

fn parse_workers(scanner: &mut Scanner) -> Result<Command, CliError> {
    match scanner.take_token().as_deref() {
        Some("list") => {
            RawArgs::parse(scanner)?.finish("workers list")?;
            Ok(Command::WorkersList)
        }
        Some("show") => {
            let mut raw = RawArgs::parse(scanner)?;
            let id = raw.positional("workers show <id>")?;
            raw.finish("workers show")?;
            Ok(Command::WorkersShow { id })
        }
        Some("enroll") => {
            let mut raw = RawArgs::parse(scanner)?;
            let ttl_seconds = raw.number::<u64>("--ttl-seconds")?;
            let suggested_worker_id = raw.single("--suggested-worker-id");
            raw.finish("workers enroll")?;
            Ok(Command::WorkersEnroll {
                ttl_seconds,
                suggested_worker_id,
            })
        }
        Some("rotate") => {
            let mut raw = RawArgs::parse(scanner)?;
            let id = raw.positional("workers rotate <id>")?;
            raw.finish("workers rotate")?;
            Ok(Command::WorkersRotate { id })
        }
        Some("revoke") => {
            let mut raw = RawArgs::parse(scanner)?;
            let id = raw.positional("workers revoke <id>")?;
            raw.finish("workers revoke")?;
            Ok(Command::WorkersRevoke { id })
        }
        Some("approve") => {
            let mut raw = RawArgs::parse(scanner)?;
            let id = raw.positional("workers approve <id>")?;
            raw.finish("workers approve")?;
            Ok(Command::WorkersApprove { id })
        }
        Some("config") => parse_workers_config(scanner),
        Some(other) => Err(CliError::Usage(format!(
            "unknown workers command {other:?}"
        ))),
        None => Err(CliError::Usage("workers requires a command".to_string())),
    }
}

/// `workers config show <id>` or `workers config apply <id> --revision <n>`.
fn parse_workers_config(scanner: &mut Scanner) -> Result<Command, CliError> {
    match scanner.take_token().as_deref() {
        Some("show") => {
            let mut raw = RawArgs::parse(scanner)?;
            let id = raw.positional("workers config show <id>")?;
            raw.finish("workers config show")?;
            Ok(Command::WorkersConfigShow { id })
        }
        Some("apply") => {
            let mut raw = RawArgs::parse(scanner)?;
            let id = raw.positional("workers config apply <id>")?;
            let revision = raw
                .number::<u64>("--revision")?
                .ok_or_else(|| usage("workers config apply requires --revision"))?;
            raw.finish("workers config apply")?;
            Ok(Command::WorkersConfigApply { id, revision })
        }
        Some(other) => Err(CliError::Usage(format!(
            "unknown workers config command {other:?}"
        ))),
        None => Err(CliError::Usage(
            "workers config requires show or apply".to_string(),
        )),
    }
}

fn parse_events(scanner: &mut Scanner) -> Result<Command, CliError> {
    match scanner.take_token().as_deref() {
        Some("watch") => {
            let mut raw = RawArgs::parse(scanner)?;
            let cursor = raw.number::<u64>("--cursor")?;
            let limit = raw.number::<usize>("--limit")?;
            raw.finish("events watch")?;
            Ok(Command::EventsWatch { cursor, limit })
        }
        Some(other) => Err(CliError::Usage(format!("unknown events command {other:?}"))),
        None => Err(CliError::Usage("events requires a command".to_string())),
    }
}

/// `result check|sync|status <delivery-id>`; every operation names its Delivery.
fn parse_result(scanner: &mut Scanner) -> Result<Command, CliError> {
    let operation = scanner
        .take_token()
        .ok_or_else(|| usage("result requires check, sync, or status"))?;
    let mut raw = RawArgs::parse(scanner)?;
    let delivery_id = raw.positional("result <operation> <delivery-id>")?;
    raw.finish(&format!("result {operation}"))?;
    match operation.as_str() {
        "check" => Ok(Command::ResultCheck { delivery_id }),
        "sync" => Ok(Command::ResultSync { delivery_id }),
        "status" => Ok(Command::ResultStatus { delivery_id }),
        other => Err(usage(format!("unknown result command {other:?}"))),
    }
}
