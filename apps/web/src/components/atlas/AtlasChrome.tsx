/**
 * Board dock chrome: the `Board controls` toolbar and the zoom cluster.
 *
 * These are pure presentation: every action arrives as a callback from the
 * workspace, which keeps the global shortcuts and the `controlsRef` contract.
 */

import {
  Crosshair,
  LayoutGrid,
  Maximize2,
  Minus,
  PanelRight,
  Plus,
  RotateCcw,
  Waypoints,
  X,
} from "lucide-react";

interface AtlasEmptyBoardProps {
  readonly activeStatusLabel: string | null;
  readonly onClearFilter: () => void;
}

/** Board with no cards: either nothing was submitted or the filter excludes all. */
export function AtlasEmptyBoard({ activeStatusLabel, onClearFilter }: AtlasEmptyBoardProps) {
  return (
    <div className="pane-empty">
      <Waypoints size={20} aria-hidden="true" />
      {activeStatusLabel !== null ? (
        <>
          <span className="pane-empty-title">No Tasks match this status</span>
          <button type="button" className="button button-sm" onClick={onClearFilter}>
            Clear filter
          </button>
        </>
      ) : (
        <>
          <span className="pane-empty-title">No Tasks in this Hub yet</span>
          <span>
            Submit Tasks from the Myrlet CLI or an enrolled agent to see them arranged
            here.
          </span>
        </>
      )}
    </div>
  );
}

interface AtlasDockProps {
  readonly activeStatusLabel: string | null;
  readonly inspectorOpen: boolean;
  readonly selectionActive: boolean;
  readonly canRestore: boolean;
  readonly onClearFilter: () => void;
  readonly onFit: () => void;
  readonly onFocusSelected: () => void;
  readonly onRestore: () => void;
  readonly onArrange: () => void;
  readonly onToggleInspector: () => void;
}

export function AtlasDock({
  activeStatusLabel,
  inspectorOpen,
  selectionActive,
  canRestore,
  onClearFilter,
  onFit,
  onFocusSelected,
  onRestore,
  onArrange,
  onToggleInspector,
}: AtlasDockProps) {
  return (
    <div className="atlas-dock" role="toolbar" aria-label="Board controls">
      <span className="atlas-dock-label">Relationships</span>
      {activeStatusLabel === null ? null : (
        <span className="active-filter-pill">
          <span>Filtered: {activeStatusLabel}</span>
          <button
            type="button"
            className="active-filter-clear"
            aria-label="Clear status filter"
            onClick={onClearFilter}
          >
            <X size={12} aria-hidden="true" />
          </button>
        </span>
      )}
      <span className="atlas-dock-spacer" />
      <div className="atlas-dock-actions">
        <button type="button" className="atlas-floating-btn" title="Fit all groups (F)" onClick={onFit}>
          <Maximize2 size={12} aria-hidden="true" />
          <span>Fit</span>
        </button>
        {selectionActive ? (
          <button
            type="button"
            className="atlas-floating-btn"
            title="Center selected Task (Shift+F)"
            onClick={onFocusSelected}
          >
            <Crosshair size={12} aria-hidden="true" />
            <span>Focus</span>
          </button>
        ) : null}
        {canRestore ? (
          <button
            type="button"
            className="atlas-floating-btn"
            title="Restore previous layout"
            onClick={onRestore}
          >
            <RotateCcw size={12} aria-hidden="true" />
            <span>Restore</span>
          </button>
        ) : null}
        <button type="button" className="atlas-floating-btn" title="Arrange board layout" onClick={onArrange}>
          <LayoutGrid size={12} aria-hidden="true" />
          <span>Arrange</span>
        </button>
        <span className="atlas-floating-divider" aria-hidden="true" />
        <button
          type="button"
          className="atlas-floating-btn"
          data-active={inspectorOpen}
          aria-pressed={inspectorOpen}
          title="Toggle Inspector (I)"
          onClick={onToggleInspector}
        >
          <PanelRight size={12} aria-hidden="true" />
          <span>Inspector</span>
        </button>
      </div>
    </div>
  );
}

interface AtlasZoomProps {
  readonly scale: number;
  readonly zoomPercent: number;
  readonly minScale: number;
  readonly maxScale: number;
  readonly zoomFactor: number;
  readonly onZoomTo: (resolve: (scale: number) => number) => void;
  readonly onResetZoom: () => void;
}

export function AtlasZoom({
  scale,
  zoomPercent,
  minScale,
  maxScale,
  zoomFactor,
  onZoomTo,
  onResetZoom,
}: AtlasZoomProps) {
  return (
    <div className="atlas-zoom">
      <button
        type="button" className="atlas-zoom-btn" onClick={() => onZoomTo((current) => current / zoomFactor)}
        disabled={scale <= minScale + 0.001} aria-label="Zoom out" title="Zoom out"
      >
        <Minus size={13} aria-hidden="true" />
      </button>
      <button
        type="button" className="atlas-zoom-value tabular" onClick={onResetZoom}
        aria-label={`Zoom ${zoomPercent}% — reset to 100%`} title="Reset zoom to 100%"
      >
        {zoomPercent}%
      </button>
      <button
        type="button" className="atlas-zoom-btn" onClick={() => onZoomTo((current) => current * zoomFactor)}
        disabled={scale >= maxScale - 0.001} aria-label="Zoom in" title="Zoom in"
      >
        <Plus size={13} aria-hidden="true" />
      </button>
    </div>
  );
}
