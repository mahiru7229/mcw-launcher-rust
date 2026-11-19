---
name: mcw-gui-audit
description: Audit existing MCW Launcher screens for usability, consistency, accessibility, visual quality, and anti-slop issues without immediately changing code.
---

# MCW GUI Audit Skill

## Trigger

Use when asked to review, critique, inspect, or improve an existing interface.

## Read

- `GUI_AGENTS.md`
- `DESIGN.md`
- `docs/gui/ANTI_SLOP_CORE.md`
- `docs/gui/VISUAL_AUDIT.md`
- `docs/gui/ACCESSIBILITY.md` where relevant.

## Procedure

1. Establish scope and inspect the running UI or available evidence.
2. Review actual user journeys, not just isolated screenshots.
3. Record observable defects and evidence.
4. Separate functional/usability defects from subjective visual opportunities.
5. Assign severity and confidence.
6. Return a prioritized findings list and proposed minimal fixes.
7. Wait for approval before broad redesign.
8. After approved fixes, re-run the audit and complete the delivery report.

## Constraints

Do not equate personal taste with a defect. Do not infer untested behavior from screenshots. Do not modify code during findings-only audit mode.
