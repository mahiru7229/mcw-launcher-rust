# Changelog

All notable changes to **MCW Launcher: Rust Edition** are documented in this file.

## [1.3.2] - 2026-11-26

### Added
- **3D Skin & Cape Manager (`mcw-auth`, `mcw-desktop`)**:
  - Added a pure-Rust Z-buffered 3D character & cape software rasterizer (`render_skin_3d_rgba`) supporting mouse-drag yaw/pitch rotation, outer overlay layer toggles (`Hat`, `Jacket`, `Sleeves`, `Pants`), Classic (4px arms) vs Slim (3px arms), modern `64x64` and legacy `64x32` skin layouts, and active cape previews.
  - Added an **In-App Centered Modal** (`3D Skin & Cape Manager`) accessible from the **Accounts** modal, backed by `<app_root>/skins/` local skin library management (import/list/delete PNGs) and full **Mojang Profile API** integration (`fetch_full_profile`, multipart `upload_skin`, `set_active_cape`, `hide_cape`) for Microsoft accounts.
- **Inline World Data Packs Manager (`mcw-minecraft`, `mcw-desktop`)**:
  - Added per-world **`Data Packs`** management directly inside the `Worlds` tab with `← Back to Worlds`, **`+ Add Data Pack`** (opening the locked inline Data Packs browser targeting the selected world), **Import Local `.zip`...**, **Open `datapacks/` Folder**, sliding enable/disable switches (`.zip` <-> `.zip.disabled`), and delete actions.
- **Prism-Inspired UX & Navigation Conveniences (`mcw-minecraft`, `mcw-desktop`)**:
  - Added a crisp, scannable **QR Code** (`qrcode`) alongside the Microsoft Device Login code & verification URL in both the Accounts modal and First-Run wizard.
  - Added direct desktop **Shortcut** creation buttons on **World** and **Server** cards (for Minecraft `>= 1.20` Quick Play) generating `--launch <id> --world <folder>` and `--server <ip>` shortcuts.
  - Enhanced global **Drag & Drop** handling with a translucent drop-target overlay and automatic routing for `.mrpack` / `.mcwpack` modpacks, `.jar` mods, `.png` (`64x64` / `64x32`) skins into `<app_root>/skins/`, and smart `.zip` inspection (`ZipContentKind`: worlds, data packs, shaders, resource packs, modpacks).

### Improved & Restored
- **MCW Security Bar Inline Verification & Windows Signing Infrastructure (`mcw-minecraft`, `mcw-desktop`, `.github/workflows/release.yml`, `docs/security/WINDOWS_SIGNING.md`)**:
  - Added an inline **`Sync & Verify`** button directly on the Workspace Security Status bar (auto-hidden when `PROTECTED`) and removed duplicated status label prefixes so the full security description reads cleanly.
  - Added Windows PE `VERSIONINFO` metadata in `apps/desktop/build.rs` and `apps/updater/build.rs`, generated `SHA256SUMS.txt` release manifests, wired optional SignPath Foundation code-signing in `.github/workflows/release.yml`, and documented Windows SmartScreen / Authenticode trust preparation in `docs/security/WINDOWS_SIGNING.md`.
- **Transactional Repair Center Rebuild (`mcw-minecraft`, `mcw-desktop`)**:
  - Rebuilt `RepairService::repair_instance` with transactional backup/rollback staging across Vanilla, Fabric, Quilt, Forge, and NeoForge—re-verifying client JARs, libraries, and assets, clearing `last_launch_crashed`, and strictly preserving `saves/`, `mods/`, and `options.txt`—alongside the restored footer **`Repair`** button and live progress bar inside *Repair Center & Health*.

---

## [1.3.1] - 2026-11-25

### Security
- **Two-Layer Minecraft2Shell Security Hotfix (`mcw-runtime`, `mcw-minecraft`, `mcw-desktop`, `tools/mcw_security_agent`)**:
  - **Layer 1 — Mojang FreeType Library Sync (`MojangLibrarySync`, `LibraryManager`, `VersionManifestService`)**: Refreshes official `version_manifest_v2.json` and verifies SHA-1 checksums to detect and apply Mojang's patched **`org.lwjgl:lwjgl-freetype:3.4.3-mojang-2.15.0`** (`FreeType 2.15.0`, shipped on `26.3` and `26.4-pre-1`), cleaning stale FreeType natives while leaving unrelated dependencies untouched and providing full transactional backup + automatic rollback on failure or offline interruption.
  - **Layer 2 — Independent Runtime Mitigation (`MCW Security Agent` / `mcw-security-agent.jar`)**: Pure-Java `--release 8` instrumentation agent attached automatically **only** to instances with unpatched `lwjgl-freetype` (`1.20.5`–`1.21.11`, `26.1`–`26.2`), forcing `FT_LOAD_NO_HINTING` (`0x2`) across `nFT_Load_Glyph`, `nFT_Load_Char`, `FT_Load_Glyph`, and `FT_Load_Char` to block `Ins_SHZ` hinting bytecode exploits. Emits a verified runtime status handshake (`mcw-security-agent-status.json`: `INITIALIZED` -> `ACTIVE_VERIFIED` / `INCOMPATIBLE` / `FAILED`), validates Java 8+ compatibility, omits automatically once patched Mojang FreeType is verified on disk, and supports clean independent uninstallation.
- **MCW Security Manager (`SecurityManager`)**:
  - Evaluates merged dependencies, on-disk SHA-1s, Java runtime compatibility, runtime verification status, and policy into 4 transparent states (**`PROTECTED`**, **`PARTIALLY_PROTECTED`**, **`UNPROTECTED`**, **`UNKNOWN`**) always accompanied by a clear human-readable description in Workspace, *Launcher Settings -> Security*, Launch Logs, and Pre-Launch confirmation prompts.
- **Signed Remote Security Policy (`SecurityPolicyService`, `ring` Ed25519)**:
  - Verifies remote policy metadata envelopes with Ed25519 signatures and explicit expiration checks (pure declarative metadata—no remote code execution) with offline baseline fallback (`3.4.3-mojang-2.15.0` verified SHA-1s).
