//! Atomic Task Bundle import: up to 100 `held` Tasks with bundle-local references.

use crate::id::TaskId;
use crate::task::{TaskDefinition, TaskRecord, TaskValidationError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use thiserror::Error;

pub const MAX_BUNDLE_TASKS: usize = 100;
pub const MAX_TREE_DEPTH: u8 = 5;

/// Bundle import request; creates and submit-authorizes every Task in one transaction.
///
/// Each Task lands in `queued`, or stays `held` while any prerequisite would not be
/// `finished` after this transaction. Import never picks a Worker and never creates a
/// Delivery.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBundleImportRequest {
    pub tasks: Vec<TaskDefinition>,
}

/// Bundle import response with resolved local references and created records.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBundleImportResponse {
    pub client_ref_to_task_id: BTreeMap<String, TaskId>,
    pub tasks: Vec<TaskRecord>,
    /// Task IDs that are `queued` and claimable immediately.
    #[serde(default)]
    pub eligible_task_ids: Vec<TaskId>,
}

impl TaskBundleImportRequest {
    /// Validate the bundle and every contained definition before persistence.
    pub fn validate(&self) -> Result<(), BundleValidationError> {
        if self.tasks.is_empty() || self.tasks.len() > MAX_BUNDLE_TASKS {
            return Err(BundleValidationError::TaskCountOutOfRange);
        }
        let mut refs: HashMap<&str, usize> = HashMap::with_capacity(self.tasks.len());
        for (index, task) in self.tasks.iter().enumerate() {
            task.validate()
                .map_err(|source| BundleValidationError::InvalidTask(index, source))?;
            let Some(client_ref) = task.client_ref.as_deref() else {
                continue;
            };
            if client_ref.is_empty() {
                return Err(BundleValidationError::EmptyClientRef(index));
            }
            if refs.insert(client_ref, index).is_some() {
                return Err(BundleValidationError::DuplicateClientRef(
                    client_ref.to_owned(),
                ));
            }
        }
        let mut parent_edges = vec![Vec::new(); self.tasks.len()];
        let mut dependency_edges = vec![Vec::new(); self.tasks.len()];
        let mut parent_of = Vec::with_capacity(self.tasks.len());
        for (index, task) in self.tasks.iter().enumerate() {
            let self_parent = task
                .parent_task_id
                .as_ref()
                .is_some_and(|parent| task.client_ref.as_deref() == Some(parent.as_str()));
            if self_parent {
                return Err(BundleValidationError::SelfParent(index));
            }
            let parent = task
                .parent_task_id
                .as_ref()
                .and_then(|parent| refs.get(parent.as_str()).copied());
            if let Some(parent_index) = parent {
                parent_edges[parent_index].push(index);
            }
            parent_of.push(parent);
            for dependency in &task.dependencies {
                let Some(&dependency_index) = refs.get(dependency.as_str()) else {
                    continue;
                };
                if self.tasks[dependency_index].parent_task_id != task.parent_task_id {
                    return Err(BundleValidationError::NonSiblingDependency(
                        index,
                        dependency.clone(),
                    ));
                }
                dependency_edges[dependency_index].push(index);
            }
        }
        if let Some(index) = find_cycle(&parent_edges) {
            return Err(BundleValidationError::ParentCycle(index));
        }
        if let Some(index) = find_cycle(&dependency_edges) {
            return Err(BundleValidationError::DependencyCycle(index));
        }
        match (0..self.tasks.len()).find(|&index| depth_of(index, &parent_of) > MAX_TREE_DEPTH) {
            Some(index) => Err(BundleValidationError::DepthExceeded(index)),
            None => Ok(()),
        }
    }
}

fn find_cycle(edges: &[Vec<usize>]) -> Option<usize> {
    let mut indegree = vec![0usize; edges.len()];
    for targets in edges {
        for &target in targets {
            indegree[target] += 1;
        }
    }
    let mut ready: Vec<usize> = (0..edges.len())
        .filter(|&index| indegree[index] == 0)
        .collect();
    while let Some(index) = ready.pop() {
        for &target in &edges[index] {
            indegree[target] -= 1;
            if indegree[target] == 0 {
                ready.push(target);
            }
        }
    }
    (0..edges.len()).find(|&index| indegree[index] > 0)
}

