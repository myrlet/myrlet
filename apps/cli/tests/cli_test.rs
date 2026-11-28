//! End-to-end CLI behavior against an in-process axum Hub stub.

use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

#[derive(Clone, Default)]
struct Hub {
    log: Arc<Mutex<Vec<Recorded>>>,
}

#[derive(Clone)]
struct Recorded {
    method: String,
    path: String,
    key: Option<String>,
    authorization: Option<String>,
    body: Value,
}

impl Hub {
    fn push(&self, request: Recorded) {
        self.log.lock().unwrap().push(request);
    }
    fn all(&self) -> Vec<Recorded> {
        self.log.lock().unwrap().clone()
    }
}
async fn capture(
    State(hub): State<Hub>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let (parts, body) = request.into_parts();
    let header = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let key = header("idempotency-key");
    let authorization = header("authorization");
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_or_default();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    hub.push(Recorded {
        method: parts.method.to_string(),
        path: parts.uri.path().to_string(),
        key,
        authorization,
        body,
    });
    next.run(axum::extract::Request::from_parts(parts, Body::from(bytes)))
        .await
}

fn router(hub: Hub) -> Router {
    Router::new()
        .route("/v1/snapshot", get(|| async { Json(snapshot_value()) }))
        .route(
            "/v1/tasks",
            get(|| async { Json(task_list()) }).post(|| async {
                Json(json!({ "task": task("queued", 1, Value::Null), "eligible": true }))
            }),
        )
        .route(
            "/v1/tasks/:id",
            get(
                |axum::extract::Path(id): axum::extract::Path<String>| async move {
                    if id == "task_related" {
                        Json(summary_only_read(&id, 7))
                    } else {
                        Json(task_read("dispatched", 3, delivery()))
                    }
                },
            )
            .post(|| async { Json(task("queued", 4, Value::Null)) }),
        )
        .route(
            "/v1/tasks/:id/cancel",
            post(|| async { Json(task("cancelled", 5, Value::Null)) }),
        )
        .route(
            "/v1/tasks/:id/finish",
            post(|| async { Json(task("finished", 6, Value::Null)) }),
        )
        .route(
            "/v1/task-bundles/import",
            post(|| async {
                Json(json!({
                    "clientRefToTaskId": { "a": "task_01" },
                    "tasks": [task("queued", 1, Value::Null)],
                    "eligibleTaskIds": ["task_01"],
                }))
            }),
        )
        .route(
            "/v1/workers",
            get(|| async { Json(json!({ "workers": [worker_value()] })) }),
        )
        .route(
            "/v1/workers/:id",
            get(|| async {
                Json(json!({
                    "worker": worker_value(),
                    "reportedConfig": null,
                    "claimableTaskIds": ["task_02"],
                }))
            }),
        )
        .route(
            "/v1/workers/:id/approve",
            post(|| async { StatusCode::NO_CONTENT }),
        )
        .route(
            "/v1/workers/:id/config/apply",
            post(|| async { Json(json!({ "appliedRevision": 1 })) }),
        )
        .route(
            "/v1/settings/security-trust",
            get(|| async {
                Json(json!({
                    "requireWorkerApproval": true,
                    "requireConfigurationApply": false,
                }))
            })
            .patch(|Json(body): Json<Value>| async move {
                let stored = json!({
                    "requireWorkerApproval": body
                        .get("requireWorkerApproval")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    "requireConfigurationApply": body
                        .get("requireConfigurationApply")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
                Json(stored)
            }),
        )
        .route(
            "/v1/workers/:id/permission-tier",
            axum::routing::patch(|| async {
                Json(json!({ "workerId": "wkr_000000000000", "permissionTier": "owner" }))
            }),
        )
        .route("/v1/events", get(events_route))
        .layer(axum::middleware::from_fn_with_state(hub, capture))
}

async fn start(hub: Hub) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router(hub)).await.unwrap() });
    format!("http://{address}")
}

