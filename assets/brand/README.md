# Myrlet brand assets

These are the shared visual assets for the selected monochrome Myrlet ant mark: two bent antennae, two horizontal eyes, and three collaboration nodes.

## Status Face System

Runtime expression assets live in [`expressions/`](expressions/README.md), with five visual states (Default, Working, Idle, Warning, Offline), inverse options, design geometry tokens, a review sheet, and [motion guidance](expressions/motion-spec.md).

The two official brand marks below remain the authoritative static logos. The Default status face is an **exact copy**, never a modified alternative. Do not use a runtime expression in branding.

## Files and purpose

| Asset | Use |
| --- | --- |
| `myrlet-mark.svg` | Default, scalable black mark on white/light surfaces; canonical editable SVG representation |
| `myrlet-mark-inverse.svg` | White version for dark backgrounds |
| `myrlet-avatar-512.png` | Square white-background PNG for the GitHub organization avatar |

To export any other resolution, render the SVG rather than committing many redundant PNG sizes:

```bash
inkscape assets/brand/myrlet-mark.svg --export-type=png --export-width=1024 --export-filename=myrlet-avatar-1024.png
```

## Usage rules

- Keep the icon in monochrome; don't recolor the eyes or collaboration nodes independently.
- Preserve the two bent antennae and all three collaboration nodes; don't crop the icon inside a circle.
- Reserve padding around the icon, especially for GitHub's circular avatar mask.
- The official static mark uses the two default **horizontal eyes**. Runtime Status Face variants are a distinct UI component and must not replace the official repository/organization logo.
- This folder includes only the **symbol**, not a finalized `Myrlet` wordmark or a proprietary font.
- Never infer service health from the logo alone: product interfaces must also convey explicit status text.

## Source and quality

The paths have been **reconstructed with smooth cubic Bézier curves from the approved pixel-traced silhouette** (and rounded eye capsules), replacing the former dense straight-line polygon traces. They contain no embedded bitmap. This is an optical refinement based on the approved raster, **not** a recovered original designer-authored master. See the [before/after comparison](expressions/vector-refinement-comparison.svg) and review curve fidelity manually before high-precision print or manufacturing use.

For favicon and tray sizes, verify visibility at **16, 24, and 32 px**. A separately reviewed small-size optical variant may be warranted; don't assume shrinking every detail works.