fn depth_of(index: usize, parent_of: &[Option<usize>]) -> u8 {
    let mut depth = 1u8;
    let mut current = index;
    while let Some(parent) = parent_of[current] {
        depth = depth.saturating_add(1);
        current = parent;
    }
    depth
}

/// Rejection reasons for an invalid Bundle import.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BundleValidationError {
    #[error("bundle must contain between 1 and 100 tasks")]
    TaskCountOutOfRange,
    #[error("task at index {0} is invalid")]
    InvalidTask(usize, #[source] TaskValidationError),
    #[error("task at index {0} has an empty clientRef")]
    EmptyClientRef(usize),
    #[error("duplicate clientRef {0}")]
    DuplicateClientRef(String),
    #[error("task at index {0} cannot be its own parent")]
    SelfParent(usize),
    #[error("task at index {0} introduces a parent cycle")]
    ParentCycle(usize),
    #[error("task at index {0} exceeds the maximum tree depth")]
    DepthExceeded(usize),
    #[error("task at index {0} depends on non-sibling task {1}")]
    NonSiblingDependency(usize, TaskId),
    #[error("task at index {0} participates in a dependency cycle")]
    DependencyCycle(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use BundleValidationError as E;

    fn task(client_ref: &str, parent: Option<&str>) -> TaskDefinition {
        let mut task: TaskDefinition = serde_json::from_value(serde_json::json!({
            "clientRef": client_ref, "title": client_ref, "prompt": "p"
        }))
        .unwrap();
        task.parent_task_id = parent.map(TaskId::from);
        task
    }

    fn check(specs: Vec<(&str, Option<&str>)>) -> Result<(), E> {
        let tasks = specs.into_iter().map(|(r, p)| task(r, p)).collect();
        TaskBundleImportRequest { tasks }.validate()
    }

    #[test]
    fn accepts_valid_and_rejects_bad_references_and_excess_depth() {
        let mut child = task("child", Some("root"));
        child.dependencies = vec![TaskId::from("sibling")];
        let valid = vec![task("root", None), child, task("sibling", Some("root"))];
        assert_eq!(TaskBundleImportRequest { tasks: valid }.validate(), Ok(()));
        let duplicate = check(vec![("a", None), ("a", None)]);
        assert_eq!(duplicate, Err(E::DuplicateClientRef("a".into())));
        assert_eq!(check(vec![("a", Some("a"))]), Err(E::SelfParent(0)));
        let cycle = check(vec![("a", Some("b")), ("b", Some("a"))]);
        assert_eq!(cycle, Err(E::ParentCycle(0)));
        let refs = ["d1", "d2", "d3", "d4", "d5", "d6"];
        let chain = (0..refs.len()).map(|i| (refs[i], i.checked_sub(1).map(|p| refs[p])));
        let depth = check(chain.collect());
        assert_eq!(depth, Err(E::DepthExceeded(5)));
    }

    #[test]
    fn rejects_non_sibling_and_cyclic_dependencies() {
        let mut other = task("other", None);
        other.dependencies = vec![TaskId::from("child")];
        let bundle = vec![task("root", None), task("child", Some("root")), other];
        let error = TaskBundleImportRequest { tasks: bundle }.validate();
        let expected = E::NonSiblingDependency(2, TaskId::from("child"));
        assert_eq!(error, Err(expected));
        let mut first = task("first", Some("root"));
        let mut second = task("second", Some("root"));
        first.dependencies = vec![TaskId::from("second")];
        second.dependencies = vec![TaskId::from("first")];
        let bundle = vec![task("root", None), first, second];
        let error = TaskBundleImportRequest { tasks: bundle }.validate();
        assert_eq!(error, Err(E::DependencyCycle(1)));
    }
}