async fn run(args: &[&str], url: Option<&str>, token: Option<&str>) -> (ExitCode, String, String) {
    let mut env = HashMap::new();
    env.extend(url.map(|u| ("MYRLET_HUB_URL".to_string(), u.to_string())));
    env.extend(token.map(|t| ("MYRLET_ADMIN_TOKEN".to_string(), t.to_string())));
    let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code =
        myrlet_cli::run_with_io(&args, |k| env.get(k).cloned(), &mut stdout, &mut stderr).await;
    (
        code,
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

async fn ok(args: &[&str], url: &str) -> (String, String) {
    let (code, out, err) = run(args, Some(url), Some("admin-token")).await;
    assert_eq!(code, ExitCode::SUCCESS, "stderr: {err}");
    (out, err)
}

fn temp_json(name: &str, contents: &str) -> String {
    let path = std::env::temp_dir().join(format!("myrlet-cli-{name}-{}.json", std::process::id()));
    std::fs::write(&path, contents).unwrap();
    path.to_string_lossy().into_owned()
}

fn delivery() -> Value {
    json!({
        "id":"delivery_01","taskId":"task_01","workerId":"wkr_000000000000","slotId":"slot-a",
        "executorId":null,"connectionEpoch":7,"state":"accepted",
        "cancelState":null,"cancelReason":null,"createdAtMs":30,"updatedAtMs":40,
        "progress":{ "deliveryId":"delivery_01","taskId":"task_01","text":"hello progress",
            "observedAtMs":41,"receivedAtMs":42,"firstReceivedAtMs":42 }
    })
}

fn task(status: &str, version: u64, current: Value) -> Value {
    json!({
        "id":"task_01","rootTaskId":"task_01","depth":1,"status":status,"version":version,
        "createdAtMs":10,"updatedAtMs":20,"finishedAtMs":null,
        "task":{ "title":"Verify the build","prompt":"Run the test suite." },
        "currentDelivery":current,
        "blockedBy":[],
        "resultMarkdown":"# Findings\n\n- one","resultRecordedAtMs":45,
        "subtree":{"total":1,"held":1,"queued":0,"dispatched":0,"finished":0,"cancelled":0}
    })
}

fn task_read(status: &str, version: u64, current: Value) -> Value {
    json!({ "task": task(status, version, current), "summary": null, "related": [] })
}

fn task_list() -> Value {
    json!({
        "tasks":[{
            "id":"task_01","parentTaskId":null,"rootTaskId":"task_01","depth":1,
            "sourceTaskId":null,"dependencies":[],"blockedBy":[],"category":"general",
            "title":"Verify the build","status":"held","version":1,"createdAtMs":10,
            "updatedAtMs":20,"finishedAtMs":null,"currentDelivery":null,
            "subtree":{"total":1,"held":1,"queued":0,"dispatched":0,"finished":0,"cancelled":0}
        }],
        "totalTasks":1,"truncated":false,"eventCursor":41
    })
}

fn snapshot_value() -> Value {
    json!({
        "schemaVersion":"myrlet.snapshot/v1","generatedAtMs":100,"eventCursor":41,
        "tasksTruncated":false,"totalTasks":1,"workers":[],
        "tasks":[]
    })
}

fn worker_value() -> Value {
    json!({
        "workerId":"wkr_000000000000","online":true,"revoked":false,"approval":"approved",
        "permissionTier":"worker","connectionEpoch":7,"lastSeenAtMs":null,
        "activeDeliveries":0,"host":null,"reportedRevision":1,"appliedRevision":1,
        "appliedConfig":null
    })
}

async fn events_route() -> Response {
    let first = json!({"seq":42,"kind":"task.created","createdAtMs":1,"payload":{}});
    let second = json!({"seq":43,"kind":"delivery.progress","createdAtMs":2,"payload":{}});
    let body = format!("data: {first}\n\ndata: {second}\n\n");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .body(Body::from(body.into_bytes()))
        .unwrap()
}

#[tokio::test]
async fn json_flag_anywhere_and_usage_errors() {
    let hub = Hub::default();
    let url = start(hub.clone()).await;
    let (before, _) = ok(&["--json", "tasks", "list"], &url).await;
    let before: Value = serde_json::from_str(&before).unwrap();
    assert_eq!(before["totalTasks"], 1);
    assert_eq!(before["eventCursor"], 41);
    let (after, _) = ok(&["tasks", "list", "--json"], &url).await;
    let after: Value = serde_json::from_str(&after).unwrap();
    assert_eq!(after["tasks"][0]["id"], "task_01");
    let (code, out, _) = run(&["tasks", "list"], Some(url.as_str()), None).await;
    assert_eq!(code, ExitCode::from(2u8));
    assert!(out.is_empty());
    let (code, _, err) = run(&["bogus"], Some(url.as_str()), Some("admin-token")).await;
    assert_eq!(code, ExitCode::from(2u8));
    assert!(err.contains("unknown command"), "{err}");
    // Execution-local commands do not need the Admin token, only `MYRLET_HOME`.
    let (code, _, err) = run(&["done", "delivery_01"], Some(url.as_str()), None).await;
    assert_eq!(code, ExitCode::from(2u8));
    assert!(err.contains("MYRLET_HOME"), "{err}");
}

#[tokio::test]
async fn submit_show_and_update() {
    let hub = Hub::default();
    let url = start(hub.clone()).await;
    let (human, _) = ok(
        &[
            "tasks",
            "submit",
            "--title",
            "T",
            "--prompt",
            "P",
            "--idempotency-key",
            "k1",
        ],
        &url,
    )
    .await;
    assert!(human.contains("Status: queued"), "{human}");
    let create = hub.all().pop().unwrap();
    assert_eq!(create.method, "POST");
    assert_eq!(create.path, "/v1/tasks");
    assert_eq!(create.key.as_deref(), Some("k1"));
    assert_eq!(create.body["title"], "T");

    let (human, _) = ok(&["tasks", "show", "task_01"], &url).await;
    assert!(human.contains("Root: task_01"), "{human}");
    assert!(human.contains("FirstReceivedAtMs: 42"), "{human}");
    assert!(human.contains("hello progress"), "{human}");
    assert!(human.contains("Result:"), "{human}");
    assert!(human.contains("  # Findings"), "{human}");
    let (json_out, _) = ok(&["--json", "tasks", "show", "task_01"], &url).await;
    let show: Value = serde_json::from_str(&json_out).unwrap();
    assert_eq!(
        show["task"]["currentDelivery"]["progress"]["text"],
        "hello progress"
    );
    assert_eq!(show["task"]["resultMarkdown"], "# Findings\n\n- one");
    assert_eq!(show["task"]["resultRecordedAtMs"], 45);

    let _ = ok(
        &[
            "tasks",
            "update",
            "task_01",
            "--expected-version",
            "3",
            "--depends-on",
            "task_a",
            "--idempotency-key",
            "u1",
        ],
        &url,
    )
    .await;
    let update = hub.all().pop().unwrap();
    assert_eq!(update.method, "POST");
    assert_eq!(update.path, "/v1/tasks/task_01");
    assert_eq!(update.body["expectedTaskVersion"], 3);
    assert_eq!(update.body["dependencies"], json!(["task_a"]));

    let file = temp_json("submit-file", r#"{"title":"From file","prompt":"Do it"}"#);
    let _ = ok(
        &[
            "tasks",
            "submit",
            "--file",
            file.as_str(),
            "--idempotency-key",
            "s1",
        ],
        &url,
    )
    .await;
    let from_file = hub.all().pop().unwrap();
    assert_eq!(from_file.body["title"], "From file");
}

#[tokio::test]
async fn cancel_and_finish_fetch_the_current_version_when_omitted() {
    let hub = Hub::default();
    let url = start(hub.clone()).await;
    let _ = ok(
        &["tasks", "cancel", "task_01", "--reason", "no longer needed"],
        &url,
    )
    .await;
    let requests = hub.all();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/v1/tasks/task_01");
    assert_eq!(requests[1].path, "/v1/tasks/task_01/cancel");
    assert_eq!(requests[1].body["expectedTaskVersion"], 3);
    assert_eq!(requests[1].body["reason"], "no longer needed");
    assert!(
        requests[1]
            .key
            .as_deref()
            .is_some_and(|key| key.starts_with("myrlet-")),
        "a generated key is sent"
    );

    let _ = ok(
        &["tasks", "finish", "task_01", "--idempotency-key", "f1"],
        &url,
    )
    .await;
    let requests = hub.all();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[3].path, "/v1/tasks/task_01/finish");
    assert_eq!(requests[3].body["expectedTaskVersion"], 3);
    assert_eq!(requests[3].key.as_deref(), Some("f1"));
}

#[tokio::test]
async fn list_import_workers_show_and_events_watch() {
    let hub = Hub::default();
    let url = start(hub.clone()).await;

    let (list, _) = ok(&["tasks", "list", "--json"], &url).await;
    let list: Value = serde_json::from_str(&list).unwrap();
    assert_eq!(list["tasks"].as_array().unwrap().len(), 1);

    let file = temp_json("bundle", r#"{"tasks":[{"title":"A","prompt":"P"}]}"#);
    let (bundle_out, _) = ok(
        &[
            "tasks",
            "import",
            "--file",
            file.as_str(),
            "--idempotency-key",
            "b1",
        ],
        &url,
    )
    .await;
    assert!(bundle_out.contains("Imported 1 task"), "{bundle_out}");
    assert!(bundle_out.contains("Eligible now: task_01"), "{bundle_out}");
    let import = hub.all().pop().unwrap();
    assert_eq!(import.path, "/v1/task-bundles/import");

    let (workers, _) = ok(&["--json", "workers", "list"], &url).await;
    let workers: Value = serde_json::from_str(&workers).unwrap();
    assert_eq!(workers["workers"][0]["workerId"], "wkr_000000000000");

    let (detail, _) = ok(&["workers", "show", "wkr_000000000000"], &url).await;
    assert!(detail.contains("Approval: approved"), "{detail}");
    assert!(detail.contains("ClaimableTasks: task_02"), "{detail}");
    let (config, _) = ok(&["workers", "config", "show", "wkr_000000000000"], &url).await;
    assert!(config.contains("ReportedRevision: 1"), "{config}");
    assert!(config.contains("ApplyPending: false"), "{config}");

    let _ = ok(&["workers", "approve", "wkr_000000000000"], &url).await;
    let approve = hub.all().pop().unwrap();
    assert_eq!(approve.method, "POST");
    assert_eq!(approve.path, "/v1/workers/wkr_000000000000/approve");

    let (applied, _) = ok(
        &[
            "--json",
            "workers",
            "config",
            "apply",
            "wkr_000000000000",
            "--revision",
            "1",
        ],
        &url,
    )
    .await;
    let applied: Value = serde_json::from_str(&applied).unwrap();
    assert_eq!(applied["workerId"], "wkr_000000000000");
    assert_eq!(applied["appliedRevision"], 1);
    let apply = hub.all().pop().unwrap();
    assert_eq!(apply.method, "POST");
    assert_eq!(apply.path, "/v1/workers/wkr_000000000000/config/apply");
    assert_eq!(apply.body["revision"], 1);

    let (code, _, err) = run(
        &["workers", "config", "apply", "wkr_000000000000"],
        Some(url.as_str()),
        Some("admin-token"),
    )
    .await;
    assert_eq!(code, ExitCode::from(2u8));
    assert!(err.contains("--revision"), "{err}");

    let (policies, _) = ok(&["--json", "policy", "show"], &url).await;
    let policies: Value = serde_json::from_str(&policies).unwrap();
    assert_eq!(policies["requireWorkerApproval"], true);
    assert_eq!(policies["requireConfigurationApply"], false);

    let (changed, _) = ok(&["--json", "policy", "set", "--auto-approve", "true"], &url).await;
    let changed: Value = serde_json::from_str(&changed).unwrap();
    assert_eq!(changed["requireWorkerApproval"], false);
    let patch = hub.all().pop().unwrap();
    assert_eq!(patch.method, "PATCH");
    assert_eq!(patch.path, "/v1/settings/security-trust");
    assert_eq!(patch.body["requireWorkerApproval"], false);
    // The untouched policy is never set to a value: absent or null both leave it stored.
    assert!(patch
        .body
        .get("requireConfigurationApply")
        .is_none_or(Value::is_null));

    let (code, _, err) = run(&["policy", "set"], Some(url.as_str()), Some("admin-token")).await;
    assert_eq!(code, ExitCode::from(2u8));
    assert!(err.contains("--auto-approve"), "{err}");

    let (events, _) = ok(&["events", "watch", "--limit", "1", "--json"], &url).await;
    let lines: Vec<&str> = events.lines().collect();
    assert_eq!(lines.len(), 2, "{events}");
    let snapshot: Value = serde_json::from_str(lines[0]).unwrap();
    let event: Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(snapshot["type"], "snapshot");
    assert_eq!(event["type"], "event");
    assert_eq!(event["event"]["seq"], 42);
}

#[tokio::test]
async fn execution_local_commands_require_a_worker_socket() {
    let url = "http://127.0.0.1:1";
    for args in [
        vec!["result", "check", "delivery_01"],
        vec!["result", "sync", "delivery_01"],
        vec!["result", "status", "delivery_01"],
    ] {
        let (code, _, err) = run(&args, Some(url), Some("admin-token")).await;
        assert_eq!(code, ExitCode::from(2u8), "result {args:?}");
        assert!(err.contains("MYRLET_HOME"), "result {args:?}: {err}");
    }
}

/// Run one command against a fixed home directory, so a Worker credential is available.
async fn run_with_home(
    args: &[&str],
    url: &str,
    admin_token: Option<&str>,
    home: &str,
) -> (ExitCode, String, String) {
    let mut env = HashMap::new();
    env.insert("MYRLET_HUB_URL".to_string(), url.to_string());
    env.insert("MYRLET_HOME".to_string(), home.to_string());
    env.extend(admin_token.map(|t| ("MYRLET_ADMIN_TOKEN".to_string(), t.to_string())));
    let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code =
        myrlet_cli::run_with_io(&args, |k| env.get(k).cloned(), &mut stdout, &mut stderr).await;
    (
        code,
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

/// Write a Worker credential document under a private temp home.
fn worker_home(credential: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let home = std::env::temp_dir().join(format!(
        "myrlet-cli-home-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    let worker_dir = home.join("worker");
    std::fs::create_dir_all(&worker_dir).unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&worker_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let document = json!({
        "hubId": "hub_test",
        "workerId": "wkr_000000000000",
        "credential": credential,
    })
    .to_string();
    let path = worker_dir.join("credentials.json");
    std::fs::write(&path, document).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    home.to_string_lossy().into_owned()
}

#[tokio::test]
async fn worker_tier_commands_use_the_worker_credential_and_admin_commands_do_not() {
    let hub = Hub::default();
    let url = start(hub.clone()).await;
    let home = worker_home("worker-bearer");

    // A read/coordination command with no Admin token falls back to the Worker credential.
    let (code, _, err) = run_with_home(&["tasks", "list"], &url, None, &home).await;
    assert_eq!(code, ExitCode::SUCCESS, "{err}");
    assert_eq!(
        hub.all().pop().unwrap().authorization.as_deref(),
        Some("Bearer worker-bearer")
    );
    let (code, _, err) = run_with_home(&["workers", "list"], &url, None, &home).await;
    assert_eq!(code, ExitCode::SUCCESS, "{err}");
    assert_eq!(
        hub.all().pop().unwrap().authorization.as_deref(),
        Some("Bearer worker-bearer")
    );

    // An Admin-only command never downgrades to the Worker credential.
    let (code, _, err) = run_with_home(
        &[
            "workers",
            "permission",
            "set",
            "wkr_000000000000",
            "--tier",
            "owner",
        ],
        &url,
        None,
        &home,
    )
    .await;
    assert_eq!(code, ExitCode::from(2u8));
    assert!(err.contains("MYRLET_ADMIN_TOKEN"), "{err}");

    // With the Admin token present the same command is allowed.
    let (code, out, err) = run_with_home(
        &[
            "--json",
            "workers",
            "permission",
            "set",
            "wkr_000000000000",
            "--tier",
            "owner",
        ],
        &url,
        Some("admin-token"),
        &home,
    )
    .await;
    assert_eq!(code, ExitCode::SUCCESS, "{err}");
    let response: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(response["permissionTier"], "owner");
    let recorded = hub.all().pop().unwrap();
    assert_eq!(recorded.method, "PATCH");
    assert_eq!(
        recorded.path,
        "/v1/workers/wkr_000000000000/permission-tier"
    );
    assert_eq!(
        recorded.authorization.as_deref(),
        Some("Bearer admin-token")
    );
    assert_eq!(recorded.body["permissionTier"], "owner");
}

#[tokio::test]
async fn worker_commands_fail_closed_without_any_credential() {
    let url = start(Hub::default()).await;
    let (code, _, err) = run(&["tasks", "list"], Some(&url), None).await;
    assert_eq!(code, ExitCode::from(2u8));
    assert!(err.contains("MYRLET_HOME"), "{err}");
    // A missing credential file is a runtime failure, never a silent anonymous request.
    let home = std::env::temp_dir().join(format!("myrlet-cli-empty-{}", std::process::id()));
    std::fs::create_dir_all(home.join("worker")).unwrap();
    let (code, _, err) =
        run_with_home(&["tasks", "list"], &url, None, home.to_str().unwrap()).await;
    assert_eq!(code, ExitCode::from(1u8));
    assert!(err.contains("credential"), "{err}");
}

fn summary_only_read(id: &str, version: u64) -> Value {
    json!({
        "task": null,
        "summary": {
            "id": id,
            "parentTaskId": null,
            "depth": 1,
            "dependencies": [],
            "blockedBy": [],
            "title": "Related work",
            "status": "queued",
            "version": version,
        },
        "related": []
    })
}

#[tokio::test]
async fn a_worker_coordination_command_reads_the_summary_version() {
    // A Worker may read a directly related Task only as a summary; a coordination command
    // that omits `--expected-version` must still reach the Hub and surface its 403.
    let hub = Hub::default();
    let url = start(hub.clone()).await;
    let home = worker_home("worker-bearer");

    let (code, _, err) =
        run_with_home(&["tasks", "cancel", "task_related"], &url, None, &home).await;
    // The Hub stub answers 200, but the important part is that the CLI reached it using
    // the summary version rather than failing locally.
    assert_eq!(code, ExitCode::SUCCESS, "{err}");
    let requests = hub.all();
    assert_eq!(requests[0].path, "/v1/tasks/task_related");
    assert_eq!(requests[1].path, "/v1/tasks/task_related/cancel");
    assert_eq!(requests[1].body["expectedTaskVersion"], 7);
    assert_eq!(
        requests[1].authorization.as_deref(),
        Some("Bearer worker-bearer")
    );
}
