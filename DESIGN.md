# MCW Launcher Rust Edition - Product Design Direction

Status: **v2.1 UPDATED (2026-11-21)** - incorporates owner UI/UX review & polish decisions after `v0.13.0-beta.1` (supersedes v1.0 Slate/Emerald sharp master-detail). Source of truth for `apps/desktop` GUI decisions.

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
