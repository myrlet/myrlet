# Myrlet Status Face System — SVG Assets

**One identity. Multiple states.** These files are visual options for a single observed resource, not five independent logos.

## Design review — light assets

| Default | Working | Idle | Warning | Offline |
|:---:|:---:|:---:|:---:|:---:|
| <img src="face-default.svg" width="120" alt="Myrlet default face"> | <img src="face-working.svg" width="120" alt="Myrlet working face"> | <img src="face-idle.svg" width="120" alt="Myrlet idle face"> | <img src="face-warning.svg" width="120" alt="Myrlet warning face"> | <img src="face-offline.svg" width="120" alt="Myrlet offline face"> |

[Open the side-by-side light/dark expression sheet](status-face-sheet.svg) · [Before/after vector refinement](vector-refinement-comparison.svg).

## Files

- `face-default.svg` and `face-default-inverse.svg`: **byte-for-byte copies** of the approved official mark SVGs, ensuring identical Default geometry.
- `face-working.svg` / `face-working-inverse.svg`: short, inward-tilted eyes.
- `face-idle.svg` / `face-idle-inverse.svg`: shorter, slightly lowered horizontal eyes.
- `face-warning.svg` / `face-warning-inverse.svg`: intentionally mild asymmetry in one eye.
- `face-offline.svg` / `face-offline-inverse.svg`: short, low, subtly drooping eyes.
- `expression-tokens.json`: coordinates and transform parameters relative to the approved 1024×1024 vector trace.
- `motion-spec.md`: design constraints for any future runtime animation and
  accessibility behavior. The current SVG assets are static; this document does
  not imply that animated status faces are implemented or shipped.
- `status-face-sheet.svg`: review-only visual comparison, **not** a product asset.

The identity, outer contour, face/visor, antennae and three connected nodes are identical across all status faces and come from the **refined** `../myrlet-mark.svg`. Default uses the refined official mark with no geometric modification; the four other expressions transform only the two eye cutouts. Each SVG remains a **single even-odd compound vector path with transparent eye cutouts**: no filters, embedded bitmap, or animation.

## State-to-face mapping

| Health | Activity | Face | UI label |
| --- | --- | --- | --- |
| `offline` | any | Offline | Explicitly offline |
| `degraded` | any | Warning | Explicitly degraded |
| `healthy` | `working` | Working | Working |
| `healthy` | `idle` | Idle | Idle |
| `healthy` | `unknown` | Default | Health and activity stated separately |
| `unknown` | any | Default | **Unknown**, never healthy |

Attention is orthogonal: `awaiting_owner` / `human_input_required` require a distinct attention indicator, not a reinterpretation of the Health face.

## Use and limitations

1. **Do not** replace the GitHub avatar, favicon, printed logo or README branding with runtime expressions. Official brand = Default.
2. Do not infer execution completion or termination from Worker connectivity. The icon reflects only a specific, observed resource; Hub and Worker scopes must not be conflated.
3. Always provide accurate status text, accessible semantics and a separate warning/offline indicator where appropriate. Never use eye shapes as the only state channel.
4. Keep status SVGs static; animate only in the product UI, respecting reduced-motion preferences. See `motion-spec.md`.
5. Review exports on both light and dark backgrounds and at **16 / 24 / 32 px** before adopting them as compact platform icons. Subtle expression distinctions are not guaranteed to be perceivable at tiny sizes.
6. The master is now a curve-refined reconstruction of the approved raster, **not** recovered designer-authored Bézier source. Any further optical cleanup must regenerate the entire expression family consistently. Compare [original tracing vs refined vectors](vector-refinement-comparison.svg).