- **Pre-Launch Server Resource Pack Warning Modal (`mcw_modal_security_rp_confirm`)**:
  - Scans `servers.dat` before launching FreeType-enabled versions for `acceptTextures = 1` auto-accept servers and shows the instance's full security assessment description, per-server **Delete Server**, **I know what I'm doing (Trust & Don't Ask Again)**, and **Cancel Launch** controls.

### Added
- **Security Architecture & Lifecycle Documentation (`docs/security/`)**: Added `README.md`, `THREAT_MODEL.md`, `SECURITY_AGENT.md`, `LIBRARY_SYNC.md`, `LIFECYCLE.md`, and `TEST_PLAN.md`.
- **JSON `$schema` Support (`schemas/`, `mcw-models`, `mcw-desktop`)**:
  - Added Draft-07 JSON Schemas (`schemas/launcher-settings.schema.json`, `schemas/instance-settings.schema.json`, `schemas/instance.schema.json`) and automatic `$schema` fields across `config/launcher_settings.json`, `instances/<id>/settings.json`, and `instances/<id>/instance.json`.

---

## [1.3.0] - 2026-11-25

### Highlights
- **All-in-One Mod & Modpack Ecosystem (`mcw-providers`, `mcw-mods`, `mcw-desktop`)**:
  - Added **Technic Platform & Solder API** modpack browsing, installation, and updates alongside **Modrinth**, **CurseForge**, **FTB**, and **ATLauncher** (5 online providers), plus **Prism Launcher / MultiMC `.zip`** import and **Universal Launcher Importer** (read-only migration from Prism/MultiMC, Modrinth App, CurseForge App, and Vanilla `.minecraft`).
  - Added **Modrinth-style Inline Project Detail View** in the Content Browser (`← Back to Results`, categories/loaders/compatibility chips, inline Markdown formatting with clickable links, CurseForge creator attribution, and a clickable **Gallery** thumbnail strip opening an **In-App Image Preview Modal** `mcw_modal_gallery_preview`).
  - Added **Multi-Mod Download Cart** (`+ Cart` -> `Review & Confirm`), **Bulk Mod Management** with status/search filters and sliding **Enabled/Disabled** toggle switch, **Resource Pack & Shader `pack.png` Icons + Modrinth Updates**, and **Selective File Tree** exports.
- **Workspace AIO, Accounts & Quick Launch (`mcw-minecraft`, `mcw-desktop`)**:
  - Added collapsible **Library Grid View** on `Home`, quick top-bar **Account Switcher** + per-instance **Preferred Account** override, `--launch` / `--world` / `--server` direct launch CLI flags, and **Create Desktop Shortcut** (`.lnk` / `.desktop`).
  - Added **Instance Notes (`notes.txt`)**, visual **Screenshot Gallery & Preview Modal**, World **`icon.png`** / **Copy Seed** / **Export `.zip`**, and **Export Modlist...** (`.md` / `.txt`).
- **Networking, Appearance & Diagnostics (`mcw-net`, `mcw-minecraft`, `mcw-runtime`, `mcw-desktop`)**:
  - Added **HTTP & SOCKS5 Proxy Settings** (`reqwest` `"socks"`), **Crash Auto-Log**, `All` / `INFO` / `WARN` / `ERROR` **Log Level Filter**, and automatic **Crash Cause Summary** diagnostics.
  - Added centered HSV **In-App Accent Color Picker Modal** (`mcw_modal_color_picker`), unified **`Apply` / `Reset Defaults`** typography controls (`UI Font`, `UI Scale`, `Font Size`), conditional **`Show Snapshots`** checkbox for Vanilla instances, remote-only **GitHub Changelog** loading, and **Asset Cache Zero-Rewrite Fast Path** (including 0-byte objects and cached vs missing progress).
- **Final Pre-Release Fixes & Temporary Removals**:
  - Left-aligned card summary labels in Content Browser, decoded extended HTML typography entities (`&ensp;`, `&emsp;`, `&thinsp;`, `&mdash;`, `&ndash;`, `&hellip;`), fixed overlapping buttons in the **About** modal footer, and cleared stale search filters across category/tab switches.
  - **Temporarily Removed**: The **`Repair`** action button in *Repair Center & Health* has known issues and has been temporarily removed (keeping `Refresh` and `Close`) so users can act on diagnostics directly; it will return fixed in a future update.

---

## [1.3.0-beta.4] - 2026-11-25

### Added
- **Modrinth-Style Inline Detail View & Rich Provider Metadata (`mcw-providers`, `mcw-desktop`)**:
  - Clicking any card in **Mod & Modpacks** (or an instance inline browser) now opens a Modrinth-style **Inline Detail View** inside the main results frame with `← Back to Results`, icon, categories/loaders/compatibility chips, action buttons (**Install**, **`+ Cart`**, **Open Web Page**, **Source**, **Issues**), an async full overview preview, and a horizontal **Gallery** thumbnail strip.
  - Added `ModrinthClient::get_project` (`ModrinthProjectDetail`, `ModrinthGalleryImage`) and `CurseForgeClient::get_mod_description` (`strip_html_to_preview`), and populated **CurseForge creator attribution** (`by <author>` via `CurseForgeMod::primary_author`).
- **In-App Accent Color Picker Modal (`mcw-desktop`)**:
  - Added a centered HSV **Accent Color Picker** modal (`mcw_modal_color_picker`) opened by clicking the accent swatch or **`Pick Color...`** in *Launcher Settings -> Appearance*, with two-way `#RRGGBB` hex sync and quick color presets.
- **Conditional Vanilla Snapshots in Instance Settings (`mcw-desktop`)**:
  - Added a **`Show Snapshots`** checkbox next to Target Minecraft & Mod Loader when **Mod Loader** is set to **Vanilla** (`edit_runtime_show_snapshots`), mixing snapshot versions cleanly into the Minecraft Version `ComboBox` when enabled.

### Changed & Temporarily Removed
- **Mods Tab Sliding Toggle Switch (`mcw-desktop`)**: Replaced the plain `Enabled`/`Disabled` text button on mod cards in the Mods tab with a sliding pill toggle switch + explicit state label (`render_mod_toggle_switch`) to avoid action-vs-status confusion.
- **Unified Typography `Apply` / `Reset Defaults` (`mcw-desktop`)**: Staged `UI Font`, `UI Scale`, and `Font Size` in *Launcher Settings -> Appearance* with live `Preview` sizing and explicit **`Apply`** / **`Reset Defaults`** buttons so slider drags no longer shift UI layout underneath the cursor.
- **Remote-Only GitHub Changelog (`mcw-desktop`)**: Removed the compile-time `include_str!("../../../CHANGELOG.md")` embed so `CHANGELOG.md` always loads directly from GitHub Raw on opening the Changelog page, with clear loading/offline retry UX.
- **Temporary Removal of Repair Button (`Repair Center & Health`)**: The **`Repair`** action button inside *Repair Center & Health* currently has known issues and has been temporarily removed (alongside leaving `Refresh` & `Close` intact) so users can follow diagnostic messages directly; repair automation will return fixed in a future update.

### Fixed
- **Asset Cache Verification & Zero-Rewrite Fast Path (`mcw-minecraft`)**: Fixed `AssetManager::ensure_assets` (`collect_missing_asset_requests`) to reuse cached 0-byte objects alongside non-empty assets, skip `download_batch` completely with zero SSD writes when all assets are present, and display explicit `<cached> cached` vs `<missing> missing` progress.
- **Content Browser Truncation & Tab Search Reset (`mcw-desktop`)**: Prevented long mod descriptions from pushing into the download count and `+ Cart` button, and automatically cleared stale search filters when switching Mod & Modpacks categories or instance tabs.
- **About Modal Footer Responsive Layout (`mcw-desktop`)**: Fixed button overlap between **`Report Issue...`** and **`Close`** inside the About modal footer.

---

## [1.3.0-beta.3] - 2026-11-25

### Added
- **HTTP & SOCKS5 Proxy Settings (`mcw-net`, `mcw-desktop`)**:
  - Enabled the `reqwest` `"socks"` feature and added global proxy configuration (`ProxyConfig`, `apply_global_proxy`, `set_global_proxy_config`) across launcher HTTP clients.
  - Added **HTTP** and **SOCKS5** proxy controls (`None`, `HTTP`, `SOCKS5`, host, port, optional username/password) inside **Launcher Settings -> Downloads**.
- **Resource Pack & Shader `pack.png` Icons + Modrinth Update Checker (`mcw-mods`, `mcw-providers`, `mcw-desktop`)**:
  - Added `pack.png` thumbnail extraction (`read_pack_icon_png`) for Resource Packs and Shader Packs (`.zip` archives and extracted folders) with cached desktop preview thumbnails.
  - Added `ContentPackUpdateCandidate` and `ModUpdateManager::{check_content_pack_updates, apply_content_pack_updates}` backed by Modrinth version lookup, exposed in the Resource Packs & Shaders tabs.
- **Crash Auto-Log, Log Level Filter & Crash Cause Summary (`mcw-runtime`, `mcw-desktop`)**:
  - Added `CrashCauseSummary` and `analyze_crash_cause` to diagnose common Minecraft crash patterns (`OutOfMemoryError`, missing dependency/class, Java version mismatch, LWJGL/OpenGL, Mixin failure).
  - Added **Auto-open Logs on Crash** (`auto_open_logs_on_crash` in *Launcher Settings -> Updates / General* and Logs controls), an `All` / `INFO` / `WARN` / `ERROR` `ComboBox` level filter (`matches_log_level`), and an inline crash cause summary banner in the Logs tab.
- **Universal Launcher Importer (`mcw-storage`, `mcw-desktop`)**:
  - Added read-only auto-detection, inspection, and selective copy import (`ExternalLauncherKind`, `detect_default_launcher_roots`, `inspect_external_launcher`, `import_external_instances`) supporting **Prism Launcher / MultiMC**, **Modrinth App**, **CurseForge App**, and **Vanilla `.minecraft`**.
  - Added the centered **In-App Universal Launcher Importer** modal (`mcw_modal_universal_import`) accessible from **Create Instance -> Modpacks** and **Launcher Settings -> Storage**.

---

## [1.3.0-beta.2] - 2026-11-25

### Added
- **Multi-Mod Download Cart ("Select for Download" -> "Review & Confirm") (`mcw-desktop`)**:
  - Added **`+ Cart`** / **`In Cart ✓`** toggles on **Modrinth** and **CurseForge** mod search results and detail panes, paired with a **`Review & Confirm (N)`** toolbar strip.
  - Added the centered **In-App Mod Cart Modal** (`mcw_modal_mod_cart`) with a non-Vanilla target instance `ComboBox`, asynchronous compatible-version and recursive required-dependency preview (`start_resolve_mod_cart_preview`), per-item issue display, and tracked batch installation (`start_install_mod_cart`) updating `ModProvenanceRegistry`.
- **Bulk Mod Management & Status/Search Filtering (`mcw-desktop`)**:
  - Added instant search filtering (`name`, `file_name`, `mod_id`) and a `ComboBox` status filter (`All`, `Enabled`, `Disabled`, `Updates Available`, `Incompatible`) in the Mods tab (`matches_mod_filter`).
  - Added multi-select checkboxes, **Select Visible**, **Clear Selection**, and a bulk action bar supporting batch **Enable**, **Disable**, **Lock**, **Unlock**, and **Remove** alongside individual **`Enabled`/`Disabled`** toggle buttons.
- **Collapsible Library Grid View on `Home` (`mcw-desktop`)**:
  - Added a persisted **View Mode** switcher (`Workspace View` / `Library Grid`) and sort preference (`Last Played`, `Playtime`, `Name`) in `LauncherPrefs` (`sort_instances_for_grid`).
  - Added a responsive multi-column card grid organized by collapsible instance groups (`Ungrouped` + custom groups) with text filtering, status pills, playtime summaries, and quick **`Play`** / **`Open`** controls.
- **Selective File Tree for Modpack Exports (`mcw-providers`, `mcw-desktop`)**:
  - Added `ModpackInstaller::scan_export_candidates` (`ExportTreeEntry`) and extended `ModpackExportOptions` with `excluded_paths` and `extra_included_paths` while guarding against duplicate ZIP entries in `ModpackInstaller::export_instance`.
  - Added a collapsible **"Customize Files to Export"** parent/child checkbox tree inside **Export Modpack** across `.mrpack`, `.mcwpack`, and profile `.zip` exports.

---

## [1.3.0-beta.1] - 2026-11-25

### Added
- **Technic Platform & Solder API Modpack Provider (`mcw-providers`, Desktop)**:
  - Added `TechnicClient` supporting both Technic Platform single-zip packs and Technic Solder API manifests (`builds` + per-mod `md5` verification).
  - Integrated `BrowserProvider::Technic` & `ModpackInstallProvider::Technic` into the **Mod & Modpacks** browser and extended `ModpackUpdateService` across all **5 modpack providers** (**Modrinth**, **CurseForge**, **FTB**, **ATLauncher**, **Technic**).
- **Prism Launcher / MultiMC `.zip` Import (`mcw-providers`)**:
  - Added detection and import support for Prism Launcher / MultiMC `.zip` exports (`mmc-pack.json`, `instance.cfg`, `notes.txt`, `.minecraft/`), mapping Minecraft version, loader (`Fabric`, `Quilt`, `Forge`, `NeoForge`), memory/JVM preferences, and instance notes.
- **Quick Account Switcher & Per-Instance Account Override (`mcw-models`, Desktop)**:
  - Added a `ComboBox` quick account switcher in the top navigation bar alongside per-instance `preferred_account_uuid` override selection in **Instance Settings** (`resolve_account_for_instance`).
- **Direct Launch CLI & Create Desktop Shortcut (`mcw-desktop`)**:
  - Added `--launch <instance>` (with optional `--world <folder>` and `--server <host[:port]>`) CLI flags to `mcw-desktop` (`parse_startup_launch_args`).
  - Added **`Create Desktop Shortcut`** in the instance `...` Actions menu (`.lnk` on Windows, `.desktop` on Linux).
- **Workspace AIO Tools (`mcw-minecraft`, Desktop)**:
  - **Instance Notes (`notes.txt`)**: Multiline editor under **Instance Info** (*Instance Settings*) synced with `<instance_dir>/notes.txt` and previewed in the Home hero header.
  - **Visual Screenshot Gallery & Preview Modal**: Asynchronous decoded screenshot thumbnail grid (up to 48-entry LRU texture cache) with a centered **In-App Preview Modal** supporting rename, open, and delete.
  - **World Icon, Copy Seed & World Export (`.zip`)**: Renders world `icon.png`, reads NBT world seed (`WorldGenSettings.seed` / `RandomSeed`) with one-click **`Copy Seed`**, and exports worlds to `.zip` via `SaveFileDialog`.
  - **Export Modlist (`.md` / `.txt`)**: Added **`Export Modlist...`** in the Mods tab (`SaveFileDialog`), exporting sorted Markdown tables or plain-text summaries.

---

## [1.2.1] - 2026-11-23

### Added
- **In-App `Changelog` Tab (Next to `Logs`)**: Added a dedicated `Changelog` tab to the top navigation bar and `Ctrl+K` quick search, rendering the bundled `CHANGELOG.md` directly via Markdown formatting with an optional `"Refresh from GitHub"` button (with Offline guard).
- **Pre-Launch Dependency Check Toggle Across All Modloaders**:
  - Disabled pre-launch dependency warning modals (`Launch Anyway`) and blocking prompts across **all modloaders** on official modpacks (`Modrinth`, `CurseForge`, `FTB`, `ATLauncher`) and `.mcwpack` archives where enabled, while continuing to record all findings silently into Instance & Global Launcher Logs (`[LaunchCheck]`).
  - Added an independent **`Disable pre-launch dependency checking`** option (checked by default) when exporting `.mcwpack` archives and in per-instance *Instance Settings* (`disable_launch_checking`).
- **Safe `C:` / `os error 5` Handling & World Migration in `mcw-updater.exe` & Desktop**:
  - Updated standalone `mcw-updater.exe` (`apps/updater`) to detect non-writable install folders (such as protected `C:` locations) and `os error 5 (Access is Denied)` during binary replacement, redirecting into `%LOCALAPPDATA%\MCWLauncher` (`~/.local/share/mcw-launcher` on Linux).
  - Added a 5-second safety countdown dialog before actions unlock, offering **`Backup & Move (Recommended)`** (ZIP backup + user data & `saves/` worlds migration), **`Move Directly (Faster, risky)`**, and **`Cancel (Move Manually)`** when existing user data/worlds are found, or a single **`OK`** button when clean — mirrored by a startup write-probe In-App Modal guard in `mcw-desktop`.
- **Thorough Online / Offline Detection (`ConnectivityMonitor` v2)**:
  - Upgraded connectivity probing to race 4 independent endpoints concurrently (`api.modrinth.com`, `sessionserver.mojang.com`, `piston-meta.mojang.com`, `api.adoptium.net`) with a `3.5s` timeout and periodic background polling (`15s` while `Offline`, `45s` while `Online`).
  - Added an always-visible clickable **`Online`** / **`Offline`** indicator pill in the top bar and guarded network-only actions (manual update check, Microsoft login, Changelog refresh) when offline while allowing offline-account launches.

### Changed
- **New `v1.2.1` Defaults (`JetBrains Mono` & `Stable` Update Channel)**: Defaulted UI font to `"JetBrains Mono (Bundled)"` and launcher `update_channel` to `"stable"` for all fresh installs and existing users via a one-time `v121_defaults_migrated` migration (still user-editable afterward in Settings).
- **Default Global Browser Filters (`Relevance`, `All Loaders`, `All Versions`)**: Set global **Mod & Modpacks** searches to default to `Relevance`, `All Loaders`, and `All Versions` across Modrinth, CurseForge, FTB, and ATLauncher, showing the `All Versions` filter alongside `All Loaders` on Modpacks while keeping auto-filtered instance context for inline `+ Add` tabs.
- **Smart Short Modpack Naming on Import**: Added `suggest_short_instance_name` and `inspect_archive_name` across archive/online modpack imports so normal names stay exact (`<= 36` chars) and overly long titles (e.g., long suffixes/brackets) cleanly condense without filesystem/path issues.

### Fixed
- **Batch Progress ETA (`ETA 1s` on `35/567` Files)**: Reset phase timing, baseline count, and smoothed throughput EMA whenever tasks switch between byte-streaming transfers and discrete item-count batches (`ActiveTask::record_progress` + `estimated_remaining_secs`), ensuring accurate total remaining time across **Modrinth**, **CurseForge**, **FTB**, and **ATLauncher** modpack installations.

---

## [1.2.0] - 2026-11-22

### Added
- **Download Engine v2 (`mcw-net` + Desktop)**:
  - Enabled HTTP/2 ALPN multiplexing (`reqwest` `"http2"`), adaptive HTTP/2 stream/connection flow windows (`2 MiB` / `4 MiB`), `tcp_nodelay(true)`, `30s` socket read timeout (replacing whole-transfer timeouts so large files no longer drop on slow connections), `pool_max_idle_per_host(32)`, and `256 KiB` buffered writers (`BufWriter`), eliminating cyber-cafe ~500 KB/s single-stream stalls.
  - Added multi-segment HTTP `Range` downloads for files `>= 4 MiB` (automatic `HEAD` probe with transparent single-stream fallback), configurable in **Launcher Settings -> Downloads** via `ComboBox` (`Off (1)` / `4` / `6 (Default)` / `8`).
  - Added smart HTTP retry logic (skipping permanent non-408/429 4xx errors and honoring `Retry-After`) plus `ETag` / `If-None-Match` (`304 Not Modified`) caching via `DownloadClient::get_text_cached` for Mojang manifests.
  - Displayed live `MB / MB` byte progress (`48.2 / 192.0 MB (25%) · ↓ 8.40 MB/s · ETA 17s`) for large single files (client JAR, Java JDK, Forge/NeoForge installers, modpacks, mods, and launcher updates) alongside smoothed ETA on all active tasks.
- **Persistent Disk Caches & Shared ContentStore Auto-Hardlinking**:
  - Persisted `VerificationCache` entries across sessions in `cache/verification_cache.json`.
  - Added `ModScanner::scan_dir_cached` (`cache/mod_metadata/<instance_id>.json`) to eliminate redundant `.jar` ZIP parsing across restarts.
  - Enabled automatic SHA-256 `ContentStore::try_hardlink_from_store` in `.mrpack` modpack installs and `ContentStore::adopt` for downloaded mods `>= 64 KiB`.
- **Runtime Security, Custom Lifecycle Hooks & Real-Time Folder Watcher**:
  - Automatically injected Log4Shell mitigation (`-Dlog4j2.formatMsgNoLookups=true`) for vulnerable Minecraft `1.7`–`1.18.1` versions.
  - Added per-instance `Pre-launch command`, `Wrapper command`, `Post-exit command`, `KEY=VALUE` environment variables (`$INST_NAME`, `$INST_ID`, `$INST_DIR`, `$INST_MC_DIR`, `$INST_JAVA`), and Linux **GameMode** (`gamemoderun`) / **MangoHud** (`mangohud`) toggles in **Instance Settings**.
  - Added debounced real-time directory watching (`notify`) on the active instance's visible `mods/`, `resourcepacks/`, `shaderpacks/`, or `screenshots/` folder.
- **Bundled `JetBrains Mono` Font (`SIL OFL-1.1`)**: Embedded static `JetBrainsMono-Regular.ttf` as the default `Monospace` font across logs and paths, plus `"JetBrains Mono (Bundled)"` selectable in **Appearance -> Font**, with attribution in **About** and `assets/fonts/JetBrainsMono-OFL.txt`.
- **Automatic Junk File Detection & Cleanup Reminder Modal**:
  - Background startup scan checking leftover `.part` / staging files, aged logs/crash-reports, and expired caches/unused versions while strictly protecting `saves/`, `mods/`, `config/`, `backups/`, `accounts/`, and `runtimes/`.
  - Opens an In-App Centered Modal when reclaimable junk reaches `>= 50 MB` with per-category checkboxes, `Later (Remind in 3 days)` snooze, and a two-way synced `Don't show automatic cleanup reminders` option backed by **Launcher Settings -> Storage** (`Retention days` + `Scan Junk Now`).

### Fixed
- **Linux Folder/File Browser & `open_external`**: Replaced non-Windows dialog stubs with native `rfd` (`xdg-desktop-portal`) + `zenity`/`kdialog` CLI fallbacks for `pick_file_dialog`, `pick_folder_dialog`, and `save_file_dialog`, and added a multi-launcher Linux fallback chain (`xdg-open` -> `gio` -> `kioclient` -> `kde-open` -> common file managers) with warning logging on failure.
- **Fresh-Machine Theme & `ComboBox` Visuals**: Styled `visuals.widgets.open`, explicitly synced `ThemePreference` and `ctx.set_theme(...)`, and applied active styles across both `Theme::Dark` and `Theme::Light` so `ComboBox` and theme tokens render properly on first run on brand-new machines without needing to re-apply in Settings.

---

## [1.1.1] - 2026-11-22

### Fixed
- **Forge 1.17+ / NeoForge `BootstrapLauncher` Split-Package Launch Crash (`_1._20._1` vs `minecraft`)**: Updated `ClasspathBuilder::build` in `mcw-minecraft` to omit the base vanilla `client_jar` when `main_class` is `BootstrapLauncher` (e.g., Forge 1.20.1 / NeoForge), and updated `ArgumentBuilder::build` to automatically ensure `<inherits_from>.jar` is present in `-DignoreList=`. Resolves `java.lang.module.ResolutionException` on modpacks such as *DeceasedCraft* (`1.20.1-forge-47.4.0` + `customnpcs` + Sinytra `Connector`).
- **Sticky Tab Counts Across Instance Tabs**: Preserved loaded item counts (`Mods (306)`, `Worlds (...)`, `Resource packs (...)`, `Shaders (...)`, `Screenshots (...)`, `Backups (...)`) when switching to other tabs within the same instance, updating live on asynchronous tab reloads and clearing automatically when switching to a different instance.
- **Single Active-Window Notification Banner**: Removed duplicate notice rendering in `Mod & Modpacks`, gated the main `CentralPanel` banner when any modal is open, scoped in-app modal notice display to the single highest-priority active modal (`topmost_notice_modal`), and added unified notice display inside *Launcher Settings* so notifications only appear once in the currently active window.
- **Immediate Download Speed Hiding When Idle**: Snapped `current_download_bps` immediately to `0.0` when sampled network byte delta is zero and suppressed the speed indicator whenever no unpaused/unfinished download task is actively running.
- **Rendered Markdown Release Notes in Update Dialog**: Replaced raw Markdown text in the Update Dialog with formatted rich-text Markdown rendering supporting headings (`#`–`####`), horizontal rules, fenced code blocks, blockquotes, nested bullet/numbered lists, bold, italic, inline code, and styled links.
- **Comprehensive Global Launcher Event Log**: Expanded Global Log retention to 2,000 entries with `[HH:MM:SS]` UTC timestamps and recorded full lifecycle diagnostics (version manifest loads, deduplicated download/launch stages, update checks & installs, modpack/mod/content pack operations, Java/repair/backup actions, and notification banners).

---

## [1.1.0] - 2026-11-22

### Added
- **In-App Centered Modal System (`render_in_app_modal`)**: Converted all 25 non-log popups (including `First-Run Setup`, `Create Instance`, `Select Version`, `Install Destination`, `Accounts`, `Export Modpack`, `Modpack Update`, `Repair Center`, `Java Runtimes`, `Content Library`, `Python Import`, `About`, `Update Dialog`, `All Active Tasks`) into In-App Centered Modals with backdrop dim and in-modal notice banners. Only `Global Logs` and `LAN Agent Log` remain OS viewports.
- **Inline Single-Category Per-Instance Browsing**: `+ Add Mod`, `+ Add Resource Pack`, `+ Add Shader Pack`, and Data Pack browsing now open locked single-category browsers directly inside the matching instance tab with `← Back to Installed`.
- **Modpack Protection (`disable_mod_repair_and_updates`)**: Verified online modpacks and imported `.mcwpack` instances default to hiding per-mod update checks, `Lock`, and dependency/compatibility repair banners inside `Mods`, configurable in `.mcwpack` Export and *Instance Settings*.
- **4-Provider In-Place Modpack Updater & Pre-Update Backup Prompt**: Added origin tracking & in-place `ModpackUpdateService` support for **CurseForge**, **FTB**, and **ATLauncher** alongside **Modrinth**, replacing `Check Mod Updates` with `Check Modpack Update` on official packs and prompting **`Yes (Launcher Backup)`** / **`I know what I'm doing`** / **`Cancel`** before updating.
- **Duplicate Mod Prompt & Auto-Suffix `(2)`**: Added 3-option confirmation (`Overwrite Existing` / `Keep Both` / `Cancel`) when adding duplicate mods via local `.jar` import or remote install, plus `InstanceStore::resolve_unique_instance_name` across modpack installers.
- **Python Launcher Root Auto-Detection (`resolve_python_launcher_root`)**: Selecting a parent folder in the Python Importer automatically locates the nested Python launcher root and updates the path field.

### Changed
- **Independent Rust Edition (`v1.1.0`)**: Updated project rules so MCW Launcher: Rust Edition evolves independently from MCW Launcher Python (retaining read-only Python migration tooling).
- **Responsive Bounds & UI Polish**: Set clamped `1024x640` minimum main-window size; fixed right-edge clipping in *Select Version* and *Accounts*; removed redundant `Choose Instance & Install...` CTA; simplified *Create Instance -> Modpacks* to a single `Browse Modpacks...` button; replaced fake path hints with neutral hints; enforced full main-window Back/Forward navigation (`<` / `>`) disabled while modals are open; hid `Mods` & `Shaders` tabs on Vanilla instances; displayed `(count)` only on the active instance tab; bounded remote thumbnails with a 64-entry LRU + idle cleanup (`~45-70 MB` idle / `~85-110 MB` active browsing RAM); enabled `auto_check_updates = true` by default with one-time `v1.1.0` migration; extended and polished the startup splash screen (`2.40s` with pulsing glow rings, staggered title reveal, and click/key skip).

---

## [1.0.0] - 2026-11-22

### Added
- **Official Cloudflare Edge CDN Auto-Updater (`UPDATES_ENABLED = true`)**: Enabled launcher self-updates for **MCW Launcher: Rust Edition** (`crates/mcw-updater`) targeting the Rust release feed first via `https://mcw-download.pages.dev/releases/rust/manifest.json` (`CLOUDFLARE_RUST_MANIFEST_URL`), with automatic failover to GitHub Releases (`mahiru7229/mcw-launcher-rust`).
- **SHA-256 Digest Verification & Mirror Failover**: Extended `ReleaseInfo` and `UpdateInstaller::download_and_stage` to extract platform archive SHA-256 digests (`sha256` / GitHub `digest`), verify streamed downloads byte-for-byte before staging (`sha2::Sha256`), exclude checksum/signature files (`.txt`, `.sha256`, `.sig`) from binary selection, and automatically fail over across `mirror_urls` if a CDN edge or upstream request fails.

### Changed
- **First Official General Availability Release (`1.0.0`)**: Promoted workspace version from `1.0.0-rc.1` to `1.0.0` with standalone transactional `mcw-updater`, Method 2 fallback script rollback, inline `Mod & Modpacks`, Back/Forward `<` / `>` shell navigation, animated startup splash, staged UI Scale `Apply`, universal `Enable MCW LAN Agent` creation opt-in, and safe Python Launcher migration.

---

## [1.0.0-rc.1] - 2026-11-22

### Added
- **Standalone Transactional Updater (`apps/updater` -> `mcw-updater`)**: Added a dedicated `eframe` updater executable (`mcw-updater.exe` / `mcw-updater`) spawned by the launcher to display loading progress while safely replacing binaries even when the running executable would otherwise be locked. Protects user directories (`instances`, `accounts`, `config`, `cache`, `backups`, `logs`, `runtimes`, `themes`, `lang`), creates a transactional pre-update backup with automatic rollback + `"Launch Previous Version"` button on failure, and automatically falls back to detached script replacement (Method 2, also with transactional `$backupDir` rollback) if `mcw-updater` is missing or fails to launch. Updated `.github/workflows/release.yml` to build and package `mcw-updater` alongside `mcw-desktop` and `mcw-cli`. *(Note: `mcw_updater::UPDATES_ENABLED` remains `false` during `v1.0.0-rc.1`; self-updates will be enabled in the official `v1.0.0` release.)*
- **Animated Startup Splash Screen (`render_startup_splash`)**: Added a `1.35s` startup loading screen featuring the embedded MCW logo, `"MCW Launcher: Rust Edition"`, version pill (`v1.0.0-rc.1`), ease-out progress bar, live stage status labels, and smooth fade-out (bypassed immediately when `MotionMode::Off`).
- **Back / Forward Navigation History (`<` / `>`)**: Added `NavLocation` history navigation (`can_go_back`, `can_go_forward`, up to 50 entries across pages, instances, and detail tabs), circular top-bar `<` / `>` buttons, and `Alt+Left` / `Alt+Right` keyboard shortcuts.
- **Python Migration Option in First-Run Setup**: Added a `"Migrating from MCW Launcher Python?"` card in First-Run Setup Step 0 (`Welcome`) so returning users can launch `mcw_vp_python_import` directly during onboarding.
- **`Enable MCW LAN Agent` in All Instance Creation Flows**: Added an `"Enable MCW LAN Agent (allow offline players to join LAN worlds)"` option (`apply_lan_agent_option`) across Custom creation, Import Local Archive, Online Modpack Install Dialog, and Global Install Destination -> Create New Instance.

### Changed
- **Inline `Mod & Modpacks` Workspace Page**: Unified global **Mod & Modpacks** browsing (`AppPage::Modpacks`) directly into the main workspace alongside `Versions` and `Logs`, used for both the top pill bar and *Create Instance -> Modpacks* online providers (keeping per-instance `+ Add Mod` / Content Pack pickers in isolated OS windows).
- **MCW Logo Replaces Top-Left Hamburger & Instance Flyout**: Removed the redundant top-left hamburger button and `render_instance_flyout` / `render_instance_list` (keeping the slim left instance rail as the single instance switcher) and placed the clickable MCW Launcher logo (navigating to `Home`) next to `<` / `>`.
- **UI Scale Staged Apply + High-Scale Warning**: Changed the *Appearance -> UI Scale* slider to stage `pending_ui_scale_pct` until clicking **Apply Scale** (preventing the slider from shifting under the cursor mid-drag) and added a warning notice when selecting `> 125%` scale.

---

## [0.15.0-beta.1] - 2026-11-21

### Added
- **Custom System Font, UI Scale & Font Size Controls**: Added SFNT font discovery (`scan_system_fonts()`), `UI Font` `ComboBox` selector (`Default (System UI)` + installed `.ttf`/`.otf` fonts), live `UI Scale` (`75%`–`175%`), `Font Size` (`10`–`22 px`) controls, and English/Vietnamese preview in *Launcher Settings -> Appearance*, persisted in `LauncherPrefs`.
- **Safe Import / Migration from MCW Launcher Python (`mcw_vp_python_import`)**: Added read-only `inspect_python_launcher` and copy-only `migrate_from_python_launcher` in `mcw-storage`, supporting both modern Python 2.x/v0.5+ flat instances (UUID `id` remapping, nested `settings.json`, `icon_origin` objects, `.mcw/*-pack.json` and `mod-provenance.json` schema normalization, Java runtime path sanitization) and legacy 1.8.x `.minecraft/` layouts, plus optional global settings, Java runtimes (`runtimes/java-*`), custom themes, conflict strategy (`Skip` / `RenameImported`), and clear DPAPI `accounts.db` re-login warning. Accessible from *Launcher Settings -> Storage* and *Create Instance -> Modpacks*.
- **Full English i18n Key Coverage & Language Completion Indicator**: Populated all missing desktop UI keys in `lang/en-US.json` (while preserving `lang/vi-VN.json` for manual translation) and added dynamic translation completeness calculation (`LanguageManager::translation_completion`) displaying completion `%` and incomplete-translation guidance inside First-Run Setup Step 1 and *Launcher Settings -> General*.

### Fixed
- **Manual Downloads Responsive Layout (`mcw_vp_manual_downloads`)**: Fixed overflowing filenames and off-screen `Open Page` buttons by reserving the action button first (`Layout::right_to_left`), wrapping text/instructions cleanly, pinning the footer bar, adding a native `Browse...` folder picker, and letting the `ScrollArea` occupy remaining viewport height cleanly.
- **Initial Window Sizing (`1600x900` on `1920x1080`, `1280x720` on `1366x768`)**: Enabled Windows Per-Monitor v2 DPI awareness before screen measurement in `main.rs`, divided physical targets by monitor DPI scaling to logical points accurately, gated legacy auto-saved dimensions with `window_size_schema_version = 2`, and added a 2-second startup grace window before setting `window_size_customized`.
- **Auto-Open Accounts on Missing Account or Expired Token**: Kept Launch / Quick Play / Join Server buttons actionable when signed out and added `is_account_or_token_error` detection on launch/refresh failures to immediately open and focus the Accounts OS window.

---

## [0.14.0-beta.1] - 2026-11-21

### Added
- **Native App Icon, Windows Executable Metadata & Security Policy**: Added `assets/icons/mcw_launcher.ico` and `mcw_launcher.png`, Windows resource embedding (`apps/desktop/build.rs` + `winresource`), runtime window/taskbar icon loading in `main.rs`, and `SECURITY.md`.
- **In-Place Modrinth Modpack Updater (`ModpackUpdateService`)**: Recorded Modrinth project/version/managed files via `ModrinthPackRegistry` (`.mcw/modrinth-pack.json`) and added a dedicated `Modpack Update` OS window (`mcw_vp_modpack_update`, accessible from Home `...` Actions menu) that upgrades `.mrpack` instances in-place while protecting `saves/`, `config/`, `options.txt`, `servers.dat`, `screenshots/`, `resourcepacks/`, and `shaderpacks/`.
- **Installed Content Library OS Window (`mcw_vp_content_library`)**: Added UI inspector over `mcw-mods::InstalledContentLibrary` supporting multi-instance search, kind/instance/pinned filters, persistent `Pin`/`Unpin`, and `Open Folder`.
- **Interactive Compatibility Confirmation Dialog (`mcw_vp_preflight_confirm`)**: Added `"Launch anyway this time"`, `"Always allow for this instance"` (`InstanceSettings.forge_preflight_always_allow`), and `"Cancel"` confirmation before Forge/NeoForge launches when compatibility issues trigger confirmation.
- **Sanitized Issue Report OS Window (`IssueReportBuilder` & `mcw_vp_issue_report`)**: Added Markdown bug report builder (scrubbing tokens/paths/usernames via `sanitize_log_text`), Diagnostics `.zip` export, and one-click GitHub Issue URL generation accessible from `Logs`, `About`, and `Ctrl+K` Quick Search.
- **Connectivity Monitor, Download Journal, Theme Validation & Unsaved Settings Guard**: Added `mcw_net::{ConnectivityMonitor, DownloadJournal}`, top-bar clickable `"Offline"` indicator when unreachable, external JSON `ThemeCatalog` validation issues window (`mcw_vp_theme_issues`), and `"Unsaved Changes"` confirmation (`mcw_vp_unsaved_changes`) when switching away from modified Instance Settings.

### Changed
- **Release Archive Packaging**: Updated `.github/workflows/release.yml` so Windows (`x64`) and Linux (`x64`) release archives also bundle the `themes/` directory alongside binaries and docs.
- **Backlog & Design Sync**: Updated `docs/BACKLOG.md` and appended Section 10 to `DESIGN.md`.

---

## [0.13.2-beta.1] - 2026-11-21

### Added
- **Content Browser Pagination (`Previous` / `Next`)**: Added full page controls (`Page X / Y (N results)`, `PageSize = 20`) inside the global and per-instance **Content Browser** across **Modrinth**, **CurseForge** (`CurseForgePagination`), **FTB**, and **ATLauncher** (`search_paged`), resetting to Page 1 whenever query/sort/loader/MC-version filters change.
- **Dedicated Step-2 `Install Destination` OS Window (`mcw_vp_install_target`)**: Clicking `Install` or `Install Selected Version` in global **Mod & Modpacks** Version Picker now opens a clean second OS window allowing users to choose an **Existing Instance** (strictly filtered by **exact matching Minecraft version** + **exact matching mod loader**, with Data Pack world selection) or **Create New Instance** (MC version & loader `ComboBox`), with `Back` support to return to Version Picker.
- **Dedicated `Java Runtimes` OS Window (`mcw_vp_java_manager`) & Dynamic Latest JDK Lookup**: Replaced inline stacked buttons and overflowing Java paths in *Launcher Settings -> Runtime* with a compact summary card (`Manage Java Runtimes...`, dynamic `Install latest JDK (Java {major})` queried from Adoptium `available_releases`, and `Scan Java installations`) and a dedicated scrollable OS window with version dropdown, Latest JDK button, truncated paths with hover tooltips, `Open Folder`, and `Uninstall`.
- **Native Dedicated GPU Detection (`GpuPreferenceManager::detect()`)**: Added Windows (`Win32_VideoController`) and Linux (`lspci`) dedicated vs. integrated adapter detection inside `FirstRunRecommendation`.

### Changed
- **Responsive Content Browser Header Pill Bar**: Split Title + `Close` (Row 1) from the left-to-right Category Pill Bar (`Mods`, `Modpacks`, `Resource packs`, `Shader packs`, `Data packs` on Row 2) so tabs never squeeze or wrap awkwardly onto a second line.
- **Smart Default Window Size**: Fresh launches automatically size to `1600x900` on `>= 1920x1080` displays and `1280x720` on `>= 1366x768` displays (DPI-clamped on high-DPI), tracking explicit user resizes via `window_size_customized` so prior auto-saved `1080x680` defaults upgrade cleanly.
- **Pure Black Default Background (`#000000`)**: Removed the procedural Horizon pixel-landscape fallback; Dark mode now uses pure `#000000` black by default when no custom user wallpaper is configured.
- **Python-Parity 6-Step First-Run Setup Wizard**: Rebuilt First-Run Setup (`Welcome` -> `Account` -> `Java` -> `Memory` -> `Graphics` -> `Finish`) matching MCW Launcher Python's `first_run_setup_dialog.py` workflow, i18n keys, account creation/OAuth helpers, Java auto-selection, RAM slider, GPU preference toggle, and extended configuration summary.

---

## [0.13.1-beta.1] - 2026-11-21

### Added
- **1-Step Global Content Install Target Dialog**: Browsing **Mod & Modpacks** (`Mods`, `Resource packs`, `Shader packs`, `Data packs`) from the top navigation opens a dedicated target selector directly inside the version picker (`Existing Instance` with Data Pack world selection, or `Create New Instance` pre-filled with the matching MC version and loader).
- **JVM Preset Selector in Modpack Install Dialog**: Added a `JVM Preset` `ComboBox` (`Default`, `Aikar`, `ZGC`, `Shenandoah`) directly inside the Modpack Install dialog, persisting selected flags into the created instance's `settings.json`.
- **Cross-Category Right-Pane Search in Launcher Settings**: Searching in `Launcher Settings` now filters setting cards across all 6 categories (`General`, `Downloads`, `Runtime`, `Appearance`, `Storage`, `Updates`) and displays matched cards with category headers in the right pane.
- **Secondary OS Viewport Re-Focus**: Re-triggering an already-open secondary OS window (`Create Instance`, `Export Modpack`, `Content Browser`, `Accounts`, `Repair Center`, `Global Logs`, `First-Run Setup`, `Manual Downloads`, `About`, `All Active Tasks`, `Rename`/`Clone`/`Delete`/`Group & Tags`) brings it back to the foreground.

### Changed
- **Navigation & Home Tab Restructure**: Renamed top-bar `Mods` to **Mod & Modpacks** (global browsing mode); moved **Servers & LAN** from the top bar into each instance's Home tab bar; replaced the Home quick-edit pencil with an 8-tooth **Gear** icon opening `Instance Settings` and removed the `Instance Settings` pill from the tab bar; removed the duplicate `MCW LAN Agent` block from `Instance Settings`.
- **Manual-Only Instant LAN Hosting**: Removed `e4mc` tunnel options and the `Apply LAN Hosting` button from **Servers & LAN**; toggling `Enable MCW LAN Agent` now saves `Manual (Local LAN / VPN)` mode immediately without touching user-installed tunnel mods.
- **Content Browser Vertical Card Layout & Global Filters**: Fixed horizontal stacking in the Content Browser result list by wrapping split columns in explicit top-down layouts, added a global MC Version filter `ComboBox`, and set Modpacks default loader filters to `Fabric` (Modrinth) and `Forge` (CurseForge, ATLauncher, FTB).
- **Versions Page Single Overall Scroll**: Made `Cached Versions` collapsible (`Show` / `Hide`) and removed the inner `max_height` scroll inside `Available Minecraft Versions` so the entire page scrolls smoothly together.
- **Origin-Aware Modpack Export Formats**: Restricted Export Modpack formats by detected instance origin (`Modrinth` -> `.mrpack` + `.mcwpack`; `CurseForge` -> `CurseForge Modpack (.zip)` + `.mcwpack`; `ATLauncher`/`FTB` -> `Provider Profile (.zip)` + `.mcwpack`; manual/custom -> `.mcwpack` only).
- **First-Run / Default Recommended RAM Formula**: Updated default max memory calculation to `max(4096 MB, 50% total RAM)` snapped to 256 MB steps (`<= total - 1024 MB` safety clamp on `<= 4 GB` PCs, `16384 MB` cap).
- **Display Polish**: Title-cased mod loader and version type labels (`Vanilla`, `Fabric`, `Quilt`, `Forge`, `NeoForge`, `Release`, `Snapshot`, `Old Beta`, `Old Alpha`, `All Loaders`) across the UI while keeping lowercase JSON slugs; replaced all Unicode em/en-dashes in UI strings and window titles with ASCII `-`.

### Fixed
- **Background Picker Hang**: Moved background picture file picking (`TopMost` Windows dialog) and PNG/JPEG/WebP image decoding onto background threads so clicking `Choose a picture...` never freezes the UI thread.
- **OptiFine / Import Browse Label Leak**: Fixed raw `"common.browse"` key display by switching to the verified `instance_defaults.java.browse` localization key.

---

## [0.13.0-beta.1] - 2026-11-21

### Added
- **Global `Ctrl+K` Quick Search (`quick_search.rs`)**: Top-bar compact `Search...  Ctrl K` trigger pill (`shell_nav.rs`) and centered in-app command palette overlay searching instances, pages (`Home`, `Servers`, `Versions`, `Logs`), Content Browser categories (`Mods`, `Modpacks`, `Resource packs`, `Shader packs`, `Data packs`), and quick actions with instant `Enter` selection and `Esc` / backdrop dismiss.

### Changed
- **Full UI v2.0 Dialog & Component Restyle**: Upgraded remaining sharp `CornerRadius::same(2..4)` tokens across Create Instance, Modpack Install, Version Picker, Accounts, Manual Downloads, First-Run Wizard, Update Dialog, Repair Center, Global Logs, About, LAN Agent Log, Launcher Settings cards, flyout cards, right-click context menu, thumbnails, account avatars, and group/tag badges to cohesive UI v2.0 radii (`6px-14px`), rounded Create Instance pill tabs, translucent bottom Tasks Bar (`p.glass(...)`), and consistent `Esc`-to-close across all secondary OS viewports.
- **Simple Animation Polish (`shell.rs`)**: Smoothed active task progress bars using `animate_value_with_time` (`self.motion_secs(0.15)`) and gated indeterminate spinner animation on `MotionMode::Off`.
- **SOLID & Maintenance Cleanup**: Extracted the in-app Launcher Settings modal into `apps/desktop/src/app/views/settings_modal.rs` (bringing `dialogs.rs` down to ~2.2k lines), removed dead mod-install events/helpers (`CurseForgeModInstallFinished`, `ModrinthInstallFinished`, `apply_theme`), cleaned stale `#[allow(dead_code)]` attributes, wired additional official i18n keys, and synced `README.md`, `DESIGN.md`, and `docs/BACKLOG.md`.

---

## [0.12.0-beta.1] - 2026-11-21

### Added
- **Home / Instance Page (`home_page.rs`)**: Horizon-style hero header (icon, title, playtime/mod/world stats, responsive group/tag pills), large `Play` button, `•••` Actions menu (consolidating all instance management actions), quick vector icon actions (Add Content, Open Folder, Edit Settings), and one floating translucent tabbed panel (`Mods`, `Worlds`, `Resource packs`, `Shaders`, `Screenshots`, `Backups`, `Logs`, `Settings`).
- **Unified Content Browser OS Window (`content_browser.rs`)**: Centered OS viewport covering `Mods`, `Modpacks`, `Resource packs`, `Shader packs`, and `Data packs` (`mcw-mods::ContentKind::DataPack` + CurseForge class ID `6945`) with `ComboBox` filters (`Provider`, `Sort`, `Loader`, `Target Instance`), split scrollable list + right-hand detail panel (with per-world vs global target selector for Data Packs).
- **Versions Page (`versions_page.rs`)**: Dedicated management for cached Minecraft versions under `cache/versions/` (`StorageCleanupService::list_cached_versions` / `delete_cached_version`, usage tracking, `Prune Unused`) plus Mojang manifest browser (`Releases` / `Snapshots` / `All`).
- **Servers & LAN Page (`servers_page.rs`)**: Uncompressed NBT `servers.dat` reader/writer (`mcw-minecraft::ServerManager`) with Add/Remove/Copy IP, direct `--quickPlayMultiplayer` join on MC 1.20+, plus direct MCW LAN Agent & `e4mc` tunnel controls.
- **Logs Page (`logs_page.rs`)**: Unified in-app log inspector for both Global Launcher Events and per-instance live stdout/stderr streams (with Crash Report jump, `mclo.gs` upload, Diagnostic `.zip` export, and Pop Out Global Window).

### Changed
- Split desktop pages & unified Content Browser into focused modules per `SOLID.md`, trimming ~2,000 lines across `dialogs.rs` and `workspace.rs`.
- Replaced interim v0.11 top-bar destinations with dedicated `Home`, `Mods` (OS window), `Servers`, `Versions`, and `Logs` views.

---

## [0.11.0-beta.1] - 2026-11-20

### Added
- **UI v2.0 foundation (Horizon-style, see DESIGN.md)**: full-window background picture (user PNG/JPEG/WebP or a bundled procedural pixel landscape) with *Panel opacity* and *Background dimming* sliders in Launcher Settings -> Appearance (no blur).
- **Theme presets**: Violet (new default), Sunrise, Neon, Ocean, plus the classic Emerald (`dark`) and Light; custom accent and `themes/*.json` keep working.
- **New shell**: floating top pill navigation (Home, Mods, Servers, Versions, Logs), slim instance icon rail with status dots and right-click menu, instance flyout (search, favorites, groups, tags).
- **Launcher Settings is now an in-app centered modal** with category list + search (Esc / click outside to close).
- Simple animations only (hover/fade, page and modal fade-in), honoring Motion Full / Reduced / Off.

### Changed
- Global corner radii enlarged; panels are translucent over the background.
- Interim nav wiring until v0.12 pages: Servers -> instance Settings tab, Versions -> Settings -> Storage, Logs -> Global Logs window.

---

## [0.10.0-beta.1] - 2026-11-20

### Added
- **Stage 5 Full Python Parity**: Completed final parity modules across the core workspace (`92df5aa`):
  - **Shared Content Store (`mcw-storage::content_store`) & Content Library (`mcw-mods::library`)**: Content-addressed SHA-256 hardlink deduplication across instances plus unified library scanning and pin flags.
  - **Managed OptiFine Lifecycle (`mcw-minecraft::optifine`)**: Transactional Forge-based OptiFine validation, install/rollback, registry tracking, and clean uninstall.
  - **Forge Preflight Analyzer (`mcw-minecraft::preflight`)**: Pre-launch mod compatibility confirmation policies and sanitized loader diagnostics `.zip` export.
  - **LAN & Legacy Compatibility (`mcw-minecraft::lan_targets`, `legacy_libloader`)**: Mojang/Tiny mappings parser, JVM bytecode method table resolver, and embedded legacy library loader extraction.
  - **Pack Registry & Java Diagnostics (`mcw-providers::pack_registry`, `mcw-runtime::java_diagnostics`, `mcw-models::policy`)**: FTB/ATLauncher persistent registries, export origin detection, optional-file inspection (`.mrpack` / CurseForge), detailed Java runtime diagnostics + managed uninstaller, and dedicated LAN Agent Log OS window (`stage5_dialogs.rs`).

---

## [0.9.0-beta.1] - 2026-11-20

### Added
- **Stage 4 Security, Compatibility & Diagnostics**:
  - **Token Integrity & Network Controls**: RFC-2104 HMAC-SHA256 verification on encrypted `AccountStore` refresh tokens (`mcw-auth::integrity`), global MB/s bandwidth limiter (`mcw-net::bandwidth`), and optional runtime CurseForge API key override.
  - **Mod Compatibility & Instance Health**: Loader mismatch, duplicate mod & missing dependency checks (`mcw-mods::compat`), instance health repair scanning + startup stale lock/staging orphan recovery (`mcw-storage::health`).
  - **GPU Preference & OS Windows**: Windows DirectX high-performance GPU preference registry support (`mcw-runtime::gpu`), native window activation (`mcw-runtime::window_activator`), and dedicated Repair Center, Global Logs & About OS windows (`stage4_dialogs.rs`).

### Changed
- **Updater Disabled (`mcw_updater::UPDATES_ENABLED = false`)**: Disabled in-app self-updates (`e262586`) and removed the legacy Python/Cloudflare update channel while preserving the Rust-Edition GitHub Releases checker/installer scaffold.

---

## [0.8.0-beta.1] - 2026-11-20

### Added
- **Stage 1 - Runtime, Cloning & Organization**: JVM Flag Presets (`Default`, `Aikar`, `ZGC`, `Shenandoah`), Windows classpath compactor (`compactor.rs`), instance cloning & runtime/loader switcher, sidebar search + Favorites/Groups/Tags with responsive auto-sized pill badges, and `.mcwpack` portable import/export.
- **Stage 2 - Providers, LAN & World Backups**: `ModProvenanceRegistry` & `ModUpdateManager` with version locking, recursive CurseForge dependencies, `LanHostingManager`, per-world backup/restore/import, `LaunchErrorPresenter` & `DiagnosticsExporter`, SHA-1 `VerificationCache`, async `SaveFileDialog`/`OpenFileDialog`, mod version channel/version/loader filters, and schema 2.1 diagnostics.
- **Stage 3 - Appearance, Hardware Scan & Maintenance**: Microsoft skin face avatars (`mcw-auth::skin`, `skin_render.rs`), Discord IPC Rich Presence (`mcw-runtime::discord_rpc`), initial `FirstRunRecommendation` hardware/RAM/Java wizard, `StorageCleanupService` inspector & unreferenced version pruner (`mcw-storage::cleanup`), external `themes/*.json` pack loading + custom accent hex picker, and `mcw-updater::installer` scaffold (`stage3_dialogs.rs`).

### Changed
- **SOLID Architectural Refactor**: Split monolithic GUI state (`e763106`) into cohesive `controllers.rs`, `platform.rs`, and focused view modules (`views/`) and introduced the `InstanceRepository` storage trait.

---

## [0.6.0-beta.1] - 2026-11-19

### Added
- **Worlds, Screenshots, Packs & Logs**: Worlds Manager with Quick Play (`--quickPlaySingleplayer`), Screenshots gallery manager, sanitized `mclo.gs` log uploads, and Resource Packs / Shader Packs management with Modrinth & CurseForge browsing.
- **Bilingual i18n & OptiFine / LAN Agent**: Ported official MCW Python `LanguageManager` reusing embedded `lang/en-US.json` and `lang/vi-VN.json`, added OptiFine installer support and embedded MCW LAN Agent (`mcw_lan_agent`), and rebuilt Launcher Settings sections from MCW Python.
- **Rich Modpack & Content UX**: Direct FTB / ATLauncher / Modrinth modpack installs, 3-mode Modpack Export (`.mrpack`, CurseForge `.zip`, `.mcwpack`), async preview thumbnails, Version Picker & Modpack Install config dialog (Beta/Alpha channels, RAM, Java, Open Web), live download speed readout, and inline Pause/Cancel controls.

### Changed
- Preferred `javaw.exe` (suppressing extra console windows on Windows), isolated sub-windows into centered OS viewports, and switched the GitHub Release workflow to manual-only (`be36bca`, `5cfae45`).

---

## [0.5.0-beta.1] - 2026-11-19

### Added
- **Modpack Installer**: `.mrpack` (Modrinth) and CurseForge `.zip` archive import, plus CurseForge Modpacks, FTB, and ATLauncher search browsers inside the `Create Instance` window.
- **Forge & NeoForge Launch Pipeline**: Legacy Forge (`1.12.2` `install_profile.json` universal JAR extraction) and modern Forge / NeoForge (`--installClient`) wired directly into instance launching.
- **Concurrent Multi-Install & Global Live Progress Bar**: Non-blocking parallel installs for Modpacks, Mods (Modrinth/CurseForge), and managed Temurin Java runtimes with a live `ACTIVE INSTALLS (N)` bottom progress bar.
- **Incomplete Modpack Guard & Auto/Manual Download Scanner**:
  - Instances missing CurseForge opt-out manual mods are persistently flagged (`Incomplete (N)`) on the Sidebar and Detail Header.
  - Per-instance Manual Downloads windows with clear instance names, **Auto-scan** (every 2s), **Manual Scan**, browser `(1)` duplicate suffix matching, and direct CurseForge countdown links.
- **Launcher Settings Window**: Dedicated separate window accessible via top-right vector gear icon with `Appearance` tab (`Dark (Default)` / `Light` theme).
- **Backup & Safe Rollback Restore** (`mcw-storage::backup`) and **Instance Repair** (`mcw-minecraft::repair`).
- **Python 1.8.x Migration** (`mcw-storage::migration`, `mcw-cli migrate`) with `--dry-run` support.
- **Compile-Time Encrypted CurseForge API Key (`build.rs`) & CI/Release Workflows**: Automated Windows & Linux GitHub Actions builds with per-build SHA-256 CTR obfuscation for embedded API keys (`cdd049a`) and size-optimized release profile.

### Changed
- **Instant Instance Switching**: Replaced synchronous `mods/` & `backups/` scanning on click with lightweight cache sync + lazy async Tokio background loading.
- **Button Typography & Sizing**: Removed cramped `.small()` buttons globally and tuned `button_padding` so all controls auto-size cleanly around their text.
- **Download Reliability**: Deduplicated concurrent library download requests and switched temp files to unique `.{pid}-{seq}.part` names.

---

## [0.4.0-alpha.1] - 2026-11-19

### Added
- Initial native desktop GUI (`apps/desktop`, `egui`/`eframe`) implementing `DESIGN.md` (Dark Slate + Light theme, Emerald accent, Master-Detail layout).
- Microsoft OAuth PKCE & Device Code sign-in with cancellation support and Windows DPAPI encrypted refresh token storage.

---

## [0.3.0-alpha.1] - 2026-11-19

### Added
- `mcw-mods`: Parallel `rayon` `.jar` metadata scanner (Fabric, Forge, NeoForge, Jar-in-Jar) and mod enable/disable/delete manager.
- `mcw-providers`: SQLite WAL `ApiCache`, Modrinth v2 client, and CurseForge API v1 client.
- `mcw-updater`: Update checker with Cloudflare CDN + GitHub Releases failover.

---

## [0.2.0-alpha.1] - 2026-11-19

### Added
- Instance CRUD + `InstanceRunLock`.
- Microsoft OAuth (PKCE + Device Code + Xbox Live / XSTS / Minecraft token chain).
- Managed Adoptium Java Provisioner + `.class` bytecode version scanner.
- Fabric & Quilt mod loader integration.
- Win32 Job Object `ProcessSupervisor` with live log streaming and crash classification.

---

## [0.1.0-alpha.1] - 2026-11-19

### Added
- Initial Cargo workspace (`mcw-models`, `mcw-storage`, `mcw-net`, `mcw-auth`, `mcw-runtime`, `mcw-minecraft`, `apps/cli`).
- Full Vanilla Minecraft download & launch pipeline with SHA-1 verification.
