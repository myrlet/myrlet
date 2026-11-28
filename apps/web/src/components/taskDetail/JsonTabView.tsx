/**
 * JSON tab: the authoritative Task record as JSON, with its copy action, and
 * the read-only CLI command that produced the same facts.
 */

import { Copy } from "lucide-react";

import type { TaskRecord } from "../../api/types";
import { JsonViewer } from "./JsonViewer";

export interface JsonTabViewProps {
  readonly task: TaskRecord;
  readonly onCopyText: (label: string, text: string) => void;
}

export function JsonTabView({ task, onCopyText }: JsonTabViewProps) {
  const cliSnippet = `myrlet tasks show ${task.id}`;

  return (
    <>
      <section className="detail-section">
        <div className="prompt-box-head">
          <h3 className="detail-section-title">Task JSON</h3>
          <span className="header-spacer" />
          <button
            type="button"
            className="button button-sm button-ghost"
            onClick={() => onCopyText("Task JSON", JSON.stringify(task, null, 2))}
            title="Copy the full Task JSON"
          >
            <Copy size={12} aria-hidden="true" />
            <span>Copy JSON</span>
          </button>
        </div>
        <JsonViewer value={task} />
      </section>

      <section className="detail-section">
        <h3 className="detail-section-title">CLI</h3>
        <div className="copy-row">
          <code className="code-block">{cliSnippet}</code>
          <button
            type="button"
            className="button button-sm button-ghost"
            onClick={() => onCopyText("CLI command", cliSnippet)}
            title="Copy CLI command"
            aria-label="Copy CLI command"
          >
            <Copy size={12} aria-hidden="true" />
          </button>
        </div>
      </section>
    </>
  );
}
