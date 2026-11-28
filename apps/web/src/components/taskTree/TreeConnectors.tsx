/**
 * Pixel-accurate rounded SVG tree connectors aligned to parent chevron centers.
 *
 * Each ancestor column draws a continuous vertical stem only when that ancestor
 * still has remaining siblings; the final column draws a rounded `├` or `└`.
 * Slot widths come from CSS so a narrow screen can tighten the indent.
 */

export interface TreeConnectorsProps {
  readonly depth: number;
  readonly isLastSibling: boolean;
  readonly continuations: readonly boolean[];
}

export function TreeConnectors({ depth, isLastSibling, continuations }: TreeConnectorsProps) {
  if (depth <= 0) {
    return null;
  }

  return (
    <span className="list-guides" aria-hidden="true">
      {Array.from({ length: depth }, (_, level) => {
        const isBranch = level === depth - 1;
        if (!isBranch) {
          const drawStem = continuations[level] ?? false;
          return (
            <span className="list-guide-slot" key={level}>
              {drawStem ? <span className="list-guide-stem" /> : null}
            </span>
          );
        }

        return (
          <span className="list-guide-slot" key={level}>
            <svg
              className="list-guide-svg"
              width="22"
              height="52"
              viewBox="0 0 22 52"
              preserveAspectRatio="none"
            >
              {isLastSibling ? (
                <path
                  d="M 10 0 L 10 20 Q 10 26 16 26 L 21 26"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.15"
                  vectorEffect="non-scaling-stroke"
                />
              ) : (
                <>
                  <path
                    d="M 10 0 L 10 52"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.15"
                    vectorEffect="non-scaling-stroke"
                  />
                  <path
                    d="M 10 20 Q 10 26 16 26 L 21 26"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.15"
                    vectorEffect="non-scaling-stroke"
                  />
                </>
              )}
            </svg>
          </span>
        );
      })}
    </span>
  );
}
