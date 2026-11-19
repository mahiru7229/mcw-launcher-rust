# MCW Launcher Rust Edition — Product Design Direction

Status: **APPROVED (v1.0 — Confirmed with Product Owner)**

This file defines MCW Launcher Rust Edition's visual identity and desktop architecture. Treat this document as the source of truth for `apps/desktop` GUI decisions.

## 1. Product character

MCW Launcher should feel:
- **Purpose-built:** a lightweight, native Rust desktop utility for managing and launching game instances, not a generic analytics dashboard.
- **Technical but approachable:** crisp, structured engineering precision without intimidating jargon where simpler wording works.
- **Efficient:** frequent tasks (select instance, launch/stop, manage mods, inspect logs) are immediately visible and executable with pointer or keyboard.
- **Trustworthy:** progress, errors, and destructive consequences are communicated honestly.
- **Distinctive:** a recognizable, sharp technical identity built from a few consistent choices rather than decoration everywhere.

## 2. Visual direction — Confirmed Decisions

- **GUI Framework / Stack:** `egui` (`eframe`) — 100% pure Rust native desktop app (no WebView, low RAM, instant startup, direct access to all Core Rust crates).
- **Overall direction:** Restrained modern desktop utility — crisp, compact technical tool with clear hierarchy and zero visual clutter.
- **Light/dark theme support:** Both supported — **Dark mode default** with an explicit **Light mode toggle** in the top utility bar.
- **Neutral/background palette:** Slate neutral foundation:
  - Dark mode: Canvas `#111317`, Surface `#181B21`, Elevated `#20242C`, Border `#2B303B`, Text `#E6E9EF`, Muted `#8E96A6`.
  - Light mode: Canvas `#F7F8FA`, Surface `#FFFFFF`, Elevated `#EEF0F4`, Border `#D6DBE3`, Text `#181B21`, Muted `#5A6374`.
- **Primary accent:** Restrained Emerald green (`#10B981` in Dark, `#059669` in Light) for primary actions (`Launch`), active selection indicator, and running/ready semantic emphasis.
- **Semantic status colors:** Success `#10B981`, Warning `#D97706`, Danger `#DC2626`, Focus `#34D399` (Dark) / `#059669` (Light). Always paired with text labels, never color alone.
- **Typography family and fallback:** Crisp technical sans-serif loaded from system UI font with full Vietnamese diacritic support (Windows `Segoe UI` / Linux `Noto Sans`, fallback `egui` proportional) + Monospace (`Cascadia Mono` / `Consolas` / `egui` monospace) for Live Logs, file paths, versions, and UUIDs.
- **Shape language:** Sharp, disciplined technical geometry — tight corner radii (`2px–4px` on buttons, inputs, tabs, and containers), crisp `1px` borders, no heavy drop shadows, no glass/blur effects.
- **Icon family:** Minimalist, single-style monochrome geometric/glyph icons paired with clear text labels for all primary controls; no mixed emoji styles.
- **Illustration/mascot policy:** No decorative mascots, hero illustrations, or filler banners. Only authentic product visuals (instance identity / account status).
- **Navigation & layout model:** 2-column **Master–Detail** desktop layout:
  - **Top utility bar:** Active account switcher (Offline & Microsoft OAuth), Dark/Light theme toggle, and update check indicator.
  - **Left sidebar (Master):** Scannable instance list (name, MC version, loader, live status badge) + `+ New Instance` action.
  - **Right pane (Detail):** Selected instance header + primary `Launch` / `Stop` action + honest real-time `ProgressStage` bar + 4 tabs: **Overview**, **Mods** (local `mcw-mods` + Modrinth v2 browser/installer), **Logs** (live monospace stdout/stderr stream), and **Settings** (memory, resolution, Java selection & Temurin provisioning).

## 3. Visual principles

- Use hierarchy before decoration.
- Let instance identity, version, status, and primary action be easy to recognize.
- Use color semantically and consistently.
- Make technical detail available without forcing it into every user's primary path.
- Prefer a coherent visual rhythm over individually flashy components.
- Use expressive elements selectively where they strengthen product identity.
- A little warmth and personality is welcome; visual noise is not.

## 4. Desktop-first behavior

- Optimize primary workflows for desktop pointer and keyboard use.
- Keep important actions visible at common laptop window sizes (minimum sensible width ~840×560, default ~1040×680).
- Reflow gracefully when the window is resized or display scaling changes.
- Use compact, scannable layouts for instance management and clearer spacing for setup dialogs.
- Avoid mobile-style oversized controls.

## 5. Signature moments

MCW Launcher expresses its restrained technical personality in two deliberate places:
1. **Selected & Running Instance Accent Bar:** Sharp `2px–4px` instance list rows featuring a crisp left vertical Emerald accent bar indicating the active/running instance.
2. **Truthful Launch Status Strip:** A clean, sharp status & progress strip in the detail header transitioning honestly through `Preparing` → `Downloading` → `Running` → `Finished`/`Crashed`, using Emerald emphasis purposefully without decorative animation.

Do not spread decorative motifs across every surface.

## 6. What MCW should avoid

- Generic SaaS dashboard styling unrelated to launcher workflows.
- Trend-driven glass/blur/gradient effects without a role.
- Excessive cards, pills, badges, and status colors.
- Decorative statistics or fake progress indicators.
- Random mixtures of icon styles and emoji.
- Huge empty hero areas that push useful controls below the fold.

## 7. Approval rule

Changes to foundational visual identity — palette, typography, icon system, navigation model, or global component language — require product-owner review before being treated as the new standard.
