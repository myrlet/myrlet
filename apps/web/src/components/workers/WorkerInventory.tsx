/**
 * Worker inventory presentation: the KPI strip and the scheduling Slot table.
 * Advisory capacity of `null` is unknown and renders as `—`, never `0`.
 */

import type { SlotInventory } from "../../api/types";
import { CAPABILITY_STATE_LABELS, formatCapacity } from "../../format";
import type { WorkerCounts } from "./workerFilters";

export function KpiStrip({ registered, online, offline, pending, activeDeliveries }: WorkerCounts) {
  const cells: readonly { readonly label: string; readonly value: number }[] = [
    { label: "Registered", value: registered },
    { label: "Online", value: online },
    { label: "Awaiting approval", value: pending },
    { label: "Offline", value: offline },
    { label: "Active Deliveries", value: activeDeliveries },
  ];
  return (
    <div className="kpi-strip" aria-label="Worker hardware summary">
      {cells.map((cell) => (
        <div className="kpi-cell" key={cell.label}>
          <span className="kpi-value tabular">{cell.value}</span>
          <span className="kpi-label">{cell.label}</span>
        </div>
      ))}
    </div>
  );
}

export function SlotTable({
  slots,
  caption,
}: {
  readonly slots: readonly SlotInventory[];
  /** Row context for this table; every caller states what the Slots are. */
  readonly caption: string;
}) {
  if (slots.length === 0) {
    return <p className="detail-empty">No Slots reported yet.</p>;
  }
  return (
    <table className="slot-table">
      <caption className="visually-hidden">{caption}</caption>
      <thead>
        <tr>
          <th scope="col">Slot</th>
          <th scope="col">Advisory Capacity</th>
          <th scope="col">Roles</th>
          <th scope="col">Capabilities</th>
          <th scope="col">Executors</th>
        </tr>
      </thead>
      <tbody>
        {slots.map((slot) => (
          <tr key={slot.slotId} data-slot-id={slot.slotId}>
            <td>
              <span className={slot.enabled ? "slot-name" : "slot-name slot-disabled"}>
                {slot.slotId}
              </span>
              <span className="slot-state" data-enabled={slot.enabled}>
                {slot.enabled ? "enabled" : "disabled"}
              </span>
            </td>
            <td className="tabular">{formatCapacity(slot.advisoryCapacity)}</td>
            <td className="cell-muted">
              {slot.roles.length === 0 ? "—" : slot.roles.join(", ")}
            </td>
            <td>
              {slot.capabilities.length === 0 ? (
                <span className="cell-empty">—</span>
              ) : (
                <span className="capability-list">
                  {slot.capabilities.map((capability) => (
                    <span
                      className="capability-chip"
                      key={capability.name}
                      title={CAPABILITY_STATE_LABELS[capability.state]}
                    >
                      <span
                        className="state-dot"
                        data-state={capability.state}
                        aria-hidden="true"
                      />
                      {capability.name}
                    </span>
                  ))}
                </span>
              )}
            </td>
            <td className="mono cell-muted">
              {slot.executors.length === 0
                ? "—"
                : slot.executors.map((executor) => `${executor.id} · ${executor.kind}`).join(", ")}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
