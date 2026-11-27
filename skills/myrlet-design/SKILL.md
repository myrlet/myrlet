---
name: myrlet-design
description: Design, implement, refine and review Myrlet's native Rust/NSH application UI using its Quiet Precision design system and official NSH requirements.
---

# Myrlet Design

Use this skill for any visual or interaction work in apps/app/: new screens, layout changes, component work, visual polish, motion, accessibility, UI refactors and design reviews.

## Mandatory references

Read these repository files first:

- docs/design/ui-system.md — Myrlet's normative visual and interaction system.
- docs/architecture/README.md — canonical V1 product boundaries and Explicit V1 Non-Goals.

Then load the official NSH skills:

- nsh-design-guides: https://github.com/longbridge/nsh/tree/main/skills/nsh-design-guides
- nsh: https://github.com/longbridge/nsh/tree/main/skills/nsh

The official NSH guides are requirements, not suggestions. If the skills are unavailable, read their source files and the relevant pages at https://nsh.dev/llms.txt. Verify every API against NSH documentation or the current project declarations; never infer it from React or browser UI libraries.

Impeccable (https://github.com/pbakaus/impeccable) can provide optional critique/refinement commands such as critique, distill and polish when installed. Its output is advisory and must not override NSH behavior, Myrlet product semantics or the repository design system.

## Design workflow

1. Inspect the existing screen, its neighboring components, the current theme and the state owner.
2. State the primary user task, object, authoritative data source, component composition and keyboard/pointer path.
3. Follow **Quiet Precision**: calm, precise, dense-but-readable, native and truthful. Prefer hierarchy and alignment over decoration.
4. Reuse src/theme/ and src/ui/. Do not add raw colors, arbitrary radii, ad-hoc typography or page-specific variants of shared controls.
5. Before coding, write the screen's motion inventory: presses, selections, content arrival/removal, overlays, layout changes, loading and live states; include reduced-motion behavior.
6. Keep the change within the requested scope. Do not turn a narrow UI fix into a wholesale redesign or platform rewrite.
7. Add deterministic behavior tests. Keep fixtures or component previews under `#[cfg(test)]` only; never bundle them in production builds.
8. Run the relevant Rust and NSH checks, then inspect the actual macOS/Web result in Porcelain and Graphite. Capture screenshots at representative desktop and narrow widths.
9. Report what was actually verified and what remains unavailable. Headless tests do not prove visual quality or native input behavior.

## Product truth

Use the [UI data semantics](../../docs/design/ui-system.md#reading-proposed-workflow-facts) when designing for the proposed agent workflow. A wakeup leaves the Task queued. Only a committed Delivery establishes an assignment, and the Hub copy may lag the Worker.

Never invent or infer Task, Delivery, Execution, Worker, progress, authorization, or process-lifecycle facts. Preserve stale, missing, and uncertain states explicitly. Do not turn missing progress into a percentage or `finished` / `cancelled` into a success/failure verdict.

## Review output

Review in this order:

1. Product task and information hierarchy.
2. Domain truth and state ownership.
3. NSH component/API correctness and platform behavior.
4. Reuse of semantic tokens and shared components.
5. Keyboard, focus, accessibility and all reachable states.
6. Motion and reduced-motion behavior.
7. Actual target screenshots and verification evidence.

Classify findings under the repository's P0–P3 review policy. A visually plausible screenshot is not proof that keyboard, focus, responsive layout, dynamic content or failure states work.
