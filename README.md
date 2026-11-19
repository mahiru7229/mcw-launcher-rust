# MCW Launcher Rust Edition — GUI Design System v2.0

A combined, project-oriented GUI quality system inspired by the ideas behind [miqdadbadjuber/Anti-Slop](https://github.com/miqdadbadjuber/Anti-Slop) and the previous MCW GUI Design Guidelines v1.0.

This pack is an original set of instructions for MCW. It does not reproduce the upstream repository's rule text or skills. It adopts compatible high-level practices: a filter rather than a prescriptive visual style, a separate product design direction, task-specific skills, two review modes, and an explicit delivery report.

## Install

Copy the contents of this folder into the root of the MCW Launcher Rust Edition repository. If a file already exists, merge deliberately; do not overwrite project-specific instructions blindly.

Suggested placement:
- `GUI_AGENTS.md` — root-level GUI agent instructions.
- `DESIGN.md` — root-level product visual direction. Treat as the source of truth for visual identity.
- `docs/gui/` — design system and review references.
- `skills/` — task-specific agent workflows.

Add a short pointer to `GUI_AGENTS.md` from the repository's `AGENTS.md` or the agent-specific instruction file you actually use. Do not automatically load every reference file in every session.

## Contents

- `GUI_AGENTS.md` — workflow and mandatory quality gates.
- `DESIGN.md` — MCW visual direction template; must be completed with real product decisions.
- `docs/gui/ANTI_SLOP_CORE.md` — general quality filter and rule tiers.
- `docs/gui/GUI_DESIGN.md` — information hierarchy and desktop UX principles.
- `docs/gui/DESIGN_SYSTEM.md` — tokens, typography, color, density, component states.
- `docs/gui/COMPONENT_GUIDELINES.md` — controls, navigation, forms, lists, dialogs.
- `docs/gui/UX_FLOWS.md` — launcher workflows and operation states.
- `docs/gui/ACCESSIBILITY.md` — keyboard, contrast, semantics, scaling.
- `docs/gui/DESKTOP_LAYOUT.md` — desktop-specific layout and responsive resizing.
- `docs/gui/VISUAL_AUDIT.md` — audit mode and prioritized findings.
- `docs/gui/COPY_AND_MICROCOPY.md` — concise, useful interface language.
- `docs/gui/ASSET_GUIDELINES.md` — icons, illustration, texture, licensing.
- `docs/gui/DELIVERY_REPORT.md` — PASS/FAIL report template.
- `skills/gui-design/SKILL.md` — implementation-time design skill.
- `skills/gui-audit/SKILL.md` — audit-existing-UI skill.
- `skills/gui-accessibility/SKILL.md` — accessibility review skill.

## Important project caveat

These documents are framework-agnostic. They do not assume egui, iced, Slint, Tauri, GTK, Qt, or any other GUI technology. The agent must inspect the actual Rust Edition repository and its current GUI stack before implementation.

`DESIGN.md` contains deliberate TODOs. Do not let an agent invent MCW's brand direction. Complete those decisions with screenshots, references, or explicit product-owner choices.

## Two operating modes

- **During implementation:** apply the filter while designing and coding, then complete the delivery report.
- **After implementation:** audit the existing interface, report prioritized findings, wait for approval before broad changes, then re-audit.

## Relationship to upstream Anti-Slop

Upstream project: https://github.com/miqdadbadjuber/Anti-Slop  
This pack is an independently authored adaptation for MCW Launcher. Consult the upstream project for its latest skills, installation options, and license. Do not assume this pack is a synchronized copy or official distribution of Anti-Slop.
