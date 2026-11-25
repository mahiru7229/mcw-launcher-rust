use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_backups_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
        busy: bool,
    ) {
        let p = self.palette;
        let disabled = busy || self.backup_busy;

        ui.horizontal(|ui| {
            ui.label(RichText::new("Backup Note:").strong());
            ui.add(
                TextEdit::singleline(&mut self.backup_note_input)
                    .hint_text("Optional note (e.g. before installing mods)...")
                    .desired_width(260.0),
            );
            let create_btn = Button::new(
                RichText::new(if self.backup_busy {
                    "Working..."
                } else {
                    "Create Backup"
                })
                .color(p.accent_text)
                .strong(),
            )
            .fill(p.accent);
            let resp = ui.add_enabled(!disabled, create_btn);
            if busy {
                resp.on_disabled_hover_text("Stop the instance before creating a backup");
            } else if resp.clicked() {
                self.start_backup_create(ctx);
            }
            if ui.button("Refresh").clicked() {
                self.load_backups_async(&inst.instance_id, ctx, true);
            }
        });

        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("Saved Backups ({})", self.selected_backups.len()))
                .strong()
                .color(p.text),
        );
        ui.add_space(4.0);

        if self.selected_backups.is_empty() {
            let msg = if self.loading_backups.contains(&inst.instance_id) {
                "Loading backups..."
            } else {
                "No backups created for this instance yet."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        let mut delete_id: Option<String> = None;
        ScrollArea::vertical()
            .id_salt("backups_list_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for b in &self.selected_backups {
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&b.id).monospace().strong());
                                    let note_str = if b.note.is_empty() {
                                        "No note".to_string()
                                    } else {
                                        b.note.clone()
                                    };
                                    ui.label(
                                        RichText::new(format!(
                                            "{}  ·  {}",
                                            format_bytes(b.size_bytes),
                                            note_str
                                        ))
                                        .size(11.5)
                                        .color(p.text_muted),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .add_enabled(
                                            !disabled,
                                            Button::new(RichText::new("Delete").color(p.danger)),
                                        )
                                        .clicked()
                                    {
                                        delete_id = Some(b.id.clone());
                                    }
                                    if ui.add_enabled(!disabled, Button::new("Restore")).clicked() {
                                        self.pending_restore_backup_id = Some(b.id.clone());
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });

        if let Some(id) = delete_id {
            match BackupManager::new(&self.paths).delete_backup(&inst.instance_id, &id) {
                Ok(()) => {
                    self.backups_cache.remove(&inst.instance_id);
                    self.load_backups_async(&inst.instance_id, ctx, true);
                    self.notice_banner = Some((format!("Deleted backup '{id}'."), false));
                }
                Err(e) => {
                    self.notice_banner = Some((format!("Failed to delete backup: {e}"), true));
                }
            }
        }
    }

    pub(crate) fn render_mods_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
    ) {
        if self.instance_inline_browser == Some(BrowserCategory::Mods) {
            self.render_inline_instance_browser(ui, ctx);
            return;
        }
        let p = self.palette;
        let mods_dir = self.paths.minecraft_dir(&inst.instance_id).join("mods");
        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let provenance = ModProvenanceRegistry::load(&inst_dir);
        let modpack_protected = self.is_instance_modpack_protected(inst);
        let has_official_pack = detect_installed_pack(&inst_dir).is_some();
        let unlocked_updates = self
            .mod_update_candidates
            .iter()
            .filter(|c| c.update_available && !c.locked)
            .count();

        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("Installed Mods ({})", self.selected_mods.len()))
                    .strong()
                    .color(p.text),
            );
            let can_install = !inst.mod_loader.is_vanilla();
            let add_btn = Button::new(RichText::new("+ Add Mod").color(p.accent_text).strong())
                .fill(p.accent)
                .corner_radius(CornerRadius::same(8));
            let add_resp = ui.add_enabled(can_install, add_btn);
            if !can_install {
                add_resp.on_disabled_hover_text("Vanilla instances cannot install mods");
            } else if add_resp.clicked() {
                self.open_content_browser(BrowserCategory::Mods, None, ctx);
            }

            if has_official_pack {
                let busy = self.is_instance_busy(&inst.instance_id);
                if ui
                    .add_enabled(!busy, Button::new("Check Modpack Update"))
                    .on_disabled_hover_text("Stop the instance before updating")
                    .clicked()
                {
                    self.open_modpack_update_window(inst.instance_id.clone(), ctx);
                }
            }
            if !modpack_protected {
                let check_lbl = if self.checking_mod_updates {
                    "Checking Updates..."
                } else {
                    "Check Mod Updates"
                };
                if ui
                    .add_enabled(!self.checking_mod_updates, Button::new(check_lbl))
                    .clicked()
                {
                    self.start_check_mod_updates(ctx);
                }
                if unlocked_updates > 0 {
                    let upd_btn = Button::new(
                        RichText::new(format!("Update Unlocked ({unlocked_updates})"))
                            .color(p.accent_text)
                            .strong(),
                    )
                    .fill(p.accent);
                    if ui.add(upd_btn).clicked() {
                        self.start_apply_mod_updates(ctx);
                    }
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Open mods/ Folder").clicked() {
                    let _ = std::fs::create_dir_all(&mods_dir);
                    open_external(&mods_dir.display().to_string());
                }
                if ui.button("Export Modlist...").clicked() {
                    let default_name = format!("{}-modlist.md", inst.instance_id);
                    if let Some(dest_path) = save_file_dialog(
                        "Export Modlist",
                        "Markdown (*.md)|*.md|Text File (*.txt)|*.txt|All Files (*.*)|*.*",
                        &default_name,
                    ) {
                        let as_md = !dest_path
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e.eq_ignore_ascii_case("txt"));
                        let body = format_modlist_export(
                            &inst.name,
                            &inst.version_id,
                            &inst.mod_loader.name,
                            &self.selected_mods,
                            as_md,
                        );
                        match std::fs::write(&dest_path, body) {
                            Ok(()) => {
                                self.notice_banner = Some((
                                    format!("Exported modlist to {}", dest_path.display()),
                                    false,
                                ));
                            }
                            Err(e) => {
                                self.notice_banner =
                                    Some((format!("Failed to export modlist: {e}"), true));
                            }
                        }
                    }
                }
                if ui
                    .add_enabled(can_install, Button::new("Import Local .jar..."))
                    .clicked()
                {
                    if let Some(chosen) = pick_file_dialog(
                        "Import Mod JAR",
                        "Mod JAR (*.jar;*.jar.disabled)|*.jar;*.jar.disabled|All Files (*.*)|*.*",
                    ) {
                        self.import_local_mod_files(&inst.instance_id, &[chosen], ctx);
                    }
                }
                if ui.button("Refresh Local").clicked() {
                    self.load_mods_async(&inst.instance_id, ctx, true);
                }
            });
        });
        let mut open_compat_repair = false;
        if !modpack_protected {
            if let Some(ref compat_rep) = self.mod_compat_report {
                if !compat_rep.issues.is_empty() {
                    ui.add_space(4.0);
                    let frame_col = if compat_rep.error_count > 0 {
                        p.danger
                    } else {
                        p.warning
                    };
                    Frame::new()
                        .fill(p.elevated)
                        .stroke(Stroke::new(1.0_f32, frame_col))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "Compatibility Check: {} error(s), {} warning(s)",
                                        compat_rep.error_count, compat_rep.warning_count
                                    ))
                                    .strong()
                                    .size(11.5)
                                    .color(frame_col),
                                );
                                if ui.button("Open Repair Center...").clicked() {
                                    open_compat_repair = true;
                                }
                            });
                            for issue in compat_rep.issues.iter().take(2) {
                                ui.label(
                                    RichText::new(format!("• {}", issue.message))
                                        .size(11.0)
                                        .color(p.text_muted),
                                );
                            }
                        });
                }
            }
        }
        if open_compat_repair {
            self.open_repair_center(&inst.instance_id, ctx);
        }
        ui.add_space(6.0);

        if self.selected_mods.is_empty() {
            self.selected_mod_files.clear();
            let msg = if self.loading_mods.contains(&inst.instance_id) {
                "Scanning mods/ in background..."
            } else {
                "No mods installed in this instance's mods/ folder. Click '+ Add Mod' to browse Modrinth or CurseForge."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        if modpack_protected && self.mod_status_filter == "updates" {
            self.mod_status_filter = "all".to_string();
        }
        let installed_names: std::collections::HashSet<&str> = self
            .selected_mods
            .iter()
            .map(|m| m.file_name.as_str())
            .collect();
        self.selected_mod_files
            .retain(|name| installed_names.contains(name.as_str()));

        let update_clean_names: std::collections::HashSet<String> = if modpack_protected {
            std::collections::HashSet::new()
        } else {
            self.mod_update_candidates
                .iter()
                .filter(|c| c.update_available)
                .map(|c| {
                    c.current_file_name
                        .trim_end_matches(".disabled")
                        .to_string()
                })
                .collect()
        };

        let visible_mods: Vec<mcw_mods::ModInfo> = self
            .selected_mods
            .iter()
            .filter(|m| {
                let clean = m.file_name.trim_end_matches(".disabled");
                let has_upd = update_clean_names.contains(clean);
                matches_mod_filter(
                    m,
                    &self.mod_search_query,
                    &self.mod_status_filter,
                    &inst.mod_loader.name,
                    has_upd,
                )
            })
            .cloned()
            .collect();

        ui.horizontal_wrapped(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.mod_search_query)
                    .hint_text("Filter installed mods...")
                    .desired_width(210.0),
            );
            let cur_filter_lbl = match self.mod_status_filter.as_str() {
                "enabled" => "Enabled",
                "disabled" => "Disabled",
                "updates" => "Updates Available",
                "incompatible" => "Incompatible",
                _ => "All",
            };
            egui::ComboBox::from_id_salt("cb_mod_status_filter")
                .selected_text(cur_filter_lbl)
                .width(150.0)
                .truncate()
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.mod_status_filter, "all".to_string(), "All");
                    ui.selectable_value(
                        &mut self.mod_status_filter,
                        "enabled".to_string(),
                        "Enabled",
                    );
                    ui.selectable_value(
                        &mut self.mod_status_filter,
                        "disabled".to_string(),
                        "Disabled",
                    );
                    if !modpack_protected {
                        ui.selectable_value(
                            &mut self.mod_status_filter,
                            "updates".to_string(),
                            "Updates Available",
                        );
                    }
                    ui.selectable_value(
                        &mut self.mod_status_filter,
                        "incompatible".to_string(),
                        "Incompatible",
                    );
                });
            if ui
                .add_enabled(!visible_mods.is_empty(), Button::new("Select Visible"))
                .clicked()
            {
                for m in &visible_mods {
                    self.selected_mod_files.insert(m.file_name.clone());
                }
            }
            if ui
                .add_enabled(
                    !self.selected_mod_files.is_empty(),
                    Button::new("Clear Selection"),
                )
                .clicked()
            {
                self.selected_mod_files.clear();
            }
            if visible_mods.len() != self.selected_mods.len() {
                ui.label(
                    RichText::new(format!(
                        "Showing {} / {}",
                        visible_mods.len(),
                        self.selected_mods.len()
                    ))
                    .size(11.5)
                    .color(p.text_muted),
                );
            }
        });
        ui.add_space(6.0);

        #[derive(Clone, Copy)]
        enum BulkModAction {
            Enable,
            Disable,
            Lock,
            Unlock,
            Remove,
        }
        let mut bulk_action: Option<BulkModAction> = None;
        if !self.selected_mod_files.is_empty() {
            Frame::new()
                .fill(p.elevated)
                .stroke(Stroke::new(1.0_f32, p.accent))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("{} selected", self.selected_mod_files.len()))
                                .strong()
                                .color(p.accent),
                        );
                        if ui.button("Enable").clicked() {
                            bulk_action = Some(BulkModAction::Enable);
                        }
                        if ui.button("Disable").clicked() {
                            bulk_action = Some(BulkModAction::Disable);
                        }
                        if !modpack_protected {
                            if ui.button("Lock").clicked() {
                                bulk_action = Some(BulkModAction::Lock);
                            }
                            if ui.button("Unlock").clicked() {
                                bulk_action = Some(BulkModAction::Unlock);
                            }
                        }
                        if ui.button(RichText::new("Remove").color(p.danger)).clicked() {
                            bulk_action = Some(BulkModAction::Remove);
                        }
                        if ui.button("Clear").clicked() {
                            self.selected_mod_files.clear();
                        }
                    });
                });
            ui.add_space(6.0);
        }

        if visible_mods.is_empty() {
            ui.label(
                RichText::new("No installed mods match the current search or status filter.")
                    .color(p.text_muted),
            );
            return;
        }

        let mut toggle_action: Option<(String, bool)> = None;
        let mut delete_action: Option<String> = None;
        let mut lock_toggle_action: Option<(String, bool)> = None;

        ScrollArea::vertical()
            .id_salt("local_mods_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for m in &visible_mods {
                    let compat = is_loader_compatible(m.loader, &inst.mod_loader.name);
                    let mut is_sel = self.selected_mod_files.contains(&m.file_name);
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(
                            1.0_f32,
                            if is_sel { p.accent } else { p.border },
                        ))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                if ui
                                    .checkbox(&mut is_sel, "")
                                    .on_hover_text("Select for bulk action")
                                    .changed()
                                {
                                    if is_sel {
                                        self.selected_mod_files.insert(m.file_name.clone());
                                    } else {
                                        self.selected_mod_files.remove(&m.file_name);
                                    }
                                }
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&m.name).strong());
                                        ui.label(
                                            RichText::new(&m.version)
                                                .monospace()
                                                .size(11.0)
                                                .color(p.text_muted),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "{:?} · Java {}+",
                                                m.loader, m.required_java_major
                                            ))
                                            .size(11.0)
                                            .color(p.text_muted),
                                        );
                                        if !compat {
                                            ui.label(
                                                RichText::new("INCOMPATIBLE")
                                                    .size(11.0)
                                                    .color(p.warning),
                                            );
                                        }
                                        if !modpack_protected {
                                            let clean_fname =
                                                m.file_name.trim_end_matches(".disabled");
                                            if let Some(cand) =
                                                self.mod_update_candidates.iter().find(|c| {
                                                    c.current_file_name.trim_end_matches(".disabled")
                                                        == clean_fname
                                                })
                                            {
                                                if cand.update_available {
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "UPDATE -> {}",
                                                            cand.latest_version_number
                                                        ))
                                                        .strong()
                                                        .monospace()
                                                        .size(10.5)
                                                        .color(p.accent),
                                                    );
                                                }
                                            }
                                        }
                                    });
                                    ui.label(
                                        RichText::new(&m.file_name)
                                            .monospace()
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button(RichText::new("Remove").color(p.danger)).clicked()
                                    {
                                        delete_action = Some(m.file_name.clone());
                                    }
                                    if !modpack_protected {
                                        let clean_fname = m.file_name.trim_end_matches(".disabled");
                                        if let Some(tracked) = provenance.mods.values().find(|e| {
                                            e.file_name.trim_end_matches(".disabled") == clean_fname
                                        }) {
                                            let lock_lbl =
                                                if tracked.locked { "Locked" } else { "Lock" };
                                            let lock_col = if tracked.locked {
                                                p.warning
                                            } else {
                                                p.text_muted
                                            };
                                            if ui
                                                .button(
                                                    RichText::new(lock_lbl)
                                                        .size(11.0)
                                                        .color(lock_col),
                                                )
                                                .on_hover_text(
                                                    "Lock or unlock this mod from automatic updates",
                                                )
                                                .clicked()
                                            {
                                                lock_toggle_action =
                                                    Some((m.file_name.clone(), !tracked.locked));
                                            }
                                        }
                                    }
                                    let (state_lbl, state_col) = if m.enabled {
                                        ("Enabled", p.accent)
                                    } else {
                                        ("Disabled", p.text_muted)
                                    };
                                    if ui
                                        .button(
                                            RichText::new(state_lbl).size(11.0).color(state_col),
                                        )
                                        .on_hover_text("Click to toggle enabled/disabled")
                                        .clicked()
                                    {
                                        toggle_action = Some((m.file_name.clone(), !m.enabled));
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });

        if let Some(action) = bulk_action {
            let targets: Vec<mcw_mods::ModInfo> = self
                .selected_mods
                .iter()
                .filter(|m| self.selected_mod_files.contains(&m.file_name))
                .cloned()
                .collect();
            let mut changed = 0usize;
            let mut errors = 0usize;
            match action {
                BulkModAction::Enable | BulkModAction::Disable => {
                    let enable = matches!(action, BulkModAction::Enable);
                    let mgr = ModManager::new(&mods_dir);
                    for m in &targets {
                        if m.enabled != enable {
                            match mgr.set_enabled(&m.file_name, enable) {
                                Ok(_) => changed += 1,
                                Err(_) => errors += 1,
                            }
                        }
                    }
                    let verb = if enable { "Enabled" } else { "Disabled" };
                    self.notice_banner = Some((
                        if errors > 0 {
                            format!("{verb} {changed} mod(s), {errors} failed.")
                        } else {
                            format!("{verb} {changed} mod(s).")
                        },
                        errors > 0,
                    ));
                    self.mods_cache.remove(&inst.instance_id);
                    self.load_mods_async(&inst.instance_id, ctx, true);
                }
                BulkModAction::Lock | BulkModAction::Unlock => {
                    let new_locked = matches!(action, BulkModAction::Lock);
                    for m in &targets {
                        if let Ok(true) = ModProvenanceRegistry::set_locked_by_filename(
                            &inst_dir,
                            &m.file_name,
                            new_locked,
                        ) {
                            changed += 1;
                            let clean = m.file_name.trim_end_matches(".disabled");
                            for c in &mut self.mod_update_candidates {
                                if c.current_file_name.trim_end_matches(".disabled") == clean {
                                    c.locked = new_locked;
                                }
                            }
                        }
                    }
                    let verb = if new_locked { "Locked" } else { "Unlocked" };
                    self.notice_banner = Some((format!("{verb} {changed} tracked mod(s)."), false));
                }
                BulkModAction::Remove => {
                    let mgr = ModManager::new(&mods_dir);
                    for m in &targets {
                        match mgr.delete(&m.file_name) {
                            Ok(()) => changed += 1,
                            Err(_) => errors += 1,
                        }
                    }
                    self.notice_banner = Some((
                        if errors > 0 {
                            format!("Removed {changed} mod(s), {errors} failed.")
                        } else {
                            format!("Removed {changed} mod(s).")
                        },
                        errors > 0,
                    ));
                    self.mods_cache.remove(&inst.instance_id);
                    self.load_mods_async(&inst.instance_id, ctx, true);
                }
            }
            self.selected_mod_files.clear();
            return;
        }

        if let Some((file_name, new_locked)) = lock_toggle_action {
            let _ =
                ModProvenanceRegistry::set_locked_by_filename(&inst_dir, &file_name, new_locked);
            let clean = file_name.trim_end_matches(".disabled");
            for c in &mut self.mod_update_candidates {
                if c.current_file_name.trim_end_matches(".disabled") == clean {
                    c.locked = new_locked;
                }
            }
        }
        if let Some((file_name, enable)) = toggle_action {
            if let Err(e) = ModManager::new(&mods_dir).set_enabled(&file_name, enable) {
                self.notice_banner = Some((format!("Failed to toggle mod: {e}"), true));
            }
            self.mods_cache.remove(&inst.instance_id);
            self.load_mods_async(&inst.instance_id, ctx, true);
        }
        if let Some(file_name) = delete_action {
            if let Err(e) = ModManager::new(&mods_dir).delete(&file_name) {
                self.notice_banner = Some((format!("Failed to delete mod: {e}"), true));
            }
            self.mods_cache.remove(&inst.instance_id);
            self.load_mods_async(&inst.instance_id, ctx, true);
        }
    }

    pub(crate) fn render_content_pack_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
        kind: ContentKind,
    ) {
        let p = self.palette;
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);

        let (kind_singular, section_title, browser_cat) = match kind {
            ContentKind::ResourcePack => (
                "Resource Pack",
                format!(
                    "Installed Resource Packs ({})",
                    self.selected_content_packs.len()
                ),
                BrowserCategory::ResourcePacks,
            ),
            ContentKind::ShaderPack => (
                "Shader Pack",
                format!(
                    "Installed Shader Packs ({})",
                    self.selected_content_packs.len()
                ),
                BrowserCategory::ShaderPacks,
            ),
            ContentKind::DataPack => (
                "Data Pack",
                format!(
                    "Installed Data Packs ({})",
                    self.selected_content_packs.len()
                ),
                BrowserCategory::DataPacks,
            ),
        };
        if self.instance_inline_browser == Some(browser_cat) {
            self.render_inline_instance_browser(ui, ctx);
            return;
        }

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(section_title).strong().color(p.text));
            let add_btn = Button::new(
                RichText::new(format!("+ Add {kind_singular}"))
                    .color(p.accent_text)
                    .strong(),
            )
            .fill(p.accent)
            .corner_radius(CornerRadius::same(8));
            if ui.add(add_btn).clicked() {
                self.open_content_browser(browser_cat, None, ctx);
            }
            if matches!(kind, ContentKind::ResourcePack | ContentKind::ShaderPack) {
                let is_checking_this =
                    self.checking_pack_updates && self.pack_updates_kind == Some(kind);
                let check_lbl = if is_checking_this {
                    "Checking Pack Updates..."
                } else {
                    "Check Pack Updates"
                };
                if ui
                    .add_enabled(!self.checking_pack_updates, Button::new(check_lbl))
                    .clicked()
                {
                    self.start_check_content_pack_updates(kind, ctx);
                }
                if self.pack_updates_kind == Some(kind) {
                    let pack_updatable = self
                        .pack_update_candidates
                        .iter()
                        .filter(|c| c.update_available)
                        .count();
                    if pack_updatable > 0 {
                        let upd_btn = Button::new(
                            RichText::new(format!("Update Packs ({pack_updatable})"))
                                .color(p.accent_text)
                                .strong(),
                        )
                        .fill(p.accent);
                        if ui.add(upd_btn).clicked() {
                            self.start_apply_content_pack_updates(kind, ctx);
                        }
                    }
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let folder_dir = mc_dir.join(kind.folder_name());
                if ui.button("Open Folder").clicked() {
                    let _ = std::fs::create_dir_all(&folder_dir);
                    open_external(&folder_dir.display().to_string());
                }
                if ui.button("Import Local .zip...").clicked() {
                    if let Some(chosen) = pick_file_dialog(
                        "Import Pack Archive (.zip)",
                        "Pack Archive (*.zip;*.zip.disabled)|*.zip;*.zip.disabled|All Files (*.*)|*.*",
                    ) {
                        self.import_local_pack_files(&inst.instance_id, kind, &[chosen], ctx);
                    }
                }
                if ui.button("Refresh").clicked() {
                    self.load_content_packs_async(&inst.instance_id, kind, ctx, true);
                }
            });
        });

        ui.add_space(6.0);

        if self.selected_content_packs.is_empty() {
            let msg = if self
                .loading_content_packs
                .contains(&(inst.instance_id.clone(), kind))
            {
                "Scanning folder in background..."
            } else {
                "No packs installed in this folder yet. Click '+ Add' above to browse Modrinth or CurseForge."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        let mut toggle_action: Option<(String, bool)> = None;
        let mut delete_action: Option<String> = None;
        let packs_snapshot = self.selected_content_packs.clone();

        ScrollArea::vertical()
            .id_salt("local_content_packs_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for pack in &packs_snapshot {
                    let icon_tex = self.load_pack_icon_texture(pack, ctx);
                    let pack_upd = if self.pack_updates_kind == Some(kind) {
                        self.pack_update_candidates
                            .iter()
                            .find(|c| c.update_available && c.file_name == pack.file_name)
                            .map(|c| c.latest_version_number.clone())
                    } else {
                        None
                    };
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let mut en = pack.enabled;
                                if ui.checkbox(&mut en, "").changed() {
                                    toggle_action = Some((pack.file_name.clone(), en));
                                }
                                if let Some(ref tex) = icon_tex {
                                    ui.add(
                                        egui::Image::new((tex.id(), Vec2::splat(32.0)))
                                            .corner_radius(CornerRadius::same(6)),
                                    );
                                } else {
                                    let (r, _) = ui.allocate_exact_size(
                                        Vec2::splat(32.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect(
                                        r,
                                        CornerRadius::same(6),
                                        p.elevated,
                                        Stroke::new(1.0_f32, p.border),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                ui.vertical(|ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(RichText::new(&pack.display_name).strong());
                                        if let Some(ref new_ver) = pack_upd {
                                            ui.label(
                                                RichText::new(format!("UPDATE -> {new_ver}"))
                                                    .strong()
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(p.accent),
                                            );
                                        }
                                    });
                                    let meta = if pack.is_dir {
                                        format!("{} · Folder", pack.file_name)
                                    } else {
                                        format!(
                                            "{} · {}",
                                            pack.file_name,
                                            format_bytes(pack.size_bytes)
                                        )
                                    };
                                    ui.label(
                                        RichText::new(meta)
                                            .monospace()
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button(RichText::new("Remove").color(p.danger)).clicked()
                                    {
                                        delete_action = Some(pack.file_name.clone());
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });

        if let Some((file_name, enable)) = toggle_action {
            if let Err(e) = ContentPackManager::set_enabled(&mc_dir, kind, &file_name, enable) {
                self.notice_banner = Some((format!("Failed to toggle pack: {e}"), true));
            }
            self.content_packs_cache
                .remove(&(inst.instance_id.clone(), kind));
            self.load_content_packs_async(&inst.instance_id, kind, ctx, true);
        }
        if let Some(file_name) = delete_action {
            if let Err(e) = ContentPackManager::delete(&mc_dir, kind, &file_name) {
                self.notice_banner = Some((format!("Failed to delete pack: {e}"), true));
            }
            self.content_packs_cache
                .remove(&(inst.instance_id.clone(), kind));
            self.load_content_packs_async(&inst.instance_id, kind, ctx, true);
        }
    }

    pub(crate) fn render_worlds_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
        busy: bool,
    ) {
        let p = self.palette;
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let saves_dir = mc_dir.join("saves");
        let quick_play_ok = is_quick_play_supported(&inst.version_id);
        let missing_manual = self
            .pending_manual_counts
            .get(&inst.instance_id)
            .copied()
            .unwrap_or(0);

        let mut import_world_clicked = false;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "Singleplayer Worlds ({})",
                    self.selected_worlds.len()
                ))
                .strong()
                .color(p.text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Open saves/ Folder").clicked() {
                    let _ = std::fs::create_dir_all(&saves_dir);
                    open_external(&saves_dir.display().to_string());
                }
                if ui.button("Refresh").clicked() {
                    self.load_worlds_async(&inst.instance_id, ctx, true);
                }
                if ui
                    .add_enabled(!busy, Button::new("Import World (.zip)"))
                    .clicked()
                {
                    import_world_clicked = true;
                }
            });
        });
        if import_world_clicked {
            if let Some(chosen) = pick_file_dialog(
                "Import Minecraft World (.zip)",
                "World Archives (*.zip)|*.zip|All Files (*.*)|*.*",
            ) {
                match WorldManager::import_world(&mc_dir, &chosen) {
                    Ok(world_name) => {
                        self.worlds_cache.remove(&inst.instance_id);
                        self.load_worlds_async(&inst.instance_id, ctx, true);
                        self.notice_banner =
                            Some((format!("Imported world '{world_name}'."), false));
                    }
                    Err(e) => {
                        self.notice_banner = Some((format!("World import failed: {e}"), true));
                    }
                }
            }
        }

        ui.add_space(6.0);

        if self.selected_worlds.is_empty() {
            let msg = if self.loading_worlds.contains(&inst.instance_id) {
                "Scanning saves/ in background..."
            } else {
                "No singleplayer worlds found in this instance's saves/ folder."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        let mut quick_play_target: Option<String> = None;
        let mut copy_seed_target: Option<i64> = None;
        let mut export_world_target: Option<String> = None;
        let mut duplicate_target: Option<String> = None;
        let mut backup_target: Option<String> = None;
        let mut delete_target: Option<String> = None;

        ScrollArea::vertical()
            .id_salt("worlds_list_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let worlds_snapshot = self.selected_worlds.clone();
                for w in &worlds_snapshot {
                    let world_tex = w
                        .icon_path
                        .as_deref()
                        .and_then(|ip| self.load_world_icon_texture(ip, ctx));
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                if let Some(ref tex) = world_tex {
                                    ui.add(
                                        egui::Image::new(tex)
                                            .fit_to_exact_size(Vec2::splat(42.0))
                                            .corner_radius(CornerRadius::same(6)),
                                    );
                                } else {
                                    Frame::new()
                                        .fill(p.elevated)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .corner_radius(CornerRadius::same(6))
                                        .show(ui, |ui| {
                                            ui.set_min_size(Vec2::splat(42.0));
                                            ui.centered_and_justified(|ui| {
                                                ui.label(
                                                    RichText::new("W").strong().color(p.text_muted),
                                                );
                                            });
                                        });
                                }
                                ui.add_space(6.0);
                                ui.vertical(|ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(
                                            RichText::new(&w.display_name).strong().size(13.5),
                                        );
                                        let mode_color =
                                            if w.is_hardcore { p.danger } else { p.accent };
                                        ui.label(
                                            RichText::new(w.game_mode.to_uppercase())
                                                .monospace()
                                                .size(10.5)
                                                .color(mode_color),
                                        );
                                        if !w.version_name.is_empty() {
                                            ui.label(
                                                RichText::new(format!("MC {}", w.version_name))
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(p.text_muted),
                                            );
                                        }
                                        if let Some(seed) = w.seed {
                                            ui.label(
                                                RichText::new(format!("Seed: {seed}"))
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(p.text_muted),
                                            );
                                        }
                                    });
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · {}",
                                            w.folder_name,
                                            format_bytes(w.size_bytes)
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(p.text_muted),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .add_enabled(
                                            !busy,
                                            Button::new(RichText::new("Delete").color(p.danger)),
                                        )
                                        .clicked()
                                    {
                                        delete_target = Some(w.folder_name.clone());
                                    }
                                    if ui.button("Folder").clicked() {
                                        open_external(&w.folder_path.display().to_string());
                                    }
                                    if ui.add_enabled(!busy, Button::new("Duplicate")).clicked() {
                                        duplicate_target = Some(w.folder_name.clone());
                                    }
                                    if ui
                                        .add_enabled(!busy, Button::new("Export (.zip)"))
                                        .clicked()
                                    {
                                        export_world_target = Some(w.folder_name.clone());
                                    }
                                    if ui
                                        .add_enabled(!busy, Button::new("Backup (.zip)"))
                                        .clicked()
                                    {
                                        backup_target = Some(w.folder_name.clone());
                                    }
                                    if let Some(seed) = w.seed {
                                        if ui.button("Copy Seed").clicked() {
                                            copy_seed_target = Some(seed);
                                        }
                                    }
                                    if quick_play_ok {
                                        let can_play = !busy && missing_manual == 0;
                                        let play_btn = Button::new(
                                            RichText::new("Quick Play")
                                                .color(p.accent_text)
                                                .strong(),
                                        )
                                        .fill(p.accent);
                                        let resp = ui.add_enabled(can_play, play_btn);
                                        if resp.clicked() {
                                            quick_play_target = Some(w.folder_name.clone());
                                        }
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });

        if let Some(seed) = copy_seed_target {
            ctx.copy_text(seed.to_string());
            self.notice_banner = Some((format!("Copied world seed: {seed}"), false));
        }
        if let Some(folder) = export_world_target {
            let default_name = format!("{folder}.zip");
            if let Some(dest_path) = save_file_dialog(
                "Export World (.zip)",
                "Zip Archive (*.zip)|*.zip|All Files (*.*)|*.*",
                &default_name,
            ) {
                match WorldManager::export_world(&mc_dir, &folder, &dest_path) {
                    Ok(count) => {
                        self.notice_banner = Some((
                            format!(
                                "Exported world '{}' ({count} files) to {}",
                                folder,
                                dest_path.display()
                            ),
                            false,
                        ));
                    }
                    Err(e) => {
                        self.notice_banner = Some((format!("World export failed: {e}"), true));
                    }
                }
            }
        }
        if let Some(folder) = quick_play_target {
            self.start_launch_with_world(Some(folder), ctx);
        }
        if let Some(folder) = backup_target {
            let default_name = format!("world_{folder}.zip");
            if let Some(dest_path) = save_file_dialog(
                "Save World Backup (.zip)",
                "Zip Archive (*.zip)|*.zip|All Files (*.*)|*.*",
                &default_name,
            ) {
                match WorldManager::backup_world(&mc_dir, &folder, None) {
                    Ok(zip_path) => {
                        let final_path = if dest_path != zip_path {
                            let _ = std::fs::copy(&zip_path, &dest_path);
                            dest_path
                        } else {
                            zip_path
                        };
                        self.notice_banner = Some((
                            format!("Backed up world '{}' to {}", folder, final_path.display()),
                            false,
                        ));
                    }
                    Err(e) => {
                        self.notice_banner = Some((format!("World backup failed: {e}"), true));
                    }
                }
            }
        }
        if let Some(folder) = duplicate_target {
            match WorldManager::duplicate_world(&mc_dir, &folder) {
                Ok(dup) => {
                    self.worlds_cache.remove(&inst.instance_id);
                    self.load_worlds_async(&inst.instance_id, ctx, true);
                    self.notice_banner = Some((
                        format!("Duplicated world '{}' -> '{}'.", folder, dup.folder_name),
                        false,
                    ));
                }
                Err(e) => {
                    self.notice_banner = Some((format!("Duplicate world failed: {e}"), true));
                }
            }
        }
        if let Some(folder) = delete_target {
            self.pending_delete_world = Some((inst.instance_id.clone(), folder));
        }
    }

    pub(crate) fn render_screenshots_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
    ) {
        let p = self.palette;
        let shots_dir = self
            .paths
            .minecraft_dir(&inst.instance_id)
            .join("screenshots");

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Screenshots ({})", self.selected_screenshots.len()))
                    .strong()
                    .color(p.text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Open screenshots/ Folder").clicked() {
                    let _ = std::fs::create_dir_all(&shots_dir);
                    open_external(&shots_dir.display().to_string());
                }
                if ui.button("Refresh").clicked() {
                    self.load_screenshots_async(&inst.instance_id, ctx, true);
                }
            });
        });

        ui.add_space(6.0);

        if self.selected_screenshots.is_empty() {
            let msg = if self.loading_screenshots.contains(&inst.instance_id) {
                "Scanning screenshots/ in background..."
            } else {
                "No screenshots captured in this instance yet (press F2 in-game)."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        let mut delete_path: Option<PathBuf> = None;
        let mut open_preview: Option<PathBuf> = None;
        let shots_snapshot = self.selected_screenshots.clone();
        for s in &shots_snapshot {
            self.ensure_screenshot_thumbnail(&s.path, ctx);
        }

        let avail_w = ui.available_width().max(220.0);
        let spacing = 10.0_f32;
        let cols = ((avail_w + spacing) / (216.0 + spacing)).floor().max(1.0) as usize;
        let card_w =
            ((avail_w - spacing * ((cols.saturating_sub(1)) as f32)) / (cols as f32)).max(180.0);

        ScrollArea::vertical()
            .id_salt("screenshots_list_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in shots_snapshot.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = spacing;
                        for s in row {
                            let thumb_tex =
                                self.screenshot_thumbs.get(&s.path).and_then(|o| o.as_ref());
                            Frame::new()
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .corner_radius(CornerRadius::same(10))
                                .inner_margin(egui::Margin::same(8))
                                .show(ui, |ui| {
                                    ui.set_width(card_w - 18.0);
                                    let img_w = (card_w - 20.0).max(150.0);
                                    let img_h = (img_w * 0.5625).clamp(86.0, 145.0);
                                    if let Some(tex) = thumb_tex {
                                        let img_resp = ui.add(
                                            egui::Image::new(tex)
                                                .fit_to_exact_size(Vec2::new(img_w, img_h))
                                                .corner_radius(CornerRadius::same(8))
                                                .sense(egui::Sense::click()),
                                        );
                                        if img_resp
                                            .on_hover_text("Click to preview & rename")
                                            .clicked()
                                        {
                                            open_preview = Some(s.path.clone());
                                        }
                                    } else {
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(img_w, img_h),
                                            egui::Sense::click(),
                                        );
                                        ui.painter().rect_filled(
                                            rect,
                                            CornerRadius::same(8),
                                            p.elevated,
                                        );
                                        ui.painter().text(
                                            rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            "Loading...",
                                            egui::FontId::proportional(11.5),
                                            p.text_muted,
                                        );
                                        if resp.clicked() {
                                            open_preview = Some(s.path.clone());
                                        }
                                    }
                                    ui.add_space(6.0);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&s.file_name)
                                                .monospace()
                                                .strong()
                                                .size(11.5),
                                        )
                                        .truncate(),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format_bytes(s.file_size_bytes))
                                                .monospace()
                                                .size(10.5)
                                                .color(p.text_muted),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .small_button(
                                                        RichText::new("Del").color(p.danger),
                                                    )
                                                    .clicked()
                                                {
                                                    delete_path = Some(s.path.clone());
                                                }
                                                if ui.small_button("Open").clicked() {
                                                    open_external(&s.path.display().to_string());
                                                }
                                                if ui.small_button("Preview").clicked() {
                                                    open_preview = Some(s.path.clone());
                                                }
                                            },
                                        );
                                    });
                                });
                        }
                    });
                    ui.add_space(8.0);
                }
            });

        if let Some(path) = open_preview {
            self.screenshot_preview_rename_input = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            self.screenshot_preview_path = Some(path);
            self.screenshot_preview_texture = None;
        }
        if let Some(path) = delete_path {
            match ScreenshotManager::delete_screenshot(&path) {
                Ok(()) => {
                    self.screenshots_cache.remove(&inst.instance_id);
                    self.load_screenshots_async(&inst.instance_id, ctx, true);
                    self.notice_banner = Some(("Deleted screenshot.".to_string(), false));
                }
                Err(e) => {
                    self.notice_banner = Some((format!("Delete screenshot failed: {e}"), true));
                }
            }
        }
    }

    pub(crate) fn render_logs_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
    ) {
        let p = self.palette;
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let latest_crash = find_latest_crash_report(&mc_dir);
        let log_file = self
            .paths
            .logs_root
            .join(format!("{}-latest.log", inst.instance_id));

        ui.horizontal_wrapped(|ui| {
            let lvl_lbl = match self.logs_level_filter.as_str() {
                "info" => "Level: INFO",
                "warn" => "Level: WARN",
                "error" => "Level: ERROR",
                _ => "Level: All",
            };
            egui::ComboBox::from_id_salt("cb_log_level_filter")
                .selected_text(lvl_lbl)
                .width(118.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.logs_level_filter,
                        "all".to_string(),
                        "Level: All",
                    );
                    ui.selectable_value(&mut self.logs_level_filter, "info".to_string(), "INFO");
                    ui.selectable_value(&mut self.logs_level_filter, "warn".to_string(), "WARN");
                    ui.selectable_value(&mut self.logs_level_filter, "error".to_string(), "ERROR");
                });
            ui.add(
                TextEdit::singleline(&mut self.logs_tab_search)
                    .hint_text("Filter logs...")
                    .desired_width(170.0),
            );
            ui.checkbox(&mut self.logs_errors_only, "Stderr / Errors only");
            if ui.button("Copy Logs").clicked() {
                if let Some(lines) = self.instance_logs.get(&inst.instance_id) {
                    let text: String = lines
                        .iter()
                        .map(|l| l.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.ctx().copy_text(text);
                }
            }
            if ui.button("Clear").clicked() {
                self.instance_logs
                    .insert(inst.instance_id.clone(), Vec::new());
            }
            if log_file.is_file() && ui.button("Open Log File").clicked() {
                open_external(&log_file.display().to_string());
            }
            if let Some(ref crash_path) = latest_crash {
                let crash_btn = Button::new(
                    RichText::new("Open Latest Crash Report")
                        .color(p.warning)
                        .strong(),
                );
                if ui.add(crash_btn).clicked() {
                    open_external(&crash_path.display().to_string());
                }
            }
            if ui.button("Export Diagnostic Bundle (.zip)").clicked() {
                let epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let default_name = format!("mcw-diagnostics-{}-{epoch}.zip", inst.instance_id);
                if let Some(out_zip) = save_file_dialog(
                    "Export Diagnostic Bundle (.zip)",
                    "Zip Archive (*.zip)|*.zip|All Files (*.*)|*.*",
                    &default_name,
                ) {
                    let live_lines: Vec<String> = self
                        .instance_logs
                        .get(&inst.instance_id)
                        .map(|v| v.iter().map(|l| l.text.clone()).collect())
                        .unwrap_or_default();
                    match DiagnosticsExporter::export_bundle(
                        &mc_dir,
                        &inst.name,
                        &inst.version_id,
                        &inst.mod_loader.name,
                        self.pc_ram_mb,
                        &self.java_installations,
                        &live_lines,
                        &out_zip,
                    ) {
                        Ok(path) => {
                            self.notice_banner = Some((
                                format!(
                                    "Exported sanitized diagnostic bundle to {}",
                                    path.display()
                                ),
                                false,
                            ));
                        }
                        Err(e) => {
                            self.notice_banner =
                                Some((format!("Failed to export diagnostic bundle: {e}"), true));
                        }
                    }
                }
            }
            let upload_label = if self.uploading_mclogs {
                "Uploading..."
            } else {
                "Upload to mclo.gs"
            };
            let upload_btn = Button::new(RichText::new(upload_label).color(p.accent_text).strong())
                .fill(p.accent);
            if ui
                .add_enabled(!self.uploading_mclogs, upload_btn)
                .on_hover_text(
                    "Automatically redacts tokens, emails, and user paths before uploading",
                )
                .clicked()
            {
                let live_text = self
                    .instance_logs
                    .get(&inst.instance_id)
                    .map(|lines| {
                        lines
                            .iter()
                            .map(|l| l.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                let content = if !live_text.trim().is_empty() {
                    live_text
                } else {
                    std::fs::read_to_string(&log_file).unwrap_or_default()
                };
                if content.trim().is_empty() {
                    self.notice_banner = Some((
                        "No live logs or latest.log file found to upload.".to_string(),
                        true,
                    ));
                } else {
                    self.start_mclogs_upload(content, ctx);
                }
            }
            if let Some(ref url) = self.last_mclogs_url {
                if ui.button("Open mclo.gs").clicked() {
                    open_external(url);
                }
            }
        });

        ui.add_space(6.0);

        if let Some(summary) = self.crash_summaries.get(&inst.instance_id).cloned() {
            let mut dismiss_crash = false;
            Frame::new()
                .fill(p.elevated)
                .stroke(Stroke::new(1.0_f32, p.danger))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Crash Summary: {}", summary.title))
                                .strong()
                                .color(p.danger),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Dismiss").clicked() {
                                dismiss_crash = true;
                            }
                        });
                    });
                    ui.label(
                        RichText::new(&summary.likely_cause)
                            .size(12.0)
                            .color(p.text),
                    );
                    ui.label(
                        RichText::new(format!("Suggestion: {}", summary.suggestion))
                            .size(11.5)
                            .color(p.accent),
                    );
                    for hint in &summary.culprit_hints {
                        ui.label(
                            RichText::new(format!("• {hint}"))
                                .monospace()
                                .size(11.0)
                                .color(p.text_muted),
                        );
                    }
                });
            if dismiss_crash {
                self.crash_summaries.remove(&inst.instance_id);
            }
            ui.add_space(6.0);
        }

        let needle = self.logs_tab_search.trim().to_ascii_lowercase();
        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let lines = self.instance_logs.get(&inst.instance_id);
                if lines.map(|l| l.is_empty()).unwrap_or(true) {
                    ui.label(
                        RichText::new("No live logs captured for this session yet.")
                            .monospace()
                            .color(p.text_muted),
                    );
                    return;
                }
                ScrollArea::vertical()
                    .id_salt("live_logs_scroll")
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        let mut shown = 0usize;
                        for line in lines.unwrap() {
                            let is_stderr = line.stream == LogStream::Stderr;
                            let up = line.text.to_ascii_uppercase();
                            let is_err = is_stderr
                                || up.contains("ERROR")
                                || up.contains("FATAL")
                                || up.contains("EXCEPTION");
                            if self.logs_errors_only && !is_err {
                                continue;
                            }
                            if !matches_log_level(&line.text, is_stderr, &self.logs_level_filter) {
                                continue;
                            }
                            if !needle.is_empty()
                                && !line.text.to_ascii_lowercase().contains(&needle)
                            {
                                continue;
                            }
                            shown += 1;
                            let color = if is_err {
                                p.danger
                            } else if up.contains("[WARN]") || up.contains("/WARN]") {
                                p.warning
                            } else {
                                p.text
                            };
                            ui.label(
                                RichText::new(&line.text)
                                    .monospace()
                                    .size(11.5)
                                    .color(color),
                            );
                        }
                        if shown == 0 {
                            ui.label(
                                RichText::new("No log lines match the current filter.")
                                    .monospace()
                                    .color(p.text_muted),
                            );
                        }
                    });
            });
    }

    pub(crate) fn render_settings_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
    ) {
        let p = self.palette;
        let preset_lbl = self.t("jvm_preset.label").to_string();
        let preset_options: [(JvmPresetId, String); 5] = [
            (
                JvmPresetId::Default,
                self.t(JvmPresetId::Default.i18n_key()).to_string(),
            ),
            (
                JvmPresetId::Aikar,
                self.t(JvmPresetId::Aikar.i18n_key()).to_string(),
            ),
            (
                JvmPresetId::Zgc,
                self.t(JvmPresetId::Zgc.i18n_key()).to_string(),
            ),
            (
                JvmPresetId::Shenandoah,
                self.t(JvmPresetId::Shenandoah.i18n_key()).to_string(),
            ),
            (
                JvmPresetId::Custom,
                self.t(JvmPresetId::Custom.i18n_key()).to_string(),
            ),
        ];

        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let tracked_pack = detect_installed_pack(&inst_dir);
        let optifine_state = OptiFineManager::state(&inst_dir);
        let browse_lbl = self.t("instance_defaults.java.browse").to_string();
        let optifine_title_lbl = self.t("optifine.title").to_string();
        let optifine_choose_lbl = self.t("optifine.file.choose_title").to_string();
        let save_inst_lbl = self.t("instance_settings.save").to_string();
        let is_forge_family = matches!(
            inst.mod_loader.name.to_ascii_lowercase().as_str(),
            "forge" | "neoforge"
        );
        let last_status_text = match inst.last_exit_code {
            Some(code) => format!("{} (exit code: {code})", inst.last_launch_state),
            None => inst.last_launch_state.clone(),
        };

        let mut save_clicked = false;
        let mut dirty_save_now = false;
        let mut dirty_discard_now = false;
        let mut apply_runtime = false;
        let mut install_java_major: Option<u32> = None;
        let mut optifine_install_mods = false;
        let mut optifine_launch_installer = false;
        let mut optifine_uninstall = false;

        if self.instance_settings_dirty {
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.warning))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("Unsaved changes in Instance Settings.")
                                .strong()
                                .color(p.warning),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Discard").clicked() {
                                dirty_discard_now = true;
                            }
                            let sb =
                                Button::new(RichText::new("Save").color(p.accent_text).strong())
                                    .fill(p.accent);
                            if ui.add(sb).clicked() {
                                dirty_save_now = true;
                            }
                        });
                    });
                });
            ui.add_space(6.0);
        }

        let settings_before = self.selected_settings.clone();
        let jvm_before = self.edit_jvm_args_text.clone();
        let game_before = self.edit_game_args_text.clone();
        let env_before = self.edit_env_vars_text.clone();

        let Some(ref mut settings) = self.selected_settings else {
            return;
        };

        ScrollArea::vertical()
            .id_salt("instance_settings_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("Instance Info").strong());
                        ui.add_space(4.0);
                        egui::Grid::new("inst_settings_info_grid")
                            .num_columns(2)
                            .spacing([18.0, 4.0])
                            .show(ui, |ui| {
                                ui.label(RichText::new("Instance ID:").color(p.text_muted));
                                ui.label(RichText::new(&inst.instance_id).monospace());
                                ui.end_row();
                                ui.label(RichText::new("Game Directory:").color(p.text_muted));
                                ui.label(RichText::new(mc_dir.display().to_string()).monospace());
                                ui.end_row();
                                if let Some(ref pack) = tracked_pack {
                                    ui.label(RichText::new("Pack Origin:").color(p.text_muted));
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · {} ({})",
                                            pack.provider, pack.pack_name, pack.version_name
                                        ))
                                        .monospace(),
                                    );
                                    ui.end_row();
                                }
                                if !inst.icon_origin.trim().is_empty()
                                    && !inst.icon_origin.eq_ignore_ascii_case("default")
                                {
                                    ui.label(RichText::new("Icon Origin:").color(p.text_muted));
                                    ui.label(RichText::new(&inst.icon_origin).monospace());
                                    ui.end_row();
                                }
                                ui.label(RichText::new("Last Status:").color(p.text_muted));
                                ui.label(RichText::new(&last_status_text).monospace());
                                ui.end_row();
                            });
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Instance Notes (notes.txt)")
                                .size(12.0)
                                .strong(),
                        );
                        ui.add_space(2.0);
                        ui.add(
                            TextEdit::multiline(&mut self.edit_instance_notes_text)
                                .hint_text("Write notes, modpack reminders, server IP, or TODOs for this instance...")
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                    });
                ui.add_space(10.0);

                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("Target Minecraft & Mod Loader").strong());
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Minecraft Version:");
                            egui::ComboBox::from_id_salt("inst_settings_mc_ver_combo")
                                .selected_text(&self.edit_runtime_version)
                                .width(150.0)
                                .show_ui(ui, |ui| {
                                    for (vid, vtype) in &self.available_versions {
                                        if !self.prefs.show_snapshots && vtype != "release" {
                                            continue;
                                        }
                                        ui.selectable_value(
                                            &mut self.edit_runtime_version,
                                            vid.clone(),
                                            vid,
                                        );
                                    }
                                });
                            ui.add_space(6.0);
                            ui.label("Mod Loader:");
                            let loader_disp = display_loader_name(&self.edit_runtime_loader);
                            egui::ComboBox::from_id_salt("inst_settings_loader_combo")
                                .selected_text(&loader_disp)
                                .width(120.0)
                                .show_ui(ui, |ui| {
                                    for l in ["vanilla", "fabric", "quilt", "forge", "neoforge"] {
                                        ui.selectable_value(
                                            &mut self.edit_runtime_loader,
                                            l.to_string(),
                                            display_loader_name(l),
                                        );
                                    }
                                });
                            if ui.button("Apply Version / Loader").clicked() {
                                apply_runtime = true;
                            }
                        });
                        ui.add_space(6.0);
                        let is_pack_default = is_verified_or_imported_modpack(
                            &self.paths.instance_dir(&inst.instance_id),
                            &inst.icon_origin,
                        );
                        let mut disable_prot =
                            settings.is_mod_repair_and_update_disabled(is_pack_default);
                        if ui
                            .checkbox(
                                &mut disable_prot,
                                "Disable Mod Repair & Mod Updates (Recommended for modpacks)",
                            )
                            .on_hover_text(
                                "Hides individual mod update checks and compatibility repair prompts in the Mods tab to avoid breaking curated modpacks.",
                            )
                            .changed()
                        {
                            settings.disable_mod_repair_and_updates = Some(disable_prot);
                        }

                        let is_official_for_check =
                            detect_installed_pack(&self.paths.instance_dir(&inst.instance_id))
                                .is_some();
                        let mut disable_launch_chk =
                            settings.is_launch_checking_disabled(is_official_for_check);
                        if ui
                            .checkbox(
                                &mut disable_launch_chk,
                                "Disable pre-launch dependency checking (Recommended for modpacks)",
                            )
                            .on_hover_text(
                                "Skips pre-launch dependency warning modals across all modloaders while still writing findings to the Launcher Logs.",
                            )
                            .changed()
                        {
                            settings.disable_launch_checking = Some(disable_launch_chk);
                        }

                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Account Override:");
                            let pref_lbl = match settings.preferred_account_uuid.as_deref() {
                                None => "Use Global Account".to_string(),
                                Some(uuid) => self
                                    .accounts
                                    .iter()
                                    .find(|a| a.uuid == uuid)
                                    .map(|a| {
                                        let kind = if a.account_type == AccountSource::Microsoft {
                                            "MSA"
                                        } else {
                                            "Offline"
                                        };
                                        format!("{} ({kind})", a.username)
                                    })
                                    .unwrap_or_else(|| "Missing account (Global fallback)".to_string()),
                            };
                            let aw = auto_combo_width(ui, &pref_lbl, 180.0, 280.0);
                            egui::ComboBox::from_id_salt("inst_account_override_combo")
                                .selected_text(&pref_lbl)
                                .width(aw)
                                .truncate()
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut settings.preferred_account_uuid,
                                        None,
                                        "Use Global Account",
                                    );
                                    for acc in &self.accounts {
                                        let kind = if acc.account_type == AccountSource::Microsoft {
                                            "MSA"
                                        } else {
                                            "Offline"
                                        };
                                        ui.selectable_value(
                                            &mut settings.preferred_account_uuid,
                                            Some(acc.uuid.clone()),
                                            format!("{} ({kind})", acc.username),
                                        );
                                    }
                                });
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.label(RichText::new("Memory & Window").strong());
                        ui.add_space(6.0);

                        ui.horizontal(|ui| {
                            ui.label("Min RAM (MB):");
                            ui.add(egui::DragValue::new(&mut settings.min_memory).range(256..=32768));
                            ui.label("Max RAM (MB):");
                            ui.add(egui::DragValue::new(&mut settings.max_memory).range(512..=65536));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Width:");
                            ui.add(egui::DragValue::new(&mut settings.width).range(320..=7680));
                            ui.label("Height:");
                            ui.add(egui::DragValue::new(&mut settings.height).range(240..=4320));
                            ui.checkbox(&mut settings.fullscreen, "Fullscreen");
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.label(RichText::new("Java Runtime").strong());
                        ui.add_space(6.0);

                        let current_java_label = settings
                            .java_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "Automatic (recommended)".to_string());

                        egui::ComboBox::from_label("Java Executable")
                            .selected_text(current_java_label)
                            .width(420.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut settings.java_path,
                                    None,
                                    "Automatic (recommended)",
                                );
                                for j in &self.java_installations {
                                    let label = format!(
                                        "Java {} ({}) - {}",
                                        j.major,
                                        j.version_string,
                                        j.path.display()
                                    );
                                    ui.selectable_value(
                                        &mut settings.java_path,
                                        Some(j.path.clone()),
                                        label,
                                    );
                                }
                            });

                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            for major in [25u32, 21, 17, 8] {
                                let installing = self.installing_java_majors.contains(&major);
                                let lbl = if installing {
                                    format!("Installing Java {major}...")
                                } else {
                                    format!("Install Temurin Java {major}")
                                };
                                if ui.add_enabled(!installing, Button::new(lbl)).clicked() {
                                    install_java_major = Some(major);
                                }
                            }
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        // JVM Preset Dropdown + Custom JVM & Game Arguments
                        let current_jvm_lines: Vec<String> = self
                            .edit_jvm_args_text
                            .lines()
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        let detected_preset = detect_preset(&current_jvm_lines);
                        let current_preset_text = preset_options
                            .iter()
                            .find(|(id, _)| *id == detected_preset)
                            .map(|(_, t)| t.clone())
                            .unwrap_or_else(|| "Custom".to_string());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(&preset_lbl).strong());
                            egui::ComboBox::from_id_salt("inst_jvm_preset_combo")
                                .selected_text(current_preset_text)
                                .width(220.0)
                                .show_ui(ui, |ui| {
                                    for (preset, p_text) in &preset_options {
                                        let active = detected_preset == *preset;
                                        if ui.selectable_label(active, p_text).clicked()
                                            && *preset != JvmPresetId::Custom
                                        {
                                            self.edit_jvm_args_text =
                                                get_preset_flags(*preset).join("\n");
                                        }
                                    }
                                });
                        });
                        ui.add_space(4.0);
                        ui.label(RichText::new("JVM Arguments (one flag per line):").size(11.5).color(p.text_muted));
                        ui.add(
                            TextEdit::multiline(&mut self.edit_jvm_args_text)
                                .desired_rows(3)
                                .desired_width(520.0)
                                .font(egui::TextStyle::Monospace),
                        );
                        ui.add_space(4.0);
                        ui.label(RichText::new("Game Arguments (one flag per line):").size(11.5).color(p.text_muted));
                        ui.add(
                            TextEdit::multiline(&mut self.edit_game_args_text)
                                .desired_rows(2)
                                .desired_width(520.0)
                                .font(egui::TextStyle::Monospace),
                        );

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.label(RichText::new("Commands & Environment").strong());
                        ui.label(
                            RichText::new(
                                "Supports $INST_NAME, $INST_ID, $INST_DIR, $INST_MC_DIR, and $INST_JAVA in hook/env values.",
                            )
                            .size(11.0)
                            .color(p.text_muted),
                        );
                        ui.add_space(6.0);
                        egui::Grid::new("inst_commands_env_grid")
                            .num_columns(2)
                            .spacing([10.0, 6.0])
                            .show(ui, |ui| {
                                ui.label("Pre-launch command:");
                                ui.add(
                                    TextEdit::singleline(&mut settings.pre_launch_command)
                                        .desired_width(400.0)
                                        .font(egui::TextStyle::Monospace),
                                );
                                ui.end_row();
                                ui.label("Wrapper command:");
                                ui.add(
                                    TextEdit::singleline(&mut settings.wrapper_command)
                                        .hint_text("e.g. prime-run or env VAR=1")
                                        .desired_width(400.0)
                                        .font(egui::TextStyle::Monospace),
                                );
                                ui.end_row();
                                ui.label("Post-exit command:");
                                ui.add(
                                    TextEdit::singleline(&mut settings.post_exit_command)
                                        .desired_width(400.0)
                                        .font(egui::TextStyle::Monospace),
                                );
                                ui.end_row();
                            });
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            let gm_lbl = if cfg!(target_os = "linux") {
                                "Enable GameMode (gamemoderun)"
                            } else {
                                "Enable GameMode (gamemoderun, Linux)"
                            };
                            let mh_lbl = if cfg!(target_os = "linux") {
                                "Enable MangoHud (mangohud)"
                            } else {
                                "Enable MangoHud (mangohud, Linux)"
                            };
                            ui.checkbox(&mut settings.enable_gamemode, gm_lbl);
                            ui.checkbox(&mut settings.enable_mangohud, mh_lbl);
                        });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Environment variables (KEY=VALUE, one per line):")
                                .size(11.5)
                                .color(p.text_muted),
                        );
                        ui.add(
                            TextEdit::multiline(&mut self.edit_env_vars_text)
                                .hint_text("MESA_GL_VERSION_OVERRIDE=4.6\n__GL_THREADED_OPTIMIZATIONS=1")
                                .desired_rows(2)
                                .desired_width(520.0)
                                .font(egui::TextStyle::Monospace),
                        );

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.label(RichText::new(&optifine_title_lbl).strong());
                        ui.add_space(4.0);
                        if optifine_state.installed {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "Installed: OptiFine {} ({})",
                                        optifine_state.version_id, optifine_state.mode
                                    ))
                                    .monospace()
                                    .size(11.5)
                                    .color(p.accent),
                                );
                                if optifine_state.managed && ui.button("Uninstall OptiFine").clicked() {
                                    optifine_uninstall = true;
                                }
                            });
                            ui.add_space(4.0);
                        }
                        ui.label(
                            RichText::new(format!(
                                "Select or paste path to an official OptiFine_{}_*.jar file to validate and install.",
                                inst.version_id
                            ))
                            .size(11.5)
                            .color(p.text_muted),
                        );
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                TextEdit::singleline(&mut self.optifine_jar_input)
                                    .hint_text("Select or paste path to OptiFine .jar...")
                                    .desired_width(300.0),
                            );
                            if ui.button(&browse_lbl).clicked() {
                                if let Some(chosen) = pick_file_dialog(
                                    &optifine_choose_lbl,
                                    "OptiFine JAR (*.jar)|*.jar|All Files (*.*)|*.*",
                                ) {
                                    self.optifine_jar_input = chosen.display().to_string();
                                }
                            }
                            if is_forge_family {
                                if ui.button("Install Managed Mod").clicked() {
                                    optifine_install_mods = true;
                                }
                            } else if ui
                                .add_enabled(
                                    !self.optifine_installing,
                                    Button::new(if self.optifine_installing {
                                        "Installing..."
                                    } else {
                                        "Install OptiFine Profile..."
                                    }),
                                )
                                .clicked()
                            {
                                optifine_launch_installer = true;
                            }
                            if ui.button("optifine.net").clicked() {
                                open_external(mcw_minecraft::OFFICIAL_OPTIFINE_URL);
                            }
                        });

                        ui.add_space(12.0);
                        let save_btn =
                            Button::new(RichText::new(&save_inst_lbl).color(p.accent_text).strong())
                                .fill(p.accent)
                                .corner_radius(CornerRadius::same(10));
                        if ui.add(save_btn).clicked() {
                            save_clicked = true;
                        }
                    });
            });

        if optifine_uninstall {
            match OptiFineManager::uninstall(&self.paths, inst) {
                Ok(true) => {
                    self.mods_cache.remove(&inst.instance_id);
                    self.notice_banner = Some((
                        "Uninstalled MCW-managed OptiFine successfully.".to_string(),
                        false,
                    ));
                }
                Ok(false) => {}
                Err(e) => {
                    self.notice_banner = Some((format!("OptiFine uninstall failed: {e}"), true));
                }
            }
        }
        if optifine_install_mods {
            let jar_path = PathBuf::from(self.optifine_jar_input.trim().trim_matches('"'));
            match OptiFineManager::install_managed_forge_mod(&self.paths, inst, &jar_path) {
                Ok(state) => {
                    self.mods_cache.remove(&inst.instance_id);
                    self.optifine_jar_input.clear();
                    self.notice_banner = Some((
                        format!("Installed managed OptiFine {} to mods/.", state.file_name),
                        false,
                    ));
                }
                Err(e) => {
                    self.notice_banner = Some((format!("OptiFine install failed: {e}"), true));
                }
            }
        }
        if optifine_launch_installer {
            let jar_path = PathBuf::from(self.optifine_jar_input.trim().trim_matches('"'));
            if inst.mod_loader.is_vanilla() {
                self.start_optifine_standalone_install(inst.clone(), jar_path, ctx);
            } else {
                let java_exe = self
                    .selected_settings
                    .as_ref()
                    .and_then(|s| s.java_path.clone())
                    .or_else(|| self.java_installations.first().map(|j| j.path.clone()));
                match java_exe {
                    Some(java) => match OptiFineManager::launch_official_installer(
                        &jar_path,
                        &java,
                        Some(&inst.version_id),
                    ) {
                        Ok(()) => {
                            self.notice_banner = Some((
                                "Launched official OptiFine installer window.".to_string(),
                                false,
                            ));
                        }
                        Err(e) => {
                            self.notice_banner =
                                Some((format!("OptiFine installer failed: {e}"), true));
                        }
                    },
                    None => {
                        self.notice_banner = Some((
                            "No Java runtime found. Install Java 17 or 21 first.".to_string(),
                            true,
                        ));
                    }
                }
            }
        }

        if apply_runtime {
            let new_loader = if self.edit_runtime_loader == "vanilla" {
                ModLoader::vanilla()
            } else {
                ModLoader {
                    name: self.edit_runtime_loader.clone(),
                    version: String::new(),
                }
            };
            let store = InstanceStore::new(&self.paths);
            match store.set_runtime_profile(
                &inst.instance_id,
                &self.edit_runtime_version,
                new_loader,
            ) {
                Ok(_) => {
                    self.reload_instances();
                    self.notice_banner = Some((
                        format!(
                            "Updated '{}' to Minecraft {} ({}).",
                            inst.name,
                            self.edit_runtime_version,
                            display_loader_name(&self.edit_runtime_loader)
                        ),
                        false,
                    ));
                }
                Err(e) => {
                    self.notice_banner =
                        Some((format!("Failed to update version/loader: {e}"), true));
                }
            }
        }

        if self.selected_settings.as_ref() != settings_before.as_ref()
            || self.edit_jvm_args_text != jvm_before
            || self.edit_game_args_text != game_before
            || self.edit_env_vars_text != env_before
            || self.edit_instance_notes_text != self.instance_notes_baseline
        {
            self.instance_settings_dirty = true;
        }

        if dirty_discard_now {
            self.discard_selected_instance_settings();
        } else if save_clicked || dirty_save_now {
            self.save_selected_instance_settings();
        }
        if let Some(major) = install_java_major {
            self.start_java_install(major, ctx);
        }
    }
}
