# MCW Launcher Rust Edition - Product Design Direction

Status: **v2.2 UPDATED (2026-11-22)** - incorporates owner UI/UX review & Release Candidate (`v1.0.0-rc.1`) updates. Source of truth for `apps/desktop` GUI decisions.

Stack: `egui` / `eframe` (pure Rust, no WebView). Reference mood: "Horizon"-style launcher screenshots provided by the owner (pixel-landscape background, floating translucent rounded panels, violet accent). We copy the **vibe**, not the brand, content, ads, or feature set.

---

## 1. Product character
- Purpose-built native launcher: technical but approachable, efficient for frequent tasks (select instance, Play, manage content, inspect logs), honest about progress/errors.
- Warm and atmospheric, but never noisy. Hierarchy before decoration.

## 2. Confirmed decisions (owner answers)

| # | Topic | Decision |
|---|-------|----------|
| 1 | Scope | Rebuild **both style and navigation layout** like the reference, **keeping all features** and all existing OS-window rules. |
| 2 | Background | User-selectable picture (PNG/JPEG/WebP) + one bundled default pixel-landscape. File picking (`OpenFileDialog` / `SaveFileDialog`) and user image decode/resize run **asynchronously off the GUI thread** so choosing/switching a picture never freezes the window. Two sliders in *Launcher Settings -> Appearance*: **panel transparency** and **background dimming**. **No blur** (egui has none): translucent panels over the image only. |
| 3 | Colors | Default **violet accent** as in reference. Theme presets: Violet (default), Sunrise, Neon, Ocean, plus the old MCW **Emerald**. Free custom accent `#RRGGBB` stays. Existing `themes/*.json` keep working. Dark/Light remain. |
| 4 | Top nav (pill bar) | **Home, Mod & Modpacks, Versions, Logs.** (`Servers & LAN` lives inside each instance's tab bar.) Names/grouping follow the Python launcher. **No** Shop, Wardrobe, partner ads, sponsor banners. |
| 5 | Left side | Slim **icon rail** (add instance `+`, instance icons, bottom utility icons incl. Settings, version label). Click rail header/avatar opens an **instance flyout** (Add instance, instance list with status, tags/groups, search, links). |
| 6 | Home = selected instance | Large title, stats line, large **Play** button (+ `...` menu), quick icon buttons (**Add content `+` / Open folder / Gear button for Instance Settings**), and one floating panel with tabs: **Mods, Worlds, Servers & LAN, Resource packs, Shaders, Screenshots, Backups, Logs**. `Instance Settings` is removed from the tab bar (opened via the **Gear** quick icon instead), and `MCW LAN Agent` is removed from Instance Settings (managed in **Servers & LAN**). Old *Overview/Actions* become the `...` menu + Gear settings. Group/Tags live in Actions. |
| 7 | Settings | **In-app centered modal overlay** (left category list + search box + content), like reference. `Search settings` searches actual **right-pane setting cards, descriptions, and control labels** across all categories and renders a unified matching-cards view with section badges; clicking a category on the left clears search and switches to that category. Categories map to existing tabs (General, Downloads, Runtime, Appearance, Storage, Updates). Updates category stays present but disabled (`UPDATES_ENABLED = false`, no Python/Cloudflare feed). |
| 8 | OS windows | Separate, centered OS windows for: **Mod & Modpacks** browsing, Global Logs, Repair Center, LAN Agent Log, Manual Downloads, Create/Export/Clone/Delete/Rename/Group&Tags/Accounts, Modpack Install, Version Picker, Install Target picker, First-Run Wizard. Re-clicking the trigger of an already-open OS window restores (un-minimizes) and brings it to the foreground. |
| 9 | `Mod & Modpacks` nav tab | Opens the **OS-window browser** in global mode (not locked to a single fixed instance): title, pill row (`Mods` / `Modpacks` / `Resource packs` / `Shader packs` / `Data packs`), search + Sort `ComboBox`, provider filters, vertical scrollable result list with **right-hand detail panel**. Clicking **Install** on a Mod/Resource/Shader/Data pack opens a **1-step combined target dialog** allowing the user to pick an existing compatible instance OR create a new instance pre-filled with the mod version's Minecraft version & loader + select the mod version in the same window. Modpacks default loader filter: **Fabric** on Modrinth, **Forge** on CurseForge / ATLauncher / FTB. Inside an instance (`+ Add Mod`), content browsing continues targeting that instance directly. |
| 10 | Animation | **Simple only**: hover/fade on buttons & cards (~120 ms), fade-in on page change / modal open (~150 ms), smooth progress bar, spinner while loading. **No** background motion, parallax, or sliding pages. Must respect `MotionMode` Full / Reduced / Off (Off = instant). |

## 3. Carry-over rules (unchanged, from `.agent/rules` + earlier owner requests)
- Multi-choice options use `ComboBox`, never rows of buttons.
- Every list inside a panel is scrollable (`ScrollArea`, `auto_shrink([false,false])`), never hidden under another widget.
- Every OS window opens centered on screen, and re-triggering an active OS window restores and focuses it.
- Every save/export uses async `SaveFileDialog`; dismissible notices auto-hide after 8 s (configurable).
- Tags/groups chips auto-size to text and wrap responsively.
- Reuse Python i18n keys (`lang/en-US.json`, `lang/vi-VN.json`); no invented translations (verify exact key names before use so raw keys like `common.browse` never leak to UI).
- `VerificationCache` / no re-download of unchanged assets.
- SOLID: each new view in its own module; continue splitting `dialogs.rs`, `controllers.rs`, `workspace.rs` (see `docs/BACKLOG.md`).
- Updater stays disabled; no Python/Cloudflare update branch.
- **Dashes & casing**: Use ASCII hyphen `-` instead of em-dash/en-dash (`—` / `–`) across all UI titles, subtitles, and labels. Capitalize display labels consistently (`Vanilla`, `Fabric`, `Quilt`, `Forge`, `NeoForge`, `Release`, `Snapshot`, etc.) while keeping lowercase slugs in persisted JSON files for Python parity.

## 4. Visual language
- **Canvas:** full-window background image (dimmed) -> large rounded floating panels (radius ≈ 10-14 px) with 1 px soft border and semi-transparent fill; pills (radius full) for nav and tabs; primary button = accent fill, large rounded.
- **Shadows/gradients:** only subtle panel border/overlay needed for legibility; no decorative gradients beyond the background image itself.
- **Typography:** system UI sans (Segoe UI / Noto Sans, full Vietnamese diacritics), monospace for paths/versions/logs. Big bold page titles (~28-32 px), compact body (12-13 px).
- **Icons:** single-style monochrome line icons with text labels for primary actions; no mixed emoji.
- **Contrast:** text color is always computed against the panel+background mix (accent-aware); status never by color alone.
- **Default window:** ~1100x700, minimum ~860x560; panels reflow on resize.

## 5. Delivery plan (SemVer tags, no separate hotfix system)

1. **`v0.11.0-beta.1` - Foundation + Shell** *(Completed)*
   - Background image system (bundled default, user picture, panel transparency, dim sliders).
   - New palette model + presets (Violet default, Sunrise, Neon, Ocean, Emerald) + custom accent; keep `themes/*.json`.
   - Shell: top pill nav, left icon rail, instance flyout, account chip, Settings -> in-app modal.
   - Simple animation helpers (hover/fade) honoring `MotionMode`.
2. **`v0.12.0-beta.1` - Pages** *(Completed)*
   - Home/instance page (title, Play, quick icons, floating tabbed panel, `...` menu).
   - Versions page (`cache/versions/` + Mojang manifest browser).
   - New centered OS-window content browser (`Mods`, `Modpacks`, `Resource packs`, `Shader packs`, `Data packs` - `ComboBox` filters + split list & detail panel).
   - Servers/LAN page (`servers.dat` NBT manager + Quick Play + LAN), Logs page (unified Global & per-instance viewer).
3. **`v0.13.0-beta.1` - Polish** *(Completed)*
   - Global `Ctrl+K` Quick Search / Command Palette (`quick_search.rs`) + top-bar trigger pill.
   - Restyle every remaining dialog, card, badge, thumbnail, context menu, and tasks bar to UI v2.0 rounded tokens + `Esc`-to-close consistency.
   - Smooth animated progress bars honoring `MotionMode`, verified i18n polish, dead UI code cleanup, `settings_modal.rs` extraction, and docs sync (`README.md`, `CHANGELOG.md`, `DESIGN.md`, `docs/BACKLOG.md`).
4. **`v0.13.1-beta.1` - UI Polish & UX Fixes** *(Planned)*
   - Implement all 16 confirmed post-review fixes listed in Section 8 below + verified `common.browse` key fix.

Each stage ends with: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, release build, manual QA list, local commit + annotated tag (no push unless asked).

## 6. What MCW still avoids
- Ads, sponsor banners, shop/wardrobe/fake stats, fake progress bars.
- Heavy or continuous animation; blur/glass effects that egui cannot do honestly.
- Mixed icon styles, oversized mobile-style controls, huge empty hero areas that push controls below the fold.

## 7. Approval rule
Changes to foundational identity (palette model, typography, icon system, navigation model, global component language) require owner review. This v2.1 document is that updated approval for UI v2.0 + post-review refinements.

---

## 8. Confirmed UI Polish & UX Refinements (`v0.13.1-beta.1` - Completed)

1. **Versions page scroll & layout**: Replace cramped inner `max_height` scrolling with one overall vertical `ScrollArea` covering the page smoothly, keeping `Cached Versions` compact and giving `Available Minecraft Versions` generous room.
2. **Global `Mod & Modpacks` OS window & 1-step Install Target dialog**: Rename top-bar `Mods` -> `Mod & Modpacks`. Separate global top-bar browsing from per-instance `+ Add Mod`: in global mode, clicking **Install** on a Mod, Resource Pack, Shader Pack, or Data Pack opens a single-step combined dialog allowing the user to pick an existing compatible instance OR create a new instance pre-filled with the selected version's MC version & loader + choose the content version in the same window.
3. **Content Browser vertical list fix**: Fix the `ScrollArea::vertical` layout inside `ui.horizontal_top` so result cards stack strictly vertically top-to-bottom instead of side-by-side horizontally.
4. **Move `Servers & LAN` into per-instance tabs**: Remove `Servers` from the top navigation pill bar and add `Servers & LAN` as a dedicated tab inside each instance's floating tab bar on `Home`.
5. **Remove duplicate `MCW LAN Agent` from Instance Settings + fix `common.browse`**: Remove the `MCW LAN Agent` section from `Instance Settings` (now handled in `Servers & LAN`) and fix the raw `"common.browse"` key leak in the OptiFine section using verified label text.
6. **Capitalize display names consistently**: Title-case loader names (`Vanilla`, `Fabric`, `Quilt`, `Forge`, `NeoForge`) and version types (`Release`, `Snapshot`, `Old Beta`, `Old Alpha`) in every dropdown, badge, card, and header while storing canonical lowercase slugs in JSON.
7. **Gear icon replaces `Instance Settings` tab pill**: Change the small edit quick button next to Folder on `Home` to a crisp **Gear** icon and remove `Instance Settings` from the tab bar (clicking the Gear button opens Instance Settings inside the floating panel and highlights active).
8. **JVM Preset selector inside Modpack Install dialog**: Add a `JVM Preset` `ComboBox` right inside the Modpack Install configuration dialog and persist the chosen preset flags into the created instance's `settings.json`.
9. **Fix launcher freeze when picking/changing background**: Execute file open/save dialogs and background image loading/resizing asynchronously on worker threads so the GUI thread never blocks or hangs when clicking `Choose a picture...`.
10. **First-Run & default Recommended RAM formula**: Compute recommended max RAM for First-Run Setup & launcher defaults as `max(4096 MB, 50% total system RAM)` rounded to 256 MB steps, clamped safely to `<= (total_ram - 1024 MB)` on `<= 4 GB` machines and capped at `16384 MB`. Keep loader-based RAM recommendations when installing modpacks (`Forge` / `NeoForge` up to `8192 MB`, `Fabric` / `Quilt` / `Vanilla` up to `4096 MB`).
11. **Modpacks default loader filter by provider**: When opening or switching providers in Modpacks browsing, default the loader filter to **Fabric** for **Modrinth**, and **Forge** for **CurseForge**, **ATLauncher**, and **FTB**.
12. **LAN Hosting Manual-only & instant auto-apply**: Remove `e4mc` tunnel from LAN Hosting title, description, and provider options (keep only `Manual (Local LAN / VPN)`). Checking/unchecking `Enable MCW LAN Agent` saves and applies immediately with no separate `Apply LAN Hosting` button.
13. **Conditional Export Modpack formats by instance origin**:
    - Installed from **Modrinth**: `Modrinth Modpack (.mrpack)` + `MCW Portable (.mcwpack)`.
    - Installed from **CurseForge**: `CurseForge Modpack (.zip)` + `MCW Portable (.mcwpack)`.
    - Installed from **ATLauncher** or **FTB**: `Provider Profile (.zip)` + `MCW Portable (.mcwpack)`.
    - **Custom / manually created instance**: `MCW Portable (.mcwpack)` (`Smart` / `Full`) only.
14. **Launcher Settings right-pane content search**: Typing into `Search settings` filters setting cards/controls across all categories (titles, descriptions, and control labels) and renders all matching cards together in the right pane with category header tags; clicking a left category clears the search box and switches to that category.
15. **Replace all em-dashes & en-dashes with `-`**: Replace every `—` / `–` in `apps/desktop/src/` UI strings and OS window titles with ASCII `-`.
16. **Focus already-open secondary OS windows**: Re-clicking the button/menu item of an already-open OS viewport (`Create Instance`, `Export Modpack`, `Content Browser`, `Accounts`, `Repair Center`, `Global Logs`, `LAN Agent Log`, `Manual Downloads`, etc.) un-minimizes and brings that viewport to the foreground.

---

## 9. UI/UX & Python-Parity Fixes (`v0.13.2-beta.1` - Completed)

1. **Responsive Category Pill Bar in Content Browser**: Header Row 1 renders only Title + Subtitle (left) and `Close` (right); Row 2 renders a dedicated full-width Category Pill Bar left-to-right (`Mods`, `Modpacks`, `Resource packs`, `Shader packs`, `Data packs`) so category tabs never squeeze or wrap unevenly inside `[1020x660]` or `[760x480]` viewports.
2. **Content Browser Pagination (`Next` / `Previous`)**: Added `Previous` / `Next` page controls and `"Page X / Y (N results)"` footer (`BROWSER_PAGE_SIZE = 20`) across all 4 providers (Modrinth offset pagination, CurseForge pagination metadata, FTB paged slices, ATLauncher `search_paged`). Filter/query changes automatically reset `browser_page` to `0`.
3. **Screen-Aware Default Window Sizing**: First launch initializes at `1600x900` on `>= 1920x1080` displays and `1280x720` on `>= 1366x768` displays (DPI-clamped for high-DPI desktops). Existing auto-saved defaults (`window_size_customized == false`) upgrade seamlessly until the user manually resizes the window.
4. **Pure Black Default Background (`#000000`)**: Removed procedural dusk/pixel-landscape Horizon fallback; in Dark mode without a custom picture, the launcher paints pure `Color32::BLACK` (`#000000`). User-selected custom wallpapers still decode off-thread and scale seamlessly with panel opacity and dimming controls.
5. **Python-Parity 6-Step First-Run Setup Wizard**: Rebuilt First-Run Setup (`render_first_run_wizard`) to mirror MCW Launcher Python's 6-step guided dialog (`Welcome` -> `Account` -> `Java` -> `Memory` -> `Graphics` -> `Finish`) with progress bar, inline offline/Microsoft PKCE/Device sign-in, Java auto-vs-detected dropdown + rescan, Memory slider + recommendation, live `GpuPreferenceManager::detect()` dedicated GPU toggle + adapter list, and full extended summary (`first_run.summary.extended`).
6. **Strict 2-Step Global Content Install**: Removed `Install Destination` from Version Picker (`Select Version`). Selecting a version and clicking `Install` / `Install Selected Version` in global `Mod & Modpacks` browsing opens a separate centered OS window (`Install Destination`, `mcw_vp_install_target`) allowing the user to pick an **Existing Instance** (strictly filtered to **exact matching Minecraft version** and, for Mods, **exact matching loader**, plus Data Pack world target) or **Create New Instance** (pre-filled MC version + compatible loader choices) with `Back` navigation to Version Picker.
7. **Compact Java Card & Dedicated Java Runtimes OS Window + Dynamic Latest JDK**: Replaced the overflowing inline Java list and stacked install buttons inside *Launcher Settings -> Runtime* with a compact summary (`detected` / `managed` / available majors / latest JDK status) + `Manage Java Runtimes...` + dynamic `Install latest JDK (Java {major})` (fetched from Adoptium `available_releases` API) + `Scan Java installations`. Full runtime cards (`Open Folder`, `Uninstall`, Java version dropdown, dynamic Latest JDK, `Scan`) live cleanly inside an isolated scrollable OS window (`mcw_vp_java_manager`).

---

## 10. Full Python Parity & Release Readiness (`v0.14.0-beta.1` - Completed)

1. **Windows Application Icon & Runtime Window Icon**: Copied `assets/icons/mcw_launcher.ico` and `mcw_launcher.png` from Python, added `apps/desktop/build.rs` + `winresource` for Windows binary icon/metadata, and embedded `mcw_launcher.png` via `ViewportBuilder::with_icon` in `main.rs`.
2. **Packaging & Policy Completeness**: Added `SECURITY.md` and updated `.github/workflows/release.yml` to include `themes/` alongside packaged binaries on Windows and Linux.
3. **In-Place Modrinth Modpack Update (`ModpackUpdateService`)**: Tracks Modrinth project/version/managed files in `.mcw/modrinth-pack.json` (`ModrinthPackRegistry`) and upgrades `.mrpack` instances in-place (`mcw_vp_modpack_update`) by removing obsolete managed files and applying overrides while strictly protecting `saves/`, `config/`, `options.txt`, `servers.dat`, `screenshots/`, `resourcepacks/`, and `shaderpacks/`.
4. **Installed Content Library OS Window**: Dedicated `mcw_vp_content_library` window inspecting all installed mods, resource packs, shader packs, and modpacks across instances with search, kind/instance/pin filters, persistent Pin/Unpin toggles, and Open Folder actions.
5. **Interactive Forge/NeoForge Preflight Confirmation**: `mcw_vp_preflight_confirm` pauses launch cleanly when preflight encounters confirmation-policy issues, offering `"Launch anyway this time"`, `"Always allow for this instance"` (persisted on `InstanceSettings`), or `"Cancel"`.
6. **Sanitized Issue Report Builder & Dialog**: `IssueReportBuilder` (`mcw-runtime`) + `mcw_vp_issue_report` generate privacy-scrubbed Markdown bug reports with diagnostics `.zip` export and one-click pre-filled GitHub Issue creation.
7. **Connectivity Monitor, Download Journal, Theme Issues & Unsaved Settings Guard**: Top-bar `"Offline"` status badge + re-check (`ConnectivityMonitor`), `DownloadJournal` core persistence, `"Theme Issues (N)"` validator dialog (`mcw_vp_theme_issues`), and `"Unsaved Changes"` confirmation (`mcw_vp_unsaved_changes`) when leaving edited Instance Settings.

---

## 11. Typography, Account UX, i18n Completeness & Python Migration (`v0.15.0-beta.1` - Completed)

1. **Manual Downloads Responsive OS Window**: Pin footer in `TopBottomPanel::bottom`, render `Open Page` right-aligned first with wrapping filenames/IDs, add `Browse...` folder picker, and auto-expand vertical `ScrollArea`.
2. **DPI-Aware Initial Window Size**: Call Windows Per-Monitor v2 DPI awareness before `default_window_size_for_screen()`, convert `1600x900` (`>=1920x1080`) and `1280x720` (`>=1366x768`) targets accurately to logical points, upgrade stale `window_size_schema_version < 2`, and defer user-resize detection by 2 seconds after launch.
3. **Auto-Open Accounts on Missing/Expired Auth**: Keep launch/play buttons clickable and immediately open/focus `Accounts` whenever no account is active or launch fails with a token/authentication expiration error.
4. **Custom Font, UI Scale & Font Size**: Scan OS `.ttf`/`.otf` fonts via SFNT `name` table in `theme.rs` with Segoe UI/Noto fallback, expose Font `ComboBox`, `UI Scale` (`75%-175%`), and `Font Size` (`10-22 px`) controls in *Appearance* with live preview and reset.
5. **English i18n Completeness + Translation % Notice**: Maintain complete English keys in `lang/en-US.json` (never machine-translating `lang/vi-VN.json`) and display exact completion percentage + untranslated English fallback notice in First-Run Setup Step 1 and *Launcher Settings -> General*.
6. **Safe Python Launcher Migration (`mcw_vp_python_import`)**: Read-only source inspection & copy-only migration (`inspect_python_launcher`, `migrate_from_python_launcher`) supporting Python 2.x flat instances and legacy 1.8.x `.minecraft/` layouts, normalizing `.mcw/` provenance & modpack registries, sanitizing custom Java paths, and warning clearly that encrypted DPAPI tokens in `accounts/accounts.db` require fresh sign-in.

---

## 12. Release Candidate Shell, Navigation & Updater (`v1.0.0-rc.1` - Completed)

1. **Inline `Mod & Modpacks` Workspace Page**: Global **Mod & Modpacks** browsing (`AppPage::Modpacks`) now renders inline in the main workspace (synchronized visually with `Versions` and `Logs`), used by both the top navigation bar and *Create Instance -> Modpacks* providers. Per-instance `+ Add Mod` / Resource / Shader / Data pack triggers keep their dedicated OS viewport (`mcw_vp_content_browser`).
2. **Top-Bar MCW Logo & `<` / `>` Navigation History**: Removed the top-left hamburger button and `render_instance_flyout` / `render_instance_list` (keeping the left instance rail). Placed the clickable MCW logo (`Home`) and circular Back (`<`) / Forward (`>`) history buttons (`Alt+Left` / `Alt+Right`) at the left end of the top bar.
3. **Animated Startup Splash Screen**: Plays a `1.35s` dark splash overlay (`render_startup_splash`) displaying `"MCW Launcher: Rust Edition"`, version pill `v1.0.0-rc.1`, logo, progress bar, and stage status before entering the workspace (skipped cleanly under `MotionMode::Off`).
4. **UI Scale Staged Apply & Warning**: *Appearance -> UI Scale* uses staged `pending_ui_scale_pct` with an explicit **Apply Scale** button so the slider never shifts mid-drag, plus a high-scale (`> 125%`) layout warning.
5. **`Enable MCW LAN Agent` Across Every Instance Creation Flow**: Exposed `"Enable MCW LAN Agent"` consistently across Custom creation, Import Local Archive, Online Modpack Install Dialog, and Global Install Destination -> Create New Instance (`apply_lan_agent_option`).
6. **First-Run Setup Step 0 Python Migration Entry**: Added a `"Migrating from MCW Launcher Python?"` card directly in First-Run Setup Welcome launching `mcw_vp_python_import`.
7. **Standalone `mcw-updater` GUI + Transactional Fallback**: Built `apps/updater` (`mcw-updater.exe` / `mcw-updater`) with protected user directories, transactional backup + automatic rollback, `"Launch Previous Version"` recovery action, and automatic fallback to Method 2 detached scripts (also with `$backupDir` rollback). Auto-updating remained gated (`UPDATES_ENABLED = false`) during RC.

---

## 13. Official General Availability Release (`v1.0.0` - Completed)

1. **Auto-Updater Enabled (`mcw_updater::UPDATES_ENABLED = true`)**: Switched on Stable & Beta release checking from startup preferences and *Launcher Settings -> Updates*, wired primarily to `https://mcw-download.pages.dev/releases/rust/manifest.json` (`mcw-download` Cloudflare Edge CDN) with automatic failover to GitHub Releases API (`mahiru7229/mcw-launcher-rust`).
2. **CDN + Mirror Failover & SHA-256 Verification**: `UpdateInstaller::download_and_stage` streams downloads through `sha2::Sha256`, verifies digests before atomic rename, skips checksum text files (`SHA256SUMS.txt`), and falls back across deduplicated `mirror_urls` cleanly.
