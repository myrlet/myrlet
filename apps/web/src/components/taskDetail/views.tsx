/**
 * The four read-only Task Detail tab views.
 *
 * Each tab is a standalone component with narrow props, so it can be rendered
 * and asserted on its own; the panel below only decides which one is visible.
 */

import type { Ref } from "react";

import type { TaskRecord, TaskTreeResponse } from "../../api/types";
import { DeliveryTabView } from "./DeliveryTabView";
import { JsonTabView } from "./JsonTabView";
import { OverviewView } from "./OverviewView";
import { TreeView } from "./TreeView";

export type DetailTab = "overview" | "tree" | "delivery" | "json";

export const DETAIL_TABS: readonly { id: DetailTab; label: string }[] = [
  { id: "overview", label: "Overview" },
  { id: "tree", label: "Tree & Deps" },
  { id: "delivery", label: "Delivery" },
  { id: "json", label: "JSON" },
];

export interface DetailTabBodyProps {
  readonly tab: DetailTab;
  readonly task: TaskRecord;
  readonly tree: TaskTreeResponse | null;
  readonly onSelectTask: (taskId: string) => void;
  readonly onCopyText: (label: string, text: string) => void;
  readonly onOpenReader: () => void;
  readonly readerTriggerRef?: Ref<HTMLButtonElement>;
}

/** Renders exactly the selected tab. */
export function DetailTabBody({
  tab,
  task,
  tree,
  onSelectTask,
  onCopyText,
  onOpenReader,
  readerTriggerRef,
}: DetailTabBodyProps) {
  if (tab === "overview") {
    return (
      <OverviewView
        task={task}
        onCopyText={onCopyText}
        onOpenReader={onOpenReader}
        readerTriggerRef={readerTriggerRef}
      />
    );
  }
  if (tab === "tree") {
    return <TreeView task={task} tree={tree} onSelectTask={onSelectTask} />;
  }
  if (tab === "delivery") {
    return <DeliveryTabView task={task} />;
  }
  return <JsonTabView task={task} onCopyText={onCopyText} />;
}
