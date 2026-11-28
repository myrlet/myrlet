//! Worker-scoped Task reads: the one-hop authorization boundary, deterministic bounded
//! related summaries, and the indexes that keep the lookup cheap.

use std::collections::HashMap;

use myrlet_protocol::{TaskDefinition, TaskId, WorkerId, WorkerInventory};
use myrlet_store::HubStore;

const T0: i64 = 1_764_070_000_000;
const SLOT: &str = "slot-a";

fn inventory() -> WorkerInventory {
    serde_json::from_value(serde_json::json!({
        "revision": 1,
        "observedAtMs": T0,
        "host": { "os": "linux", "arch": "x86_64", "deploymentMode": "native" },
        "slots": [{
            "slotId": SLOT,
            "enabled": true,
            "capabilities": [{ "name": "shell", "state": "available" }],
            "executors": [{ "id": "exec-claude", "kind": "claude" }],
        }]
    }))
    .unwrap()
}

fn definition(title: &str, parent: Option<&TaskId>, dependencies: &[&TaskId]) -> TaskDefinition {
    let mut definition: TaskDefinition =
        serde_json::from_value(serde_json::json!({ "title": title, "prompt": "p" })).unwrap();
    definition.parent_task_id = parent.cloned();
    definition.dependencies = dependencies.iter().map(|id| (*id).clone()).collect();
    definition
}

/// Pair and fully enroll one Worker so it may claim.
async fn enrolled(store: &HubStore) -> (WorkerId, String, u64) {
    let issued = store
        .create_enrollment_code(Some(600), None, T0)
        .await
        .unwrap();
    let request = serde_json::from_value(serde_json::json!({
        "enrollmentCode": issued.enrollment_code,
    }))
    .unwrap();
    let (paired, _) = store
        .redeem_pairing_code("hub_1".into(), request, T0)
        .await
        .unwrap();
    let worker_id = paired.worker_id;
    let (epoch, _) = store
        .open_worker_connection(worker_id.clone(), inventory(), T0)
        .await
        .unwrap();
    store.approve_worker(worker_id.clone(), T0).await.unwrap();
    let reported = store
        .get_worker(worker_id.clone(), HashMap::new())
        .await
        .unwrap()
        .reported_revision
        .expect("a report exists");
    store
        .apply_worker_config(worker_id.clone(), reported, T0)
        .await
        .unwrap();
    (worker_id, paired.credential, epoch)
}

async fn claim(store: &HubStore, worker_id: WorkerId, task_id: &TaskId, epoch: u64) {
    let request = serde_json::from_value(serde_json::json!({
        "taskId": task_id.as_str(),
        "slotId": SLOT,
        "connectionEpoch": epoch,
    }))
    .unwrap();
    store
        .claim_task(worker_id, request, T0)
        .await
        .expect("claim succeeds");
}

/// Create one Task and return its ID.
async fn create(store: &HubStore, key: &str, def: TaskDefinition) -> TaskId {
    let (record, _, _) = store.create_task(key.into(), def, T0).await.unwrap();
    record.id
}

/// A root plus one assigned Task plus parent, child, dependency, and dependent neighbors.
struct Graph {
    root: TaskId,
    assigned: TaskId,
    child: TaskId,
    dependency: TaskId,
    dependent: TaskId,
    stranger: TaskId,
}

async fn graph(store: &HubStore, worker_id: WorkerId, epoch: u64) -> Graph {
    let root = create(store, "root", definition("Root", None, &[])).await;
    let assigned = create(store, "assigned", definition("Assigned", Some(&root), &[])).await;
    let child = create(store, "child", definition("Child", Some(&assigned), &[])).await;
    let dependency = create(
        store,
        "dependency",
        definition("Dependency", Some(&root), &[]),
    )
    .await;
    let dependent = create(
        store,
        "dependent",
        definition("Dependent", Some(&root), &[&assigned]),
    )
    .await;
    let stranger = create(store, "stranger", definition("Stranger", Some(&root), &[])).await;
    claim(store, worker_id, &assigned, epoch).await;
    Graph {
        root,
        assigned,
        child,
        dependency,
        dependent,
        stranger,
    }
}

