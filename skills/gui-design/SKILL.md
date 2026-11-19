---
name: mcw-gui-design
description: Design and implement a focused, coherent MCW Launcher desktop GUI change using the project's approved visual direction and quality gates.
---

# MCW GUI Design Skill

## Trigger

Use when implementing a new screen, changing layout, styling components, or modifying a GUI workflow.

## Read

- `GUI_AGENTS.md`
- `DESIGN.md`
- `docs/gui/ANTI_SLOP_CORE.md`
- Relevant component, UX, desktop layout, and accessibility references only.

## Procedure

1. Inspect the actual framework, relevant screen, shared components, and state ownership.
2. Identify user goal, context, primary action, and edge states.
3. Check whether `DESIGN.md` has enough direction. If not, keep choices conservative and mark the result as a draft.
4. Propose the smallest coherent design change.
5. Implement with shared tokens/components.
6. Verify real data, long text, empty/loading/error/success states.
7. Check keyboard, focus, resizing, and supported themes.
8. Apply the anti-slop purpose test to every prominent treatment.
9. Complete `docs/gui/DELIVERY_REPORT.md`.

## Constraints

Do not create a generic dashboard, fake metrics, decorative progress, inert controls, or unapproved global design changes. Effects are permitted when justified.
