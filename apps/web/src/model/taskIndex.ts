import type { SnapshotTaskSummary, TaskRecord } from "../api/types";

export type TaskTreeSource = SnapshotTaskSummary | TaskRecord;

export function taskParentId(task: TaskTreeSource): string | null {
  return "task" in task ? task.task.parentTaskId : task.parentTaskId;
}

export function compareTasks(a: TaskTreeSource, b: TaskTreeSource): number {
  if (a.createdAtMs !== b.createdAtMs) return a.createdAtMs - b.createdAtMs;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/** Index the available records. Each consumer decides how to traverse or lay them out. */
export function indexTasks<T extends TaskTreeSource>(tasks: readonly T[]) {
  const byId = new Map(tasks.map((task) => [task.id, task]));
  const roots: T[] = [];
  const childrenByParent = new Map<string, T[]>();
  for (const task of tasks) {
    const parent = taskParentId(task);
    if (parent === null || parent === task.id || !byId.has(parent)) {
      roots.push(task);
      continue;
    }
    const children = childrenByParent.get(parent);
    if (children) children.push(task);
    else childrenByParent.set(parent, [task]);
  }
  roots.sort(compareTasks);
  for (const children of childrenByParent.values()) children.sort(compareTasks);
  return { byId, childrenByParent, roots };
}
