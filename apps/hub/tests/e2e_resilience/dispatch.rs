//! Dispatch and restart scenarios: a queued Task waits for a compatible Worker, a restart
//! replays without a duplicate submission, and Slot capability selection is enforced.

use std::time::{Duration, Instant};

use crate::fixtures::{
    clear_trust_gates, create_task, finished, gpu_yaml, line_count, resync_yaml, show, status,
};
use crate::harness::{
    binaries, cli, enroll_and_pair, field, json_file_count, online_epoch, poll, run_worker,
    shutdown, spawn_daemon, start_hub, worker_json, worker_offline,
};

#[tokio::test]
async fn a_queued_task_waits_for_a_compatible_worker_to_claim_it() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    myrlet_protocol::ensure_private_dir(&home).expect("create home");
    let (mut hub_guard, hub_url) = start_hub(&hub_bin, &home).await;
    let cli = cli(&cli_bin, &home, &hub_url);
    let env_home = home.to_str().expect("utf-8 home");

    // Submission authorizes the Task; with no Worker online it simply stays queued.
    let task = create_task(&cli, "queue-create", "Queued GPU work", "gpu", "exec-gpu");
    assert_eq!(status(&show(&cli, &task)), "queued");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        status(&show(&cli, &task)),
        "queued",
        "a queued Task waits; no Worker means no claim"
    );
    assert!(
        show(&cli, &task)["currentDelivery"].is_null(),
        "no wakeup or queue admission creates a Delivery"
    );

    enroll_and_pair(&cli, &worker_bin, &home, &hub_url);
    std::fs::write(home.join("worker/worker.yaml"), gpu_yaml(&hub_url)).expect("write worker.yaml");
    let mut worker_guard = spawn_daemon(&worker_bin, &[("MYRLET_HOME", env_home)]);
    poll("worker online", Duration::from_secs(10), || {
        online_epoch(&cli)
    });
    clear_trust_gates(&cli);

    let shown = poll("the queued task to finish", Duration::from_secs(15), || {
        let shown = show(&cli, &task);
        (status(&shown) == "finished").then_some(shown)
    });
    let delivery = field(&shown, "currentDelivery").expect("current delivery");
    assert_eq!(
        delivery["progress"]["text"], "gpu queued ok",
        "progress: {delivery}"
    );
    assert!(
        delivery["progress"]["firstReceivedAtMs"]
            .as_i64()
            .unwrap_or(0)
            > 0
    );

    shutdown(&mut worker_guard.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}

#[tokio::test]
async fn worker_restart_resync_prevents_double_submit_and_replays_offline_cancel() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    myrlet_protocol::ensure_private_dir(&home).expect("create home");
    let counter = directory.path().join("counter.txt");
    let (mut hub_guard, hub_url) = start_hub(&hub_bin, &home).await;
    let cli = cli(&cli_bin, &home, &hub_url);
    let env_home = home.to_str().expect("utf-8 home");

    enroll_and_pair(&cli, &worker_bin, &home, &hub_url);
    let yaml = home.join("worker/worker.yaml");
    std::fs::write(&yaml, resync_yaml(&hub_url, &counter)).expect("write worker.yaml");

    let mut first = spawn_daemon(&worker_bin, &[("MYRLET_HOME", env_home)]);
    let epoch_one = poll("worker online", Duration::from_secs(10), || {
        online_epoch(&cli)
    });
    clear_trust_gates(&cli);

    let counted = create_task(&cli, "resync-a", "Count one run", "shell", "exec-count");
    poll("Task A to finish", Duration::from_secs(15), || {
        finished(&cli, &counted)
    });
    assert_eq!(line_count(&counter), 1, "Task A must run exactly once");

    let sleeping = create_task(&cli, "resync-b", "Sleep", "shell", "exec-sleep");
    poll("Task B to be claimed", Duration::from_secs(15), || {
        (status(&show(&cli, &sleeping)) == "dispatched").then_some(())
    });
    let deliveries = home.join("worker/state/deliveries");
    poll("Task B registered", Duration::from_secs(10), || {
        let registered = json_file_count(&deliveries) >= 2;
        registered.then_some(())
    });
    // Registration precedes adapter submit; wait for the running sleep to be durable
    // before killing the Worker, rather than racing its own start.
    tokio::time::sleep(Duration::from_millis(500)).await;

    first.0.kill().expect("kill worker instance 1");
    first.0.wait().expect("wait worker instance 1");
    poll("worker offline", Duration::from_secs(15), || {
        worker_offline(&cli).then_some(())
    });
    // The killed Worker cannot flush a Finish queued just as it died; resync/retry at the
    // new epoch must carry the outcome without rerunning the completed Task A.
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let cancel_args = [
        "--json",
        "tasks",
        "cancel",
        &sleeping,
        "--reason",
        "offline cancel",
    ];
    let cancelled = cli.run(&cancel_args);
    assert_eq!(
        status(&cancelled),
        "cancelled",
        "offline cancel: {cancelled}"
    );

    let mut second = spawn_daemon(
        &worker_bin,
        &[
            ("MYRLET_HOME", env_home),
            ("MYRLET_WORKER_SHUTDOWN_TIMEOUT_MS", "1500"),
        ],
    );
    poll("worker online again", Duration::from_secs(15), || {
        online_epoch(&cli).filter(|epoch| *epoch > epoch_one)
    });
    // Rotation and restart preserve approval; only a changed inventory needs an Apply.
    let current = worker_json(&cli);
    if current["reportedRevision"] != current["appliedRevision"] {
        clear_trust_gates(&cli);
    }
    poll("cancel acknowledged", Duration::from_secs(10), || {
        let shown = show(&cli, &sleeping);
        let delivery = field(&shown, "currentDelivery")?;
        (!delivery["cancelState"].is_null()).then_some(())
    });
    assert_eq!(line_count(&counter), 1, "Task A must not run twice");

    shutdown(&mut second.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}

#[tokio::test]
async fn worker_uses_second_slot_when_only_it_meets_capability() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    myrlet_protocol::ensure_private_dir(&home).unwrap();
    let (_hub, url) = start_hub(&hub_bin, &home).await;
    let cli = cli(&cli_bin, &home, &url);
    enroll_and_pair(&cli, &worker_bin, &home, &url);
    let cpu = "  - slotId: cpu-first\n    capabilities: [{name: cpu, state: available}]\n    executors: [{id: exec-cpu, kind: process, program: /usr/bin/true, args: []}]\n";
    let gpu_config = gpu_yaml(&url);
    let config = gpu_config.replace("slots:\n", &format!("slots:\n{cpu}"));
    std::fs::write(home.join("worker/worker.yaml"), config).unwrap();
    run_worker(&worker_bin, &home, &["check-config"]);
    let _worker = spawn_daemon(&worker_bin, &[("MYRLET_HOME", home.to_str().unwrap())]);
    poll("two-slot worker online", Duration::from_secs(4), || {
        online_epoch(&cli)
    });
    clear_trust_gates(&cli);
    let id = create_task(&cli, "two-slots", "test", "gpu", "exec-gpu");
    let limit = Instant::now() + Duration::from_secs(4);
    loop {
        let task = show(&cli, &id);
        if let Some(slot) = task["currentDelivery"]["slotId"].as_str() {
            assert_eq!(slot, "gpu-slot");
            break;
        }
        assert!(
            Instant::now() < limit,
            "online two-slot Worker with eligible second slot: Task {}, Delivery {}",
            status(&task),
            task["currentDelivery"]
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
