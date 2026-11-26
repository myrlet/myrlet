# Myrlet Status Face — Motion Specification

The SVG assets are **static, transparent Bézier vector assets**, reconstructed from the approved silhouette. Do not use animated GIFs or independently redraw five ant silhouettes. Only the eye cutouts change. Runtime animation, if introduced, must follow the guidance below.

## Runtime motion guidance

- Interpolate the two eye shapes from `expression-tokens.json` on **observed** state transitions; keep all identity/collaboration geometry fixed.
- Duration: **200 ms** (acceptable design range: 150–250 ms); easing: `cubic-bezier(0.2, 0.7, 0.2, 1)`.
- Do not crossfade complete logos in a way that doubles the silhouette. Prefer interpolating eye vertices or the numeric eye transform.
- **Working**: shift once to inward-tilted, shortened eyes. No repetitive working animation.
- **Idle**: static by default; an infrequent blink may be added after usability review. A blink must not imply a change of observed activity.
- **Warning / Offline**: static, never flash. A disconnected Worker is not evidence that its previously launched execution has stopped.
- **Reduced motion** (`prefers-reduced-motion: reduce`, or equivalent native setting): apply the new static state immediately; disable optional blinking and nonessential transitions.
- Unknown health or activity is never treated as healthy. Keep the semantic text label visible. Attention (`awaiting_owner`, `human_input_required`) is an independent indicator.

## Asset and component boundaries

**Brand mode:** `../myrlet-mark.svg` / `../myrlet-mark-inverse.svg`, always Default.

**Product status mode:** choose one of the five `face-*.svg` assets for the scoped Hub/Worker/Agent observation; present semantic text alongside it. For dark surfaces use the `-inverse` file.

**Compact mode:** prioritize an identifiable mark and explicit status badge or label. At 16–32 px eye differences may be too small to perceive reliably.

Static assets carry no health metadata and cannot independently determine a system state. Frontends should map known status from their own observation model; the Hub must not invent additional operational states for animation.

## Visual design constraints

No mouth, eyebrows, X eyes, emoticons, added color, changes to antennae, visor or collaboration nodes. The official default brand mark remains unchanged. The eye areas are **transparent cutouts**, so every variant works on non-white surfaces when its fill color contrasts with the background.
