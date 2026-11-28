/**
 * Reported-versus-applied scheduling review.
 *
 * Apply exists so an Admin admits exactly the report they inspected. This view
 * therefore shows both sides verbatim and states only the structural differences
 * it can see (new, removed, changed Slot) — never a judgement about whether the
 * Worker or its capacity is healthy.
 */

import type { WorkerRecord, WorkerSchedulingConfig } from "../../api/types";
import { SlotTable } from "./WorkerInventory";

export interface ConfigReviewProps {
  readonly worker: WorkerRecord;
  /** The report under review, or `null` while it is still being loaded. */
  readonly reported: WorkerSchedulingConfig | null;
}

export function ConfigReview({ worker, reported }: ConfigReviewProps) {
  const reportedSlots = reported?.slots ?? [];
  const appliedSlots = worker.appliedConfig?.slots ?? [];
  const offered = reportedSlots.filter(
    (slot) => !appliedSlots.some((applied) => applied.slotId === slot.slotId),
  );
  const withdrawn = appliedSlots.filter(
    (slot) => !reportedSlots.some((reviewed) => reviewed.slotId === slot.slotId),
  );
  const changed = reportedSlots.filter((slot) => {
    const applied = appliedSlots.find((candidate) => candidate.slotId === slot.slotId);
    return applied !== undefined && JSON.stringify(applied) !== JSON.stringify(slot);
  });
  const differences = [
    ...offered.map((slot) => `new Slot ${slot.slotId}`),
    ...withdrawn.map((slot) => `removed Slot ${slot.slotId}`),
    ...changed.map((slot) => `changed Slot ${slot.slotId}`),
  ];

  return (
    <div className="config-review">
      <div className="config-review-grid">
        <section aria-label="Reported scheduling inventory">
          <h3 className="config-review-title">
            Reported <span className="tabular">rev {worker.reportedRevision}</span>
          </h3>
          {reported === null ? (
            <p className="detail-empty">Loading the report under review…</p>
          ) : (
            <SlotTable
              slots={reportedSlots}
              caption={`Reported Slots for ${worker.workerId} awaiting review`}
            />
          )}
        </section>
        <section aria-label="Applied scheduling inventory">
          <h3 className="config-review-title">
            Applied{" "}
            <span className="tabular">
              {worker.appliedRevision === null ? "none" : `rev ${worker.appliedRevision}`}
            </span>
          </h3>
          <SlotTable
            slots={appliedSlots}
            caption={`Slots the Hub schedules for ${worker.workerId}`}
          />
        </section>
      </div>
      <p className="config-diff" data-empty={differences.length === 0}>
        {differences.length === 0 ? "No field differences." : differences.join(" · ")}
      </p>
    </div>
  );
}
