---
name: mcw-gui-accessibility
description: Review and improve keyboard access, focus, contrast, semantics, scaling, and interaction clarity in MCW Launcher.
---

# MCW GUI Accessibility Skill

## Trigger

Use for accessibility reviews or changes affecting keyboard interaction, focus, labels, contrast, scaling, or motion.

## Read

- `GUI_AGENTS.md`
- `docs/gui/ACCESSIBILITY.md`
- `docs/gui/COMPONENT_GUIDELINES.md`
- `docs/gui/DELIVERY_REPORT.md`

## Procedure

1. Inspect the framework's accessibility and focus APIs.
2. Identify primary workflows and all interactive controls.
3. Test keyboard reachability, focus order, focus visibility, and modal behavior.
4. Check names/roles/states for icon-only and custom controls.
5. Review contrast and non-color status cues.
6. Test scaling, resizing, long labels, and supported themes.
7. Make focused fixes and add tests where possible.
8. Record verified behavior and framework limitations.

## Constraints

Do not claim WCAG conformance or full accessibility from a superficial visual check. State exactly what was tested and what remains unverified.
