# MCW Launcher: Rust Edition — Backlog (saved 2026-11-20, after v0.10.0-beta.1)

Parity checklist (Guide §7.1–7.8) is 100% complete. Remaining items, to be done later:

## 1. Release readiness
- [ ] Sync docs: `README.md` (still says v0.6.0-beta.1, old updater text), `CHANGELOG.md` (only up to 0.5.0-beta.1; add 0.6.0 → 0.10.0).
- [ ] Copy `SECURITY.md` from the Python repo.
- [ ] Copy `assets/` (logo/icon); add `build.rs` for Windows `.ico` + version metadata in `apps/desktop`.
- [ ] `release.yml`: also package `themes/` next to the exe (lang + LAN agent jar are already embedded).
- [ ] Push and verify CI on `ubuntu-latest` + `windows-latest` (tags v0.8.0-beta.1 … v0.10.0-beta.1 are local only).
- [ ] Enable Rust-Edition-only auto-updater (`UPDATES_ENABLED = false` today) once GitHub Releases for `mahiru7229/mcw-launcher-rust` exist.

## 2. Optional / advanced
- [ ] UI for `InstalledContentLibrary` (core exists in `crates/mcw-mods/src/library.rs`, no desktop window yet).
- [ ] i18n for remaining hard-coded English labels (Shared Content Store card, `Export Log...`, OptiFine section, etc.).
- [ ] `DownloadJournal` (resume downloads across sessions) in `mcw-net`.
- [ ] Startup splash; "unsaved changes" warning in settings.

## 3. SOLID refactor (SRP)
- [ ] Split `apps/desktop/src/app/views/dialogs.rs` (~4.6k lines).
- [ ] Split `apps/desktop/src/app/controllers.rs` (~4.3k lines).
- [ ] Split `apps/desktop/src/app/views/workspace.rs` (~2.3k lines).

## 4. UI rebuild (in discussion — "Horizon"-style reference screenshots)
- Direction: dark pixel-landscape background, floating rounded translucent panels, top pill nav, icon rail + instance flyout, side detail panel in lists, Settings as centered modal with left category list.
- Constraint from owner: **only simple animations** (no heavy/continuous motion).
- Note: conflicts with current `DESIGN.md` (no gradients/glass, sharp 2–4px radii, 2-column master-detail) → `DESIGN.md` must be updated on approval.
