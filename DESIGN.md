# MCW Launcher Rust Edition — Product Design Direction

Status: **v2.0 APPROVED by Product Owner (2026-11-20)** — supersedes v1.0 (Slate/Emerald, sharp master–detail). Source of truth for `apps/desktop` GUI decisions.

Stack: `egui` / `eframe` (pure Rust, no WebView). Reference mood: "Horizon"-style launcher screenshots provided by the owner (pixel-landscape background, floating translucent rounded panels, violet accent). We copy the **vibe**, not the brand, content, ads, or feature set.

---

## 1. Product character
- Purpose-built native launcher: technical but approachable, efficient for frequent tasks (select instance, Play, manage content, inspect logs), honest about progress/errors.
- Warm and atmospheric, but never noisy. Hierarchy before decoration.

## 2. Confirmed decisions (owner answers)

| # | Topic | Decision |
|---|-------|----------|
| 1 | Scope | Rebuild **both style and navigation layout** like the reference, **keeping all features** and all existing OS-window rules. |
| 2 | Background | User-selectable picture (PNG/JPEG/WebP) + one bundled default pixel-landscape. Two sliders in *Launcher Settings → Appearance*: **panel transparency** and **background dimming**. **No blur** (egui has none): translucent panels over the image only. |
| 3 | Colors | Default **violet accent** as in reference. Theme presets: Violet (default), Sunrise, Neon, Ocean, plus the old MCW **Emerald**. Free custom accent `#RRGGBB` stays. Existing `themes/*.json` keep working. Dark/Light remain. |
| 4 | Top nav (pill bar) | **Home, Mods, Servers/LAN, Versions, Logs.** Names/grouping follow the Python launcher. **No** Shop, Wardrobe, partner ads, sponsor banners. |
| 5 | Left side | Slim **icon rail** (add instance `+`, instance icons, bottom utility icons incl. Settings, version label). Click rail header/avatar opens an **instance flyout** (Add instance, instance list with status, tags/groups, search, links). |
| 6 | Home = selected instance | Large title, stats line, large **Play** button (+ `...` menu), quick icon buttons (Folder / Edit / Add mod), and one floating panel with tabs: **Mods, Worlds, Resource packs, Shaders, Screenshots, Backups, Logs, Settings**. Old *Overview/Actions* become the `...` menu + Edit. Group/Tags live in Actions (per earlier request). |
| 7 | Settings | **In-app centered modal overlay** (left category list + search box + content), like reference. Categories map to existing tabs (General, Appearance, Accounts, Downloads, Runtime/Game, Storage, Network, Data & privacy…, About). Updates category stays present but disabled (`UPDATES_ENABLED = false`, no Python/Cloudflare feed). |
| 8 | OS windows | Still **separate, centered OS windows** for: Mods/Modpacks/Resource/Shader/Data-pack browsing, Global Logs, Repair Center, LAN Agent Log, Manual Downloads, version pickers. |
| 9 | Mods nav tab | Opens the **OS-window browser** laid out like the reference: title, pill row (Mods / Modpacks / Resource packs / Shader packs / Data packs), search + Sort `ComboBox`, provider pills, result list with **right-hand detail panel** (Install in…, open page, policy). Inside an instance page: only the installed list (scrollable) + **Add mod** button. |
| 10 | Animation | **Simple only**: hover/fade on buttons & cards (~120 ms), fade-in on page change / modal open (~150 ms), smooth progress bar, spinner while loading. **No** background motion, parallax, or sliding pages. Must respect `MotionMode` Full / Reduced / Off (Off = instant). |

## 3. Carry-over rules (unchanged, from `.agent/rules` + earlier owner requests)
- Multi-choice options use `ComboBox`, never rows of buttons.
- Every list inside a panel is scrollable (`ScrollArea`, `auto_shrink([false,false])`), never hidden under another widget.
- Every OS window opens centered on screen.
- Every save/export uses `SaveFileDialog`; dismissible notices auto-hide after 8 s (configurable).
- Tags/groups chips auto-size to text and wrap responsively.
- Reuse Python i18n keys (`lang/en-US.json`, `lang/vi-VN.json`); no invented translations. Always check the Python source first.
- `VerificationCache` / no re-download of unchanged assets.
- SOLID: each new view in its own module; split `dialogs.rs`, `controllers.rs`, `workspace.rs` while rebuilding (see `docs/BACKLOG.md`).
- Updater stays disabled; no Python/Cloudflare update branch.

## 4. Visual language
- **Canvas:** full-window background image (dimmed) → large rounded floating panels (radius ≈ 10–14 px) with 1 px soft border and semi-transparent fill; pills (radius full) for nav and tabs; primary button = accent fill, large rounded.
- **Shadows/gradients:** only subtle panel border/overlay needed for legibility; no decorative gradients beyond the background image itself.
- **Typography:** system UI sans (Segoe UI / Noto Sans, full Vietnamese diacritics), monospace for paths/versions/logs. Big bold page titles (~28–32 px), compact body (12–13 px).
- **Icons:** single-style monochrome line icons with text labels for primary actions; no mixed emoji.
- **Contrast:** text color is always computed against the panel+background mix (accent-aware); status never by color alone.
- **Default window:** ~1100×700, minimum ~860×560; panels reflow on resize.

## 5. Delivery plan (SemVer tags, no separate hotfix system)

1. **`v0.11.0-beta.1` — Foundation + Shell** *(Completed)*
   - Background image system (bundled default, user picture, panel transparency, dim sliders).
   - New palette model + presets (Violet default, Sunrise, Neon, Ocean, Emerald) + custom accent; keep `themes/*.json`.
   - Shell: top pill nav, left icon rail, instance flyout, account chip, Settings → in-app modal. (Global `Ctrl+K` search is deferred to v0.13.)
   - Simple animation helpers (hover/fade) honoring `MotionMode`.
2. **`v0.12.0-beta.1` — Pages** *(Completed)*
   - Home/instance page (title, Play, quick icons, floating tabbed panel, `...` menu).
   - Versions page (`cache/versions/` + Mojang manifest browser).
   - New centered OS-window content browser (`Mods`, `Modpacks`, `Resource packs`, `Shader packs`, `Data packs` — `ComboBox` filters + split list & detail panel).
   - Servers/LAN page (`servers.dat` NBT manager + Quick Play + LAN/e4mc), Logs page (unified Global & per-instance viewer).
3. **`v0.13.0-beta.1` — Polish**
   - Restyle every remaining dialog (create instance, accounts, repair, manual downloads, optional files, etc.).
   - Final animation pass, i18n gaps, remove dead UI code.
   - Update `README.md`, `CHANGELOG.md`, this file's status.

Each stage ends with: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, release build, manual QA list, local commit + annotated tag (no push unless asked).

## 6. What MCW still avoids
- Ads, sponsor banners, shop/wardrobe/fake stats, fake progress bars.
- Heavy or continuous animation; blur/glass effects that egui cannot do honestly.
- Mixed icon styles, oversized mobile-style controls, huge empty hero areas that push controls below the fold.

## 7. Approval rule
Changes to foundational identity (palette model, typography, icon system, navigation model, global component language) require owner review. This v2.0 document is that approval for the Horizon-style rebuild.
