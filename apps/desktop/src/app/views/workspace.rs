use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_detail_pane(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(p.canvas)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                // Notice / Error banner
                if let Some((msg, is_err)) = self.notice_banner.clone() {
                    let banner_stroke = if is_err { p.danger } else { p.accent };
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, banner_stroke))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(msg).color(if is_err {
                                    p.danger
                                } else {
                                    p.text
                                }));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button("Dismiss").clicked() {
                                        self.notice_banner = None;
                                    }
                                });
                            });
                        });
                    ui.add_space(8.0);
                }

                let Some(inst) = self.selected_instance().cloned() else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.label(
                            RichText::new("Select or create a Minecraft instance")
                                .size(16.0)
                                .strong(),
                        );
                        ui.add_space(8.0);
                        if ui.button("+ Create Instance").clicked() {
                            let latest = self.latest_mc_version(self.create_show_snapshots);
                            if !latest.is_empty() {
                                self.create_version = latest;
                            }
                            self.show_create_modal = true;
                        }
                    });
                    return;
                };

                // Ensure selected instance icon is loaded
                self.ensure_instance_icon(&inst, ctx);

                // Detail Header + Primary Launch/Stop button
                let busy = self.is_instance_busy(&inst.instance_id);
                let has_account = self.active_account().is_some();
                let mut change_icon_clicked = false;
                let mut reset_icon_clicked = false;

                ui.horizontal(|ui| {
                    self.render_instance_icon(ui, &inst, 44.0);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&inst.name).size(20.0).strong().color(p.text));
                            let chg_lbl = self.t("workspace.action.change_icon").to_string();
                            if ui
                                .add(Button::new(RichText::new(chg_lbl).size(10.5)))
                                .clicked()
                            {
                                change_icon_clicked = true;
                            }
                            if !inst.icon.is_empty()
                                && !inst.icon.eq_ignore_ascii_case("grass_block")
                            {
                                let rst_lbl = self.t("workspace.action.reset_icon").to_string();
                                if ui
                                    .add(Button::new(RichText::new(rst_lbl).size(10.5)))
                                    .clicked()
                                {
                                    reset_icon_clicked = true;
                                }
                            }
                        });
                        ui.label(
                            RichText::new(format!(
                                "Minecraft {}  |  Loader: {}  |  Playtime: {}",
                                inst.version_id,
                                inst.mod_loader.name,
                                format_duration(inst.total_playtime_seconds)
                            ))
                            .monospace()
                            .size(11.5)
                            .color(p.text_muted),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        match self.launch_states.get(&inst.instance_id) {
                            Some(LaunchState::Running { control, .. }) => {
                                let stop_btn = Button::new(
                                    RichText::new(self.t("Stop Game"))
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(p.danger)
                                .min_size(Vec2::new(120.0, 34.0))
                                .corner_radius(CornerRadius::same(3));
                                if ui.add(stop_btn).clicked() {
                                    control.request_stop();
                                }
                            }
                            Some(LaunchState::Preparing(_)) => {
                                let launch_ctrl = self
                                    .launch_task_ids
                                    .get(&inst.instance_id)
                                    .and_then(|tid| {
                                        self.active_tasks
                                            .iter()
                                            .find(|t| t.id == *tid)
                                            .map(|t| Arc::clone(&t.control))
                                    });
                                let is_paused = launch_ctrl
                                    .as_ref()
                                    .map(|c| c.is_paused())
                                    .unwrap_or(false);
                                let is_cancelling = launch_ctrl
                                    .as_ref()
                                    .map(|c| c.is_cancelled())
                                    .unwrap_or(false);

                                let cancel_text = if is_cancelling {
                                    self.t("launch.cancelling_badge").to_string()
                                } else {
                                    self.t("launch.cancel_button").to_string()
                                };
                                let cancel_btn = Button::new(
                                    RichText::new(cancel_text)
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(p.danger)
                                .min_size(Vec2::new(110.0, 34.0))
                                .corner_radius(CornerRadius::same(3));
                                if ui.add_enabled(!is_cancelling, cancel_btn).clicked() {
                                    if let Some(ref ctrl) = launch_ctrl {
                                        ctrl.cancel();
                                    }
                                }

                                let pause_text = if is_paused {
                                    self.t("launch.resume_button").to_string()
                                } else {
                                    self.t("launch.pause_button").to_string()
                                };
                                let pause_btn = Button::new(
                                    RichText::new(pause_text)
                                        .color(if is_paused { p.accent_text } else { p.text })
                                        .strong(),
                                )
                                .fill(if is_paused { p.accent } else { p.elevated })
                                .min_size(Vec2::new(96.0, 34.0))
                                .corner_radius(CornerRadius::same(3));
                                if ui.add_enabled(!is_cancelling, pause_btn).clicked() {
                                    if let Some(ref ctrl) = launch_ctrl {
                                        ctrl.toggle_pause();
                                    }
                                }
                            }
                            _ => {
                                let missing_manual = self
                                    .pending_manual_counts
                                    .get(&inst.instance_id)
                                    .copied()
                                    .unwrap_or(0);
                                if missing_manual > 0 {
                                    let warn_btn = Button::new(
                                        RichText::new(format!("Missing Mods ({missing_manual})"))
                                            .color(Color32::BLACK)
                                            .strong(),
                                    )
                                    .fill(p.warning)
                                    .min_size(Vec2::new(140.0, 34.0))
                                    .corner_radius(CornerRadius::same(3));
                                    if ui
                                        .add(warn_btn)
                                        .on_hover_text(
                                            "This modpack still has missing manual downloads. Click to complete them before launching.",
                                        )
                                        .clicked()
                                    {
                                        self.open_manual_downloads_for(&inst.instance_id);
                                    }
                                } else {
                                    let launch_btn = Button::new(
                                        RichText::new(self.t("Launch"))
                                            .color(p.accent_text)
                                            .strong(),
                                    )
                                    .fill(p.accent)
                                    .min_size(Vec2::new(120.0, 34.0))
                                    .corner_radius(CornerRadius::same(3));
                                    let resp = ui.add_enabled(has_account, launch_btn);
                                    if !has_account {
                                        resp.on_disabled_hover_text(
                                            "Add or sign in to an account first",
                                        );
                                    } else if resp.clicked() {
                                        self.start_launch(ctx);
                                    }
                                }
                            }
                        }
                    });
                });

                ui.add_space(8.0);

                // Persistent flag banner whenever a modpack instance still has missing manual downloads
                let pending_manual_count = self
                    .pending_manual_counts
                    .get(&inst.instance_id)
                    .copied()
                    .unwrap_or(0);
                if pending_manual_count > 0 {
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.warning))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("INCOMPLETE MODPACK")
                                        .strong()
                                        .monospace()
                                        .size(11.0)
                                        .color(p.warning),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{pending_manual_count} manual mod(s) have not been downloaded yet."
                                    ))
                                    .size(12.0)
                                    .color(p.text),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let open_btn = Button::new(
                                        RichText::new("Open Manual Downloads")
                                            .color(Color32::BLACK)
                                            .strong(),
                                    )
                                    .fill(p.warning);
                                    if ui.add(open_btn).clicked() {
                                        self.open_manual_downloads_for(&inst.instance_id);
                                    }
                                    if ui.button("Scan Downloads Now").clicked() {
                                        self.run_manual_downloads_scan(&inst.instance_id, true);
                                    }
                                });
                            });
                        });
                    ui.add_space(8.0);
                }

                if change_icon_clicked {
                    if let Some(chosen) = pick_file_dialog(
                        self.t("workspace.icon.choose_title"),
                        "Images (*.png;*.jpg;*.jpeg;*.webp;*.bmp;*.ico)|*.png;*.jpg;*.jpeg;*.webp;*.bmp;*.ico",
                    ) {
                        let store = InstanceStore::new(&self.paths);
                        match store.set_instance_icon(&inst.instance_id, &chosen) {
                            Ok(_) => {
                                self.instance_icon_textures.clear();
                                self.reload_instances();
                            }
                            Err(err) => {
                                self.notice_banner = Some((format!("Failed to set icon: {err}"), true));
                            }
                        }
                    }
                }
                if reset_icon_clicked {
                    let store = InstanceStore::new(&self.paths);
                    if store.reset_instance_icon(&inst.instance_id).is_ok() {
                        self.instance_icon_textures.clear();
                        self.reload_instances();
                    }
                }

                // Launch progress is shown exclusively in the unified bottom progress bar (no duplicate big bar here).
                if let Some(LaunchState::Running { pid, .. }) =
                    self.launch_states.get(&inst.instance_id)
                {
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.accent))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                        RichText::new("RUNNING")
                                            .strong()
                                            .monospace()
                                            .color(p.accent),
                                    );
                                    ui.label(
                                        RichText::new(format!("PID {pid} — streaming live logs"))
                                            .color(p.text_muted),
                                    );
                                });
                            });
                        ui.add_space(8.0);
                    }

                // Detail Tabs
                let mut switched_tab: Option<DetailTab> = None;
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (DetailTab::Overview, self.t("Overview")),
                        (DetailTab::Mods, self.t("Mods")),
                        (DetailTab::Packs, self.t("Packs")),
                        (DetailTab::Worlds, self.t("Worlds")),
                        (DetailTab::Screenshots, self.t("Screenshots")),
                        (DetailTab::Backups, self.t("Backups")),
                        (DetailTab::Logs, self.t("navigation.logs")),
                        (DetailTab::Settings, self.t("navigation.instance_settings")),
                    ] {
                        let active = self.active_tab == tab;
                        let btn = Button::new(
                            RichText::new(label).color(if active {
                                p.accent
                            } else {
                                p.text_muted
                            }),
                        )
                        .fill(if active { p.elevated } else { p.surface })
                        .stroke(Stroke::new(
                            1.0_f32,
                            if active { p.accent } else { p.border },
                        ))
                        .corner_radius(CornerRadius::same(3));
                        if ui.add(btn).clicked() {
                            switched_tab = Some(tab);
                        }
                    }
                });
                if let Some(tab) = switched_tab {
                    self.active_tab = tab;
                    self.ensure_active_tab_loaded(ctx);
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                match self.active_tab {
                    DetailTab::Overview => self.render_overview_tab(ui, ctx, &inst, busy),
                    DetailTab::Mods => self.render_mods_tab(ui, ctx, &inst),
                    DetailTab::Packs => self.render_packs_tab(ui, ctx, &inst),
                    DetailTab::Worlds => self.render_worlds_tab(ui, ctx, &inst, busy),
                    DetailTab::Screenshots => self.render_screenshots_tab(ui, ctx, &inst),
                    DetailTab::Backups => self.render_backups_tab(ui, ctx, &inst, busy),
                    DetailTab::Logs => self.render_logs_tab(ui, ctx, &inst),
                    DetailTab::Settings => self.render_settings_tab(ui, ctx, &inst),
                }
            });
    }

    pub(crate) fn render_overview_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
        busy: bool,
    ) {
        let p = self.palette;
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let export_label = self.t("modpack_package.export.title").to_string();
        let clone_lbl = self.t("workspace.action.clone").to_string();
        let chg_icon_lbl = self.t("workspace.action.change_icon").to_string();
        let rst_icon_lbl = self.t("workspace.action.reset_icon").to_string();

        let mut trigger_repair = false;
        let mut open_export = false;
        let mut change_icon_clicked = false;
        let mut reset_icon_clicked = false;

        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(3))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // Header Row: Title + Compact Action Menu (`⋮ Actions ▾`)
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Instance Details")
                            .size(14.5)
                            .strong()
                            .color(p.text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.menu_button(
                            RichText::new("⋮ Actions ▾").strong().color(p.text),
                            |ui| {
                                ui.set_min_width(210.0);
                                if ui.button("Open .minecraft Folder").clicked() {
                                    let _ = std::fs::create_dir_all(&mc_dir);
                                    open_external(&mc_dir.display().to_string());
                                    ui.close_menu();
                                }
                                if ui
                                    .add_enabled(!busy, Button::new(&export_label))
                                    .on_disabled_hover_text("Stop the instance before exporting")
                                    .clicked()
                                {
                                    open_export = true;
                                    ui.close_menu();
                                }
                                if ui
                                    .add_enabled(!busy, Button::new("Repair Files"))
                                    .on_disabled_hover_text("Stop the instance before repairing")
                                    .clicked()
                                {
                                    trigger_repair = true;
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui
                                    .add_enabled(!busy, Button::new("Rename Instance..."))
                                    .on_disabled_hover_text("Stop the instance before renaming")
                                    .clicked()
                                {
                                    self.rename_input = inst.name.clone();
                                    self.show_rename_modal = true;
                                    ui.close_menu();
                                }
                                if ui.button(&clone_lbl).clicked() {
                                    self.clone_name_input = format!("{} (Copy)", inst.name);
                                    self.clone_include_saves = false;
                                    self.show_clone_modal = true;
                                    ui.close_menu();
                                }
                                if ui.button(&chg_icon_lbl).clicked() {
                                    change_icon_clicked = true;
                                    ui.close_menu();
                                }
                                if !inst.icon.is_empty()
                                    && !inst.icon.eq_ignore_ascii_case("grass_block")
                                    && ui.button(&rst_icon_lbl).clicked()
                                {
                                    reset_icon_clicked = true;
                                    ui.close_menu();
                                }
                                ui.separator();
                                let del_btn = Button::new(
                                    RichText::new("Delete Instance...").color(p.danger),
                                );
                                if ui
                                    .add_enabled(!busy, del_btn)
                                    .on_disabled_hover_text("Stop the instance before deleting")
                                    .clicked()
                                {
                                    self.show_delete_modal = true;
                                    ui.close_menu();
                                }
                            },
                        );
                    });
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                // Comprehensive Instance Metadata Grid
                let loader_text = if inst.mod_loader.version.trim().is_empty() {
                    inst.mod_loader.name.clone()
                } else {
                    format!("{} ({})", inst.mod_loader.name, inst.mod_loader.version)
                };
                let (mem_text, res_text, java_text, lan_text) =
                    if let Some(ref s) = self.selected_settings {
                        let mem = format!("{} MB – {} MB", s.min_memory, s.max_memory);
                        let res = if s.fullscreen {
                            format!("{} × {} (Fullscreen)", s.width, s.height)
                        } else {
                            format!("{} × {} (Windowed)", s.width, s.height)
                        };
                        let jv = s
                            .java_path
                            .as_ref()
                            .map(|jp| jp.display().to_string())
                            .unwrap_or_else(|| "Automatic (recommended)".to_string());
                        let lan = format!(
                            "Auth: {}  ·  Tunnel: {}",
                            s.lan_auth_mode, s.lan_connection_provider
                        );
                        (mem, res, jv, lan)
                    } else {
                        (
                            "Default".to_string(),
                            "Default".to_string(),
                            "Automatic (recommended)".to_string(),
                            "Default".to_string(),
                        )
                    };
                let last_played_text = if inst.last_played.trim().is_empty() {
                    "Never"
                } else {
                    inst.last_played.as_str()
                };
                let exit_text = match inst.last_exit_code {
                    Some(code) => format!("{} (exit code: {code})", inst.last_launch_state),
                    None => inst.last_launch_state.clone(),
                };
                let mods_count = self
                    .mods_cache
                    .get(&inst.instance_id)
                    .map(|v| v.len())
                    .unwrap_or(self.selected_mods.len());
                let worlds_count = self
                    .worlds_cache
                    .get(&inst.instance_id)
                    .map(|v| v.len())
                    .unwrap_or(self.selected_worlds.len());
                let shots_count = self
                    .screenshots_cache
                    .get(&inst.instance_id)
                    .map(|v| v.len())
                    .unwrap_or(self.selected_screenshots.len());
                let backups_count = self
                    .backups_cache
                    .get(&inst.instance_id)
                    .map(|v| v.len())
                    .unwrap_or(self.selected_backups.len());

                egui::Grid::new("overview_instance_details_grid")
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Instance Name / ID:").color(p.text_muted));
                        ui.label(
                            RichText::new(format!("{}  ({})", inst.name, inst.instance_id))
                                .monospace(),
                        );
                        ui.end_row();

                        ui.label(RichText::new("Minecraft Version:").color(p.text_muted));
                        ui.label(RichText::new(&inst.version_id).monospace().strong());
                        ui.end_row();

                        ui.label(RichText::new("Mod Loader:").color(p.text_muted));
                        ui.label(RichText::new(loader_text).monospace());
                        ui.end_row();

                        ui.label(RichText::new("Game Directory:").color(p.text_muted));
                        ui.label(RichText::new(mc_dir.display().to_string()).monospace());
                        ui.end_row();

                        ui.label(RichText::new("Memory Allocation:").color(p.text_muted));
                        ui.label(RichText::new(mem_text).monospace());
                        ui.end_row();

                        ui.label(RichText::new("Window Resolution:").color(p.text_muted));
                        ui.label(RichText::new(res_text).monospace());
                        ui.end_row();

                        ui.label(RichText::new("Java Runtime:").color(p.text_muted));
                        ui.label(RichText::new(java_text).monospace());
                        ui.end_row();

                        ui.label(RichText::new("LAN Hosting:").color(p.text_muted));
                        ui.label(RichText::new(lan_text).monospace());
                        ui.end_row();

                        ui.label(RichText::new("Total Playtime:").color(p.text_muted));
                        ui.label(
                            RichText::new(format_duration(inst.total_playtime_seconds)).monospace(),
                        );
                        ui.end_row();

                        ui.label(RichText::new("Last Played:").color(p.text_muted));
                        ui.label(RichText::new(last_played_text).monospace());
                        ui.end_row();

                        ui.label(RichText::new("Last Status:").color(p.text_muted));
                        ui.label(RichText::new(exit_text).monospace());
                        ui.end_row();

                        ui.label(RichText::new("Installed Content:").color(p.text_muted));
                        ui.label(
                            RichText::new(format!(
                                "Mods: {mods_count}  ·  Worlds: {worlds_count}  ·  Screenshots: {shots_count}  ·  Backups: {backups_count}"
                            ))
                            .monospace(),
                        );
                        ui.end_row();
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Group:").color(p.text_muted));
                    ui.add(
                        TextEdit::singleline(&mut self.edit_group_input)
                            .hint_text("e.g. Survival, Modded")
                            .desired_width(140.0),
                    );
                    ui.add_space(6.0);
                    ui.label(RichText::new("Tags (comma-separated):").color(p.text_muted));
                    ui.add(
                        TextEdit::singleline(&mut self.edit_tags_input)
                            .hint_text("e.g. RPG, Tech, 1.21")
                            .desired_width(200.0),
                    );
                    if ui.button("Save Group & Tags").clicked() {
                        let store = InstanceStore::new(&self.paths);
                        match store.set_library_metadata(
                            &inst.instance_id,
                            None,
                            Some(&self.edit_group_input),
                            Some(&self.edit_tags_input),
                        ) {
                            Ok(_) => {
                                self.reload_instances();
                                self.notice_banner =
                                    Some(("Updated instance group and tags.".to_string(), false));
                            }
                            Err(e) => {
                                self.notice_banner =
                                    Some((format!("Failed to update tags: {e}"), true));
                            }
                        }
                    }
                });
            });

        if open_export {
            self.show_export_modal = true;
        }
        if trigger_repair {
            self.start_repair(ctx);
        }
        if change_icon_clicked {
            if let Some(chosen) = pick_file_dialog(
                self.t("workspace.icon.choose_title"),
                "Images (*.png;*.jpg;*.jpeg;*.webp;*.bmp;*.ico)|*.png;*.jpg;*.jpeg;*.webp;*.bmp;*.ico",
            ) {
                let store = InstanceStore::new(&self.paths);
                match store.set_instance_icon(&inst.instance_id, &chosen) {
                    Ok(_) => {
                        self.instance_icon_textures.clear();
                        self.reload_instances();
                    }
                    Err(err) => {
                        self.notice_banner = Some((format!("Failed to set icon: {err}"), true));
                    }
                }
            }
        }
        if reset_icon_clicked {
            let store = InstanceStore::new(&self.paths);
            if store.reset_instance_icon(&inst.instance_id).is_ok() {
                self.instance_icon_textures.clear();
                self.reload_instances();
            }
        }
    }

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
                        .corner_radius(CornerRadius::same(3))
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
        let p = self.palette;
        let mods_dir = self.paths.minecraft_dir(&inst.instance_id).join("mods");
        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let provenance = ModProvenanceRegistry::load(&inst_dir);
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
                .corner_radius(CornerRadius::same(3));
            let add_resp = ui.add_enabled(can_install, add_btn);
            if !can_install {
                add_resp.on_disabled_hover_text("Vanilla instances cannot install mods");
            } else if add_resp.clicked() {
                if !inst.mod_loader.name.is_empty()
                    && !inst.mod_loader.name.eq_ignore_ascii_case("vanilla")
                {
                    self.mod_loader_filter = inst.mod_loader.name.to_ascii_lowercase();
                }
                self.show_mod_browser_window = true;
            }

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
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Open mods/ Folder").clicked() {
                    let _ = std::fs::create_dir_all(&mods_dir);
                    open_external(&mods_dir.display().to_string());
                }
                if ui.button("Refresh Local").clicked() {
                    self.load_mods_async(&inst.instance_id, ctx, true);
                }
            });
        });
        ui.add_space(6.0);

        if self.selected_mods.is_empty() {
            let msg = if self.loading_mods.contains(&inst.instance_id) {
                "Scanning mods/ in background..."
            } else {
                "No mods installed in this instance's mods/ folder. Click '+ Add Mod' to browse Modrinth or CurseForge."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        let mut toggle_action: Option<(String, bool)> = None;
        let mut delete_action: Option<String> = None;
        let mut lock_toggle_action: Option<(String, bool)> = None;

        ScrollArea::vertical()
            .id_salt("local_mods_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for m in &self.selected_mods {
                    let compat = is_loader_compatible(m.loader, &inst.mod_loader.name);
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let mut enabled = m.enabled;
                                if ui.checkbox(&mut enabled, "").changed() {
                                    toggle_action = Some((m.file_name.clone(), enabled));
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
                                        let clean_fname = m.file_name.trim_end_matches(".disabled");
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
                                                RichText::new(lock_lbl).size(11.0).color(lock_col),
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
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });

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

    pub(crate) fn render_packs_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
    ) {
        let p = self.palette;
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let prev_kind = self.packs_kind;

        let kind_label = match self.packs_kind {
            ContentKind::ResourcePack => "Resource Packs",
            ContentKind::ShaderPack => "Shader Packs",
        };
        let section_title = match self.packs_kind {
            ContentKind::ResourcePack => {
                format!(
                    "Installed Resource Packs ({})",
                    self.selected_content_packs.len()
                )
            }
            ContentKind::ShaderPack => {
                format!(
                    "Installed Shader Packs ({})",
                    self.selected_content_packs.len()
                )
            }
        };

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(section_title).strong().color(p.text));
            egui::ComboBox::from_id_salt("packs_kind_combo")
                .selected_text(kind_label)
                .width(150.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.packs_kind,
                        ContentKind::ResourcePack,
                        "Resource Packs",
                    );
                    ui.selectable_value(
                        &mut self.packs_kind,
                        ContentKind::ShaderPack,
                        "Shader Packs",
                    );
                });
            let add_btn = Button::new(RichText::new("+ Add Pack").color(p.accent_text).strong())
                .fill(p.accent)
                .corner_radius(CornerRadius::same(3));
            if ui.add(add_btn).clicked() {
                self.show_pack_browser_window = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let folder_dir = mc_dir.join(self.packs_kind.folder_name());
                if ui.button("Open Folder").clicked() {
                    let _ = std::fs::create_dir_all(&folder_dir);
                    open_external(&folder_dir.display().to_string());
                }
                if ui.button("Refresh").clicked() {
                    self.load_content_packs_async(&inst.instance_id, self.packs_kind, ctx, true);
                }
            });
        });

        if self.packs_kind != prev_kind {
            self.packs_mr_results.clear();
            self.packs_cf_results.clear();
            self.selected_content_packs = self
                .content_packs_cache
                .get(&(inst.instance_id.clone(), self.packs_kind))
                .cloned()
                .unwrap_or_default();
            self.load_content_packs_async(&inst.instance_id, self.packs_kind, ctx, false);
        }

        ui.add_space(6.0);

        if self.selected_content_packs.is_empty() {
            let msg = if self
                .loading_content_packs
                .contains(&(inst.instance_id.clone(), self.packs_kind))
            {
                "Scanning folder in background..."
            } else {
                "No packs installed in this folder yet. Click '+ Add Pack' to browse Modrinth or CurseForge."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        let mut toggle_action: Option<(String, bool)> = None;
        let mut delete_action: Option<String> = None;

        ScrollArea::vertical()
            .id_salt("local_content_packs_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for pack in &self.selected_content_packs {
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let mut en = pack.enabled;
                                if ui.checkbox(&mut en, "").changed() {
                                    toggle_action = Some((pack.file_name.clone(), en));
                                }
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&pack.display_name).strong());
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

        let kind = self.packs_kind;
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
        let has_account = self.active_account().is_some();

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
        let mut duplicate_target: Option<String> = None;
        let mut backup_target: Option<String> = None;
        let mut delete_target: Option<String> = None;

        ScrollArea::vertical()
            .id_salt("worlds_list_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for w in &self.selected_worlds {
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
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
                                        .add_enabled(!busy, Button::new("Backup (.zip)"))
                                        .clicked()
                                    {
                                        backup_target = Some(w.folder_name.clone());
                                    }
                                    if quick_play_ok {
                                        let can_play = !busy && has_account && missing_manual == 0;
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

        ScrollArea::vertical()
            .id_salt("screenshots_list_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for s in &self.selected_screenshots {
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&s.file_name).monospace().strong());
                                    ui.label(
                                        RichText::new(format_bytes(s.file_size_bytes))
                                            .monospace()
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button(RichText::new("Delete").color(p.danger)).clicked()
                                    {
                                        delete_path = Some(s.path.clone());
                                    }
                                    if ui.button("Copy Path").clicked() {
                                        ctx.copy_text(s.path.display().to_string());
                                    }
                                    let open_btn = Button::new(
                                        RichText::new("Open Image").color(p.accent_text).strong(),
                                    )
                                    .fill(p.accent);
                                    if ui.add(open_btn).clicked() {
                                        open_external(&s.path.display().to_string());
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });

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

        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(3))
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
                        for line in lines.unwrap() {
                            let is_err = line.stream == LogStream::Stderr
                                || line.text.contains("ERROR")
                                || line.text.contains("Exception");
                            if self.logs_errors_only && !is_err {
                                continue;
                            }
                            let color = if is_err { p.danger } else { p.text };
                            ui.label(
                                RichText::new(&line.text)
                                    .monospace()
                                    .size(11.5)
                                    .color(color),
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

        let Some(ref mut settings) = self.selected_settings else {
            return;
        };

        let mut save_clicked = false;
        let mut apply_runtime = false;
        let mut install_java_major: Option<u32> = None;
        let mut optifine_install_mods = false;
        let mut optifine_launch_installer = false;
        let mut prepare_lan_provider: Option<String> = None;

        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let is_forge_family = matches!(
            inst.mod_loader.name.to_ascii_lowercase().as_str(),
            "forge" | "neoforge"
        );

        ScrollArea::vertical()
            .id_salt("instance_settings_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(3))
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
                            egui::ComboBox::from_id_salt("inst_settings_loader_combo")
                                .selected_text(&self.edit_runtime_loader)
                                .width(120.0)
                                .show_ui(ui, |ui| {
                                    for l in ["vanilla", "fabric", "quilt", "forge", "neoforge"] {
                                        ui.selectable_value(
                                            &mut self.edit_runtime_loader,
                                            l.to_string(),
                                            l,
                                        );
                                    }
                                });
                            if ui.button("Apply Version / Loader").clicked() {
                                apply_runtime = true;
                            }
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
                                        "Java {} ({}) — {}",
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

                        ui.label(RichText::new("MCW LAN Agent (Offline LAN Hosting)").strong());
                        ui.add_space(4.0);
                        let mut lan_on = LanAgentManager::is_enabled(
                            settings.offline_multiplayer_enabled,
                            &settings.lan_auth_mode,
                        );
                        if ui
                            .checkbox(
                                &mut lan_on,
                                "Enable MCW LAN Agent (allow offline players to join your LAN worlds)",
                            )
                            .changed()
                        {
                            settings.offline_multiplayer_enabled = lan_on;
                            settings.lan_auth_mode = if lan_on {
                                "private_offline".to_string()
                            } else {
                                "microsoft_only".to_string()
                            };
                        }
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(
                                    "Attaches the verified embedded mcw-lan-agent.jar (-javaagent) on launch.",
                                )
                                .size(11.5)
                                .color(p.text_muted),
                            );
                            if ui.button("Open LAN Agent Log").clicked() {
                                let log_file = LanAgentManager::log_path(&inst_dir);
                                if log_file.is_file() {
                                    open_external(&log_file.display().to_string());
                                } else if let Some(parent) = log_file.parent() {
                                    let _ = std::fs::create_dir_all(parent);
                                    open_external(&parent.display().to_string());
                                }
                            }
                        });
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label("LAN Connection Provider:");
                            let is_e4mc =
                                settings.lan_connection_provider.eq_ignore_ascii_case("e4mc");
                            let lan_provider_text = if is_e4mc {
                                "e4mc (Public Tunnel — auto-installs e4mc mod)"
                            } else {
                                "Manual (Local LAN / VPN)"
                            };
                            egui::ComboBox::from_id_salt("inst_lan_provider_combo")
                                .selected_text(lan_provider_text)
                                .width(320.0)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(!is_e4mc, "Manual (Local LAN / VPN)")
                                        .clicked()
                                        && is_e4mc
                                    {
                                        settings.lan_connection_provider = "manual".to_string();
                                        prepare_lan_provider = Some("manual".to_string());
                                    }
                                    if ui
                                        .selectable_label(
                                            is_e4mc,
                                            "e4mc (Public Tunnel — auto-installs e4mc mod)",
                                        )
                                        .clicked()
                                        && !is_e4mc
                                    {
                                        settings.lan_connection_provider = "e4mc".to_string();
                                        prepare_lan_provider = Some("e4mc".to_string());
                                    }
                                });
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.label(RichText::new("OptiFine").strong());
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!(
                                "Paste path to an official OptiFine_{}_*.jar file to validate and install.",
                                inst.version_id
                            ))
                            .size(11.5)
                            .color(p.text_muted),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                TextEdit::singleline(&mut self.optifine_jar_input)
                                    .hint_text("C:\\Users\\...\\Downloads\\OptiFine_1.20.4_HD_U_I7.jar")
                                    .desired_width(360.0),
                            );
                            if is_forge_family {
                                if ui.button("Install to mods/").clicked() {
                                    optifine_install_mods = true;
                                }
                            } else if ui.button("Run OptiFine Installer").clicked() {
                                optifine_launch_installer = true;
                            }
                            if ui.button("optifine.net").clicked() {
                                open_external("https://optifine.net/downloads");
                            }
                        });

                        ui.add_space(12.0);
                        let save_btn =
                            Button::new(RichText::new("Save Settings").color(p.accent_text).strong())
                                .fill(p.accent)
                                .corner_radius(CornerRadius::same(3));
                        if ui.add(save_btn).clicked() {
                            save_clicked = true;
                        }
                    });
            });

        if optifine_install_mods {
            let jar_path = PathBuf::from(self.optifine_jar_input.trim().trim_matches('"'));
            match OptiFineManager::install_into_mods(&jar_path, &mc_dir, Some(&inst.version_id)) {
                Ok(dest) => {
                    self.mods_cache.remove(&inst.instance_id);
                    self.optifine_jar_input.clear();
                    self.notice_banner =
                        Some((format!("Installed OptiFine into {}", dest.display()), false));
                }
                Err(e) => {
                    self.notice_banner = Some((format!("OptiFine install failed: {e}"), true));
                }
            }
        }
        if optifine_launch_installer {
            let jar_path = PathBuf::from(self.optifine_jar_input.trim().trim_matches('"'));
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
                            inst.name, self.edit_runtime_version, self.edit_runtime_loader
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

        if save_clicked {
            let jvm_args: Vec<String> = self
                .edit_jvm_args_text
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let game_args: Vec<String> = self
                .edit_game_args_text
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if let Some(ref mut s) = self.selected_settings {
                s.jvm_arguments = jvm_args;
                s.game_arguments = game_args;
                let store = InstanceStore::new(&self.paths);
                match store.save_settings(&inst.instance_id, s) {
                    Ok(()) => {
                        self.settings_cache
                            .insert(inst.instance_id.clone(), s.clone());
                        self.notice_banner = Some(("Settings saved.".to_string(), false));
                    }
                    Err(e) => {
                        self.notice_banner = Some((format!("Failed to save settings: {e}"), true));
                    }
                }
            }
        }
        if let Some(major) = install_java_major {
            self.start_java_install(major, ctx);
        }
        if let Some(provider) = prepare_lan_provider {
            if let Some(ref s) = self.selected_settings {
                let store = InstanceStore::new(&self.paths);
                let _ = store.save_settings(&inst.instance_id, s);
                self.settings_cache
                    .insert(inst.instance_id.clone(), s.clone());
            }
            self.start_prepare_lan_hosting(provider, ctx);
        }
    }
}
