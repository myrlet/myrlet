//! Myrlet Worker daemon: outbound WSS client and Hub HTTP Progress/Finish reporter.

use std::process::ExitCode;
use std::time::Duration;

use myrlet_protocol::paths::MyrletHome;
use myrlet_protocol::WorkerId;
use myrlet_worker_daemon::credentials::{self, WorkerCredentials};
use myrlet_worker_daemon::{tls, wait_for_shutdown_signal, WorkerConfig, WorkerRuntime};
use tokio::sync::watch;

const USAGE: &str = "usage: worker [run|check-config|pair|credential-import] [options]";

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = args.get(1..).unwrap_or_default();
    let result = match args.first().map(String::as_str).unwrap_or("run") {
        "run" => run(options),
        "check-config" => check_config(),
        "pair" => pair(options),
        "credential-import" => import_credential(options),
        other => Err(format!("unknown subcommand {other:?}\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("worker: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Default command: run the Worker until `SIGTERM` / `SIGINT`, then drain and stop.
fn run(options: &[String]) -> Result<(), String> {
    if !options.is_empty() {
        return Err(format!("run takes no options\n{USAGE}"));
    }
    let home = owned(MyrletHome::resolve(None))?;
    let config = owned(WorkerConfig::load(&home.worker_config()))?;
    let timeout = Duration::from_millis(config.shutdown_timeout_ms());
    let credentials = owned(credentials::load_credentials(&home.worker_credentials()))?;
    let runtime = owned(WorkerRuntime::new(home, config, credentials))?;
    new_runtime()?.block_on(async move {
        let (sender, receiver) = watch::channel(false);
        let signal = async move {
            wait_for_shutdown_signal().await;
            let _ = sender.send(true);
        };
        let worker = runtime.run(receiver);
        tokio::pin!(signal);
        tokio::pin!(worker);
        tokio::select! {
            result = &mut worker => owned(result),
            () = &mut signal => match tokio::time::timeout(timeout, &mut worker).await {
                Ok(result) => owned(result),
                Err(_) => {
                    tracing::warn!("shutdown drain exceeded its bound; exiting");
                    Ok(())
                }
            },
        }
    })
}

/// Validate `worker.yaml` without touching credentials or the network.
fn check_config() -> Result<(), String> {
    let home = owned(MyrletHome::resolve(None))?;
    let config = owned(WorkerConfig::load(&home.worker_config()))?;
    let hub = &config.hub_url;
    println!(
        "worker: config OK ({} slots, hub {hub})",
        config.slots.len()
    );
    Ok(())
}
fn pair(options: &[String]) -> Result<(), String> {
    let code = option(options, "--code").ok_or("pair requires --code")?;
    let home = owned(MyrletHome::resolve(None))?;
    let hub_url = match option(options, "--hub-url") {
        Some(url) => url,
        None => owned(WorkerConfig::load(&home.worker_config()))?.hub_url,
    };
    let worker_id = option(options, "--worker-id").map(WorkerId::new);
    let ca_file = tls::ca_file_from_env();
    let issued = new_runtime()?.block_on(credentials::pair_with_hub(
        &hub_url,
        ca_file.as_deref(),
        &code,
        worker_id,
        &home.worker_credentials(),
    ));
    println!("worker: paired as {}", owned(issued)?.worker_id);
    Ok(())
}

/// Import an existing credential document from a file or standard input.
fn import_credential(options: &[String]) -> Result<(), String> {
    let json = match option(options, "--file") {
        Some(path) => std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?,
        None if options.iter().any(|argument| argument == "--stdin") => read_stdin()?,
        None => return Err("credential-import requires --file or --stdin".to_owned()),
    };
    let credentials: WorkerCredentials = serde_json::from_str(&json)
        .map_err(|error| format!("invalid credential document: {error}"))?;
    let home = owned(MyrletHome::resolve(None))?;
    owned(credentials::save_credentials(
        &home.worker_credentials(),
        &credentials,
    ))?;
    println!("worker: imported credential for {}", credentials.worker_id);
    Ok(())
}

fn read_stdin() -> Result<String, String> {
    use std::io::Read;
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text)
}

fn owned<T>(result: Result<T, impl std::fmt::Display>) -> Result<T, String> {
    result.map_err(|error| error.to_string())
}

fn new_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
}

fn option(options: &[String], name: &str) -> Option<String> {
    options
        .iter()
        .position(|argument| argument == name)
        .and_then(|index| options.get(index + 1))
        .cloned()
}
