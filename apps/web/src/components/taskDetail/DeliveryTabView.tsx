/**
 * Delivery tab: the committed Delivery that owns this Task, and the latest
 * Harness Progress it reported.
 *
 * A missing Delivery or Progress is stated explicitly. Cancellation is relayed
 * as the Worker reported it — `uncertain` and `unsupported` stay visible.
 */

import type { TaskRecord } from "../../api/types";
import {
  CANCEL_STATE_LABELS,
  DELIVERY_STATUS_LABELS,
  formatRelativeTime,
  formatTimestamp,
} from "../../format";
import { IdValue } from "./fields";

export interface DeliveryViewProps {
  readonly task: TaskRecord;
}

export function DeliveryTabView({ task }: DeliveryViewProps) {
  const delivery = task.currentDelivery;
  const progress = delivery?.progress ?? null;

  return (
    <>
      <section className="detail-section">
        <h3 className="detail-section-title">Current Delivery</h3>
        {delivery == null ? (
          <p className="detail-empty">No Delivery assigned yet.</p>
        ) : (
          <div className="detail-grid">
            <span className="detail-label">Delivery</span>
            <span className="detail-value mono">{delivery.id}</span>

            <span className="detail-label">State</span>
            <span className="detail-value">{DELIVERY_STATUS_LABELS[delivery.state]}</span>

            <span className="detail-label">Worker</span>
            <span className="detail-value mono">{delivery.workerId}</span>

            <span className="detail-label">Slot</span>
            <span className="detail-value mono">{delivery.slotId}</span>

            <span className="detail-label">Executor</span>
            <IdValue id={delivery.executorId} />

            <span className="detail-label">Connection epoch</span>
            <span className="detail-value tabular">{delivery.connectionEpoch}</span>

            {delivery.cancelState == null ? null : (
              <>
                <span className="detail-label">Cancel</span>
                <span className="detail-value">{CANCEL_STATE_LABELS[delivery.cancelState]}</span>
              </>
            )}

            {delivery.cancelReason == null || delivery.cancelReason === "" ? null : (
              <>
                <span className="detail-label">Cancel reason</span>
                <span className="detail-value">{delivery.cancelReason}</span>
              </>
            )}
          </div>
        )}
      </section>

      <section className="detail-section">
        <h3 className="detail-section-title">Latest Progress</h3>
        {delivery == null ? (
          <p className="detail-empty">No Delivery assigned yet.</p>
        ) : progress == null ? (
          <p className="detail-empty">No Harness progress reported yet.</p>
        ) : (
          <div className="progress-block">
            <p className="progress-block-text">{progress.text}</p>
            <div className="progress-block-meta">
              <span title={formatTimestamp(progress.firstReceivedAtMs)}>
                Execution started: {formatRelativeTime(progress.firstReceivedAtMs)}
              </span>
              <span title={formatTimestamp(progress.receivedAtMs)}>
                Last updated: {formatRelativeTime(progress.receivedAtMs)}
              </span>
            </div>
          </div>
        )}
      </section>
    </>
  );
}
