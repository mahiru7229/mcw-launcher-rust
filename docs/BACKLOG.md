# MCW Launcher: Rust Edition - Backlog (updated 2026-11-21, v0.14.0-beta.1)

Parity checklist (Guide Section 7.1-7.8), UI v2.0 rebuild (`v0.11.0-beta.1` - `v0.13.2-beta.1`), and full Python-parity / release-readiness items (`v0.14.0-beta.1`) are complete. Remaining post-publish / maintenance items:

## 1. Release readiness
- [x] Sync `README.md` to current release (`v0.14.0-beta.1`), UI v2.0 features, and `UPDATES_ENABLED = false` status.
- [x] Backfill `CHANGELOG.md` entries across all 15 releases (`0.1.0-alpha.1` -> `0.14.0-beta.1`).
- [x] Copy `SECURITY.md` from the Python repo.
- [x] Copy `assets/icons/` (`mcw_launcher.ico`, `mcw_launcher.png`); add `apps/desktop/build.rs` for Windows `.ico` + version metadata and runtime `ViewportBuilder::with_icon`.
- [x] `release.yml`: package `themes/` next to binaries and gate releases strictly on created GitHub Releases.
- [ ] Push to GitHub, verify CI on `ubuntu-latest` + `windows-latest`, and create official GitHub Releases for `mahiru7229/mcw-launcher-rust`.
- [ ] Enable Rust-Edition-only auto-updater (`UPDATES_ENABLED = false` today) once GitHub Releases for `mahiru7229/mcw-launcher-rust` are live.

## 2. Optional / Python-parity features (Completed in v0.14.0-beta.1)
- [x] UI OS window for `InstalledContentLibrary` (`mcw_vp_content_library` with search/filters/Pin/Open Folder).
- [x] In-place Modrinth `.mrpack` update check & safe upgrade (`ModpackUpdateService` + `mcw_vp_modpack_update`).
- [x] Interactive Forge/NeoForge Preflight compatibility confirmation (`mcw_vp_preflight_confirm`).
- [x] Sanitized `IssueReportBuilder` & `Report an Issue` OS window (`mcw_vp_issue_report`).
- [x] `DownloadJournal` persistence (`crates/mcw-net/src/journal.rs`) and `ConnectivityMonitor` Offline indicator.
- [x] External `ThemeIssues` diagnostics (`mcw_vp_theme_issues`) and `UnsavedChanges` guard for Instance Settings (`mcw_vp_unsaved_changes`).

## 3. SOLID refactor (SRP - ongoing maintenance)
- [x] Extract pages, unified Content Browser, Settings modal, and Stage 6 windows from `dialogs.rs` and `workspace.rs`.
- [ ] Further split remaining legacy dialogs in `apps/desktop/src/app/views/dialogs.rs` (`~2.4k` lines) and `apps/desktop/src/app/controllers.rs` (`~4.9k` lines) as needed cleanly.
