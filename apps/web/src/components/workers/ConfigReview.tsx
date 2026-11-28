/**
 * Reported-versus-applied scheduling review.
 *
 * Apply exists so an Admin admits exactly the report they inspected. This view therefore
 * shows both sides verbatim and labels the reported side with the revision of the body it
 * is actually displaying — the same revision Apply submits — so the two can never
 * disagree. It states only the structural differences it can see (new, removed, changed
 * Slot), never a judgement about whether the Worker or its capacity is healthy.
 */

import type { WorkerRecord } from "../../api/types";
import type { ReviewedConfig } from "../../state/useWorkerConfigReview";
import { SlotTable } from "./WorkerInventory";

export interface ConfigReviewProps {
  readonly worker: WorkerRecord;
  /** The reviewed report and the exact revision it was reported at. */
  readonly reviewed: ReviewedConfig | null;
  /** Whether a fresh report is being loaded. */
  readonly loading: boolean;
}

export function ConfigReview({ worker, reviewed, loading }: ConfigReviewProps) {
  const reportedSlots = reviewed?.config.slots ?? [];
  const appliedSlots = worker.appliedConfig?.slots ?? [];
  const offered = reportedSlots.filter(
    (slot) => !appliedSlots.some((applied) => applied.slotId === slot.slotId),
  );
  const withdrawn = appliedSlots.filter(
    (slot) => !reportedSlots.some((reviewedSlot) => reviewedSlot.slotId === slot.slotId),
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
            Reported{" "}
            <span className="tabular">
              {reviewed === null ? "—" : `rev ${reviewed.revision}`}
            </span>
          </h3>
          {reviewed === null ? (
            <p className="detail-empty">
              {loading
                ? "Loading the report under review…"
                : "The report under review is not loaded."}
            </p>
          ) : (
            <SlotTable
              slots={reportedSlots}
              caption={`Reported Slots for ${worker.workerId} awaiting review at revision ${reviewed.revision}`}
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
        {reviewed === null
          ? "Load the report to compare it with the applied configuration."
          : differences.length === 0
            ? "No field differences."
            : differences.join(" · ")}
      </p>
    </div>
  );
}