#[tokio::test]
async fn one_hop_visibility_covers_all_four_directions() {
    let store = HubStore::open_in_memory().unwrap();
    let (worker_id, _credential, epoch) = enrolled(&store).await;
    let graph = graph(&store, worker_id.clone(), epoch).await;

    // The assigned Task reads in full; every direct relative is a summary.
    let assigned = store
        .worker_task_read(worker_id.clone(), graph.assigned.clone())
        .await
        .unwrap()
        .expect("assigned is visible");
    assert!(assigned.task.is_some());
    assert!(assigned.summary.is_none());

    let related: Vec<&str> = assigned.related.iter().map(|t| t.id.as_str()).collect();
    assert!(
        related.contains(&graph.root.as_str()),
        "parent: {related:?}"
    );
    assert!(
        related.contains(&graph.child.as_str()),
        "child: {related:?}"
    );
    assert!(
        related.contains(&graph.dependent.as_str()),
        "dependent: {related:?}"
    );
    // A sibling that is neither dependency nor dependent is outside the boundary.
    assert!(!related.contains(&graph.stranger.as_str()));
    // The assigned Task never lists itself.
    assert!(!related.contains(&graph.assigned.as_str()));

    // Each direction is readable as a limited summary, and only as a summary.
    for id in [&graph.root, &graph.child, &graph.dependent] {
        let read = store
            .worker_task_read(worker_id.clone(), id.clone())
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{id} should be visible"));
        assert!(read.task.is_none(), "{id} must not be a full record");
        assert_eq!(read.summary.as_ref().unwrap().id, *id);
        assert!(!read.related_truncated);
    }

    // The dependency Task is *not* a neighbor of the assigned Task: `assigned` declares no
    // dependencies. It is a sibling and stays invisible.
    assert!(store
        .worker_task_read(worker_id.clone(), graph.dependency.clone())
        .await
        .unwrap()
        .is_none());
    assert!(store
        .worker_task_read(worker_id, graph.stranger)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn a_declared_dependency_is_visible_in_both_directions() {
    let store = HubStore::open_in_memory().unwrap();
    let (worker_id, _credential, epoch) = enrolled(&store).await;
    let root = create(&store, "root", definition("Root", None, &[])).await;
    let dependency = create(
        &store,
        "dependency",
        definition("Dependency", Some(&root), &[]),
    )
    .await;
    let assigned = create(
        &store,
        "assigned",
        definition("Assigned", Some(&root), &[&dependency]),
    )
    .await;
    // Claim the prerequisite: the dependent Task becomes visible only up to a summary.
    claim(&store, worker_id.clone(), &dependency, epoch).await;

    // The dependency's declared dependent is visible from the assigned Worker side.
    let from_dependency = store
        .worker_task_read(worker_id.clone(), dependency.clone())
        .await
        .unwrap()
        .unwrap();
    let related: Vec<&str> = from_dependency
        .related
        .iter()
        .map(|t| t.id.as_str())
        .collect();
    assert!(related.contains(&assigned.as_str()), "{related:?}");

    // The blocked dependent is a direct relative, but only as a limited summary.
    let dependent = store
        .worker_task_read(worker_id.clone(), assigned.clone())
        .await
        .unwrap()
        .expect("a direct dependent is visible");
    assert!(dependent.task.is_none());
    assert_eq!(dependent.summary.unwrap().id, assigned);

    // The prerequisite itself is a full read with its dependent listed.
    assert!(from_dependency.task.is_some());
    assert!(store
        .worker_task_read(worker_id.clone(), dependency.clone())
        .await
        .unwrap()
        .unwrap()
        .task
        .is_some());

    // Historical Deliveries still authorize reads: finish the assigned prerequisite,
    // then confirm the Task remains a full read for its Worker.
    let version = store.get_task(dependency.clone()).await.unwrap().version;
    let finish = serde_json::from_value(serde_json::json!({ "epoch": epoch })).unwrap();
    let delivery = store
        .get_task(dependency.clone())
        .await
        .unwrap()
        .current_delivery
        .expect("claimed");
    store
        .record_finish(worker_id.clone(), delivery.id, finish, T0)
        .await
        .unwrap();
    assert_eq!(
        store.get_task(dependency.clone()).await.unwrap().version,
        version + 1
    );
    let after_finish = store
        .worker_task_read(worker_id, dependency)
        .await
        .unwrap()
        .expect("a finished Delivery still assigns the Task");
    assert!(after_finish.task.is_some());
}

#[tokio::test]
async fn related_summaries_are_bounded_and_ordered_deterministically() {
    let store = HubStore::open_in_memory().unwrap();
    let (worker_id, _credential, epoch) = enrolled(&store).await;
    let root = create(&store, "root", definition("Root", None, &[])).await;
    let assigned = create(&store, "assigned", definition("Assigned", Some(&root), &[])).await;
    claim(&store, worker_id.clone(), &assigned, epoch).await;

    // More children than the bounded page allows.
    let limit = myrlet_store::scoped::MAX_RELATED_SUMMARIES;
    let overflow = limit + 5;
    for index in 0..overflow {
        create(
            &store,
            &format!("child-{index}"),
            definition(&format!("Child {index:03}"), Some(&assigned), &[]),
        )
        .await;
    }

    let read = store
        .worker_task_read(worker_id.clone(), assigned.clone())
        .await
        .unwrap()
        .unwrap();
    assert!(
        read.related_truncated,
        "an oversized page reports truncation"
    );
    assert_eq!(read.related.len(), limit);
    // Deterministic order: the same read returns the same page.
    let again = store
        .worker_task_read(worker_id, assigned)
        .await
        .unwrap()
        .unwrap();
    let ids: Vec<&str> = read.related.iter().map(|t| t.id.as_str()).collect();
    let again_ids: Vec<&str> = again.related.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, again_ids);
    // The parent is present among the bounded page.
    assert!(ids.contains(&root.as_str()));
}

#[tokio::test]
async fn zero_related_tasks_is_an_empty_untruncated_page() {
    let store = HubStore::open_in_memory().unwrap();
    let (worker_id, _credential, epoch) = enrolled(&store).await;
    let lonely = create(&store, "lonely", definition("Lonely", None, &[])).await;
    claim(&store, worker_id.clone(), &lonely, epoch).await;

    let read = store
        .worker_task_read(worker_id, lonely)
        .await
        .unwrap()
        .unwrap();
    assert!(read.related.is_empty());
    assert!(!read.related_truncated);
}

#[tokio::test]
async fn the_authorization_lookups_use_their_indexes() {
    let store = HubStore::open_in_memory().unwrap();
    let (worker_id, _credential, epoch) = enrolled(&store).await;
    let graph = graph(&store, worker_id.clone(), epoch).await;

    // The related-page join has to read children and dependents through their indexes.
    let plan = store.explain_related_lookup(&graph.assigned).await.unwrap();
    let plan = plan.join("\n");
    assert!(
        plan.contains("idx_tasks_parent"),
        "child lookup must use idx_tasks_parent:\n{plan}"
    );
    assert!(
        plan.contains("idx_task_deps_depends_on"),
        "dependent lookup must use idx_task_deps_depends_on:\n{plan}"
    );

    // Assignment authorization joins Deliveries by owner.
    let plan = store.explain_assignment_lookup(&worker_id).await.unwrap();
    let plan = plan.join("\n");
    assert!(
        plan.contains("idx_deliveries_worker_task") || plan.contains("PRIMARY KEY"),
        "assignment lookup must use a Delivery index:\n{plan}"
    );
}

#[tokio::test]
async fn a_large_delivery_history_does_not_multiply_queries() {
    let store = std::sync::Arc::new(HubStore::open_in_memory().unwrap());
    let (worker_id, _credential, epoch) = enrolled(&store).await;

    // Many historical, already-finished Deliveries for this Worker.
    for index in 0..200 {
        let task = create(
            &store,
            &format!("history-{index}"),
            definition(&format!("History {index}"), None, &[]),
        )
        .await;
        claim(&store, worker_id.clone(), &task, epoch).await;
        let delivery = store
            .get_task(task.clone())
            .await
            .unwrap()
            .current_delivery
            .unwrap();
        let finish = serde_json::from_value(serde_json::json!({ "epoch": epoch })).unwrap();
        store
            .record_finish(worker_id.clone(), delivery.id, finish, T0)
            .await
            .unwrap();
    }
    let target = create(&store, "target", definition("Target", None, &[])).await;
    claim(&store, worker_id.clone(), &target, epoch).await;
    let neighbor = create(
        &store,
        "neighbor",
        definition("Neighbor", Some(&target), &[]),
    )
    .await;

    // The authorization check is a single EXISTS regardless of history size.
    let read = store
        .worker_task_read(worker_id, neighbor.clone())
        .await
        .unwrap()
        .expect("a direct child is visible");
    assert!(read.task.is_none());
    assert_eq!(read.summary.unwrap().id, neighbor);
}

#[tokio::test]
async fn global_reads_are_unchanged_by_the_worker_boundary() {
    let store = HubStore::open_in_memory().unwrap();
    let (worker_id, _credential, epoch) = enrolled(&store).await;
    let graph = graph(&store, worker_id, epoch).await;

    // The global Task read still returns the full record with no related page.
    let full = store.get_task(graph.stranger.clone()).await.unwrap();
    assert_eq!(full.id, graph.stranger);
    // The global tree read is unaffected and never carries a related page.
    let tree = store
        .get_task_tree(
            graph.root.clone(),
            myrlet_protocol::TaskTreeDirection::Descendants,
            5,
            200,
        )
        .await
        .unwrap();
    assert!(tree.related.is_empty());
    assert!(!tree.related_truncated);
}

#[tokio::test]
async fn a_delivery_for_a_finished_task_still_assigns_it() {
    let store = HubStore::open_in_memory().unwrap();
    let (worker_id, _credential, epoch) = enrolled(&store).await;
    let task = create(&store, "done", definition("Done", None, &[])).await;
    claim(&store, worker_id.clone(), &task, epoch).await;
    let delivery = store
        .get_task(task.clone())
        .await
        .unwrap()
        .current_delivery
        .unwrap();
    let finish = serde_json::from_value(serde_json::json!({ "epoch": epoch })).unwrap();
    store
        .record_finish(worker_id.clone(), delivery.id.clone(), finish, T0)
        .await
        .unwrap();

    // A `finished` Delivery is still assignment evidence for read authorization.
    assert!(store
        .worker_task_read(worker_id, task)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn unknown_worker_reads_fail_closed() {
    let store = HubStore::open_in_memory().unwrap();
    let missing = WorkerId::new("wkr_000000000000");
    let task = TaskId::new("task_missing");
    // A Task that does not exist is simply not visible: the Hub maps `None` to 404.
    assert!(store
        .worker_task_read(missing, task)
        .await
        .unwrap()
        .is_none());
}
