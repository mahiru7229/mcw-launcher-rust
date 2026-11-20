# MCW Launcher: Rust Edition — Backlog (updated 2026-11-21, after v0.13.0-beta.1)

Parity checklist (Guide §7.1–7.8) and UI v2.0 rebuild (`v0.11.0-beta.1` – `v0.13.0-beta.1`) are complete. Remaining future items:

## 1. Release readiness
- [x] Sync `README.md` to current release (`v0.13.0-beta.1`), UI v2.0 features, and `UPDATES_ENABLED = false` status.
- [ ] Backfill `CHANGELOG.md` entries for intermediate parity milestones (`0.6.0` → `0.10.0`).
- [ ] Copy `SECURITY.md` from the Python repo.
- [ ] Copy `assets/` (logo/icon); add `build.rs` for Windows `.ico` + version metadata in `apps/desktop`.
- [ ] `release.yml`: also package `themes/` next to the exe (lang + LAN agent jar are already embedded).
- [ ] Push and verify CI on `ubuntu-latest` + `windows-latest` (tags `v0.8.0-beta.1` … `v0.13.0-beta.1` are local only).
- [ ] Enable Rust-Edition-only auto-updater (`UPDATES_ENABLED = false` today) once GitHub Releases for `mahiru7229/mcw-launcher-rust` exist.

## 2. Optional / advanced
- [ ] UI for `InstalledContentLibrary` (core exists in `crates/mcw-mods/src/library.rs`, no desktop window yet).
- [ ] Remaining i18n coverage where Python dictionary keys are later expanded (Shared Content Store card, `Export Log...`, etc.).
- [ ] `DownloadJournal` (resume downloads across sessions) in `mcw-net`.
- [ ] Startup splash; "unsaved changes" warning in settings.

## 3. SOLID refactor (SRP)
- [x] Extract pages, unified Content Browser, and Settings modal from `dialogs.rs` (`~4.6k` → `~2.2k` lines) and `workspace.rs` (`~2.3k` → `~1.6k` lines).
- [ ] Further split `apps/desktop/src/app/views/dialogs.rs` (`~2.2k` lines) into focused dialog modules (create/export, version picker, account, manual downloads).
- [ ] Split `apps/desktop/src/app/controllers.rs` (`~4.1k` lines).

## 4. UI v2.0 rebuild (`DESIGN.md`)
- [x] `v0.11.0-beta.1` — Foundation + Shell (background picture, opacity/dimming, presets, top pill nav, icon rail, instance flyout, in-app Settings modal).
- [x] `v0.12.0-beta.1` — Pages (Home tabbed workspace, OS Content Browser w/ Data Packs, Servers/LAN, Versions, unified Logs).
- [x] `v0.13.0-beta.1` — Polish (`Ctrl+K` Global Quick Search, full dialog/card/badge radius & Esc consistency, animated task progress bars, dead UI code cleanup, docs sync).
