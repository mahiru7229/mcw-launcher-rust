use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_settings_appearance_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        prefs_dirty: &mut bool,
    ) {
        let p = self.palette;
        ui.label(
            RichText::new(self.t("settings.section.appearance"))
                .strong()
                .size(15.0)
                .color(p.text),
        );
        ui.label(
            RichText::new(self.t("settings.section.appearance_detail"))
                .size(11.5)
                .color(p.text_muted),
        );
        ui.add_space(10.0);

        // Card 1: Built-in + External JSON Theme Selector
        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(self.t("launcher_settings.theme.selector")).strong());
                ui.label(
                    RichText::new(format!(
                        "Scans built-in themes and external JSON packs under {}",
                        self.paths.theme_root.display()
                    ))
                    .size(11.0)
                    .color(p.text_muted),
                );
                ui.add_space(8.0);

                let active_def = self.theme_catalog.find(&self.prefs.theme).clone();
                let mut picked_theme_id: Option<String> = None;
                let mut reload_clicked = false;
                let themes_snapshot = self.theme_catalog.themes.clone();

                ui.horizontal_wrapped(|ui| {
                    let combo_w = auto_combo_width(ui, &active_def.name, 180.0, 300.0);
                    egui::ComboBox::from_id_salt("launcher_settings_theme_combo")
                        .selected_text(&active_def.name)
                        .width(combo_w)
                        .truncate()
                        .show_ui(ui, |ui| {
                            for def in &themes_snapshot {
                                let selected = def.id.eq_ignore_ascii_case(&active_def.id);
                                if ui.selectable_label(selected, &def.name).clicked() && !selected {
                                    picked_theme_id = Some(def.id.clone());
                                }
                            }
                        });
                    if ui
                        .button(self.t("launcher_settings.theme.reload"))
                        .clicked()
                    {
                        reload_clicked = true;
                    }
                    if ui.button("Open Themes Folder").clicked() {
                        let _ = std::fs::create_dir_all(&self.paths.theme_root);
                        open_external(&self.paths.theme_root.display().to_string());
                    }
                });

                if !active_def.description.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(&active_def.description)
                            .size(11.0)
                            .color(p.text_muted),
                    );
                }
                if let Some(ref src_path) = active_def.source_path {
                    ui.label(
                        RichText::new(format!("Theme JSON: {}", src_path.display()))
                            .monospace()
                            .size(10.5)
                            .color(p.text_muted),
                    );
                }

                if let Some(tid) = picked_theme_id {
                    self.prefs.theme = tid;
                    self.apply_current_theme(ctx);
                    *prefs_dirty = true;
                }
                if reload_clicked {
                    self.reload_theme_catalog(ctx);
                }
            });

        ui.add_space(8.0);

        // Card 2: Custom Accent Hex & Motion Mode
        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!(
                        "{} (#RRGGBB)",
                        self.t("appearance.accent.mode.label")
                    ))
                    .strong(),
                );
                ui.label(
                    RichText::new(
                        "Override the primary accent color with contrast-aware button text.",
                    )
                    .size(11.0)
                    .color(p.text_muted),
                );
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    let preview_col =
                        parse_hex_color(&self.custom_accent_input).unwrap_or(self.palette.accent);
                    let (swatch_rect, _) =
                        ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(swatch_rect, CornerRadius::same(6), preview_col);

                    ui.add(
                        TextEdit::singleline(&mut self.custom_accent_input)
                            .hint_text(format_hex_color(self.palette.accent))
                            .desired_width(110.0),
                    );
                    if ui.button("Apply Accent").clicked() {
                        if parse_hex_color(&self.custom_accent_input).is_some() {
                            self.prefs.custom_accent_hex =
                                self.custom_accent_input.trim().to_string();
                            self.apply_current_theme(ctx);
                            *prefs_dirty = true;
                        } else {
                            self.notice_banner = Some((
                                "Enter a valid 6-digit hex color such as #10B981.".to_string(),
                                true,
                            ));
                        }
                    }
                    if ui.button(self.t("appearance.accent.reset")).clicked() {
                        self.custom_accent_input.clear();
                        self.prefs.custom_accent_hex.clear();
                        self.apply_current_theme(ctx);
                        *prefs_dirty = true;
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(RichText::new("UI Motion & Animations").strong());
                ui.add_space(4.0);
                let mut picked_motion = self.motion_mode;
                let motion_w = auto_combo_width(ui, picked_motion.label(), 150.0, 240.0);
                egui::ComboBox::from_id_salt("launcher_settings_motion_combo")
                    .selected_text(picked_motion.label())
                    .width(motion_w)
                    .truncate()
                    .show_ui(ui, |ui| {
                        for m in [MotionMode::Full, MotionMode::Reduced, MotionMode::Off] {
                            ui.selectable_value(&mut picked_motion, m, m.label());
                        }
                    });
                if picked_motion != self.motion_mode {
                    self.motion_mode = picked_motion;
                    self.prefs.motion_mode = picked_motion.as_str().to_string();
                    self.apply_current_theme(ctx);
                    *prefs_dirty = true;
                }
            });

        ui.add_space(8.0);

        // Card 3: Home background picture, panel transparency & dimming (no blur)
        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Home background").strong());
                ui.label(
                    RichText::new(
                        "Pick a PNG, JPEG or WebP picture. Panels stay translucent over it; there is no blur.",
                    )
                    .size(11.0)
                    .color(p.text_muted),
                );
                ui.add_space(6.0);
                if ui
                    .checkbox(
                        &mut self.prefs.background_enabled,
                        "Show background picture",
                    )
                    .changed()
                {
                    *prefs_dirty = true;
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Choose a picture...").clicked() {
                        if let Some(path) = pick_file_dialog(
                            "Choose a background picture",
                            "Pictures (*.png;*.jpg;*.jpeg;*.webp)|*.png;*.jpg;*.jpeg;*.webp|All Files (*.*)|*.*",
                        ) {
                            self.prefs.background_image_path = path.display().to_string();
                            self.prefs.background_enabled = true;
                            *prefs_dirty = true;
                        }
                    }
                    if ui
                        .add_enabled(
                            !self.prefs.background_image_path.is_empty(),
                            Button::new("Use default"),
                        )
                        .clicked()
                    {
                        self.prefs.background_image_path.clear();
                        *prefs_dirty = true;
                    }
                });
                let shown = if self.prefs.background_image_path.is_empty() {
                    "Default pixel landscape".to_string()
                } else {
                    self.prefs.background_image_path.clone()
                };
                ui.label(
                    RichText::new(shown)
                        .monospace()
                        .size(10.5)
                        .color(p.text_muted),
                );
                ui.add_space(8.0);
                let mut opacity_pct = self.prefs.panel_opacity * 100.0;
                if ui
                    .add(
                        egui::Slider::new(&mut opacity_pct, 25.0..=100.0)
                            .suffix("%")
                            .text("Panel opacity (lower = more transparent)"),
                    )
                    .changed()
                {
                    self.prefs.panel_opacity = (opacity_pct / 100.0).clamp(0.25, 1.0);
                    self.apply_current_theme(ctx);
                    *prefs_dirty = true;
                }
                let mut dim_pct = self.prefs.background_dim * 100.0;
                if ui
                    .add(
                        egui::Slider::new(&mut dim_pct, 0.0..=85.0)
                            .suffix("%")
                            .text("Background dimming"),
                    )
                    .changed()
                {
                    self.prefs.background_dim = (dim_pct / 100.0).clamp(0.0, 0.85);
                    *prefs_dirty = true;
                }
            });
    }
    pub(crate) fn render_settings_storage_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        prefs_dirty: &mut bool,
    ) {
        let p = self.palette;
        ui.label(
            RichText::new(self.t("storage.settings.section"))
                .strong()
                .size(15.0)
                .color(p.text),
        );
        ui.label(
            RichText::new(self.t("storage.settings.section_detail"))
                .size(11.5)
                .color(p.text_muted),
        );
        ui.add_space(10.0);

        // Card 1: Disk Usage Summary
        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Launcher Disk Usage").strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_enabled(!self.storage_scanning, Button::new("Scan Storage"))
                            .clicked()
                        {
                            self.start_storage_scan(ctx);
                        }
                        if ui.button("Open Launcher Folder").clicked() {
                            open_external(&self.paths.project_root.display().to_string());
                        }
                    });
                });
                ui.add_space(6.0);

                if self.storage_scanning {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Measuring launcher folders...").color(p.accent));
                    });
                } else if let Some(ref s) = self.storage_summary {
                    egui::Grid::new("storage_usage_grid")
                        .num_columns(2)
                        .spacing([20.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("Instances:").color(p.text_muted));
                            ui.label(RichText::new(format_bytes(s.instances_bytes)).monospace());
                            ui.end_row();
                            ui.label(
                                RichText::new("Cache (assets, libraries, versions):")
                                    .color(p.text_muted),
                            );
                            ui.label(RichText::new(format_bytes(s.cache_bytes)).monospace());
                            ui.end_row();
                            ui.label(RichText::new("Java Runtimes:").color(p.text_muted));
                            ui.label(RichText::new(format_bytes(s.runtimes_bytes)).monospace());
                            ui.end_row();
                            ui.label(RichText::new("Backups:").color(p.text_muted));
                            ui.label(RichText::new(format_bytes(s.backups_bytes)).monospace());
                            ui.end_row();
                            ui.label(RichText::new("Logs:").color(p.text_muted));
                            ui.label(RichText::new(format_bytes(s.logs_bytes)).monospace());
                            ui.end_row();
                            ui.label(RichText::new("Total:").strong());
                            ui.label(
                                RichText::new(format_bytes(s.total_bytes))
                                    .monospace()
                                    .strong(),
                            );
                            ui.end_row();
                        });
                } else {
                    ui.label(
                        RichText::new("Click 'Scan Storage' to calculate disk usage.")
                            .size(11.5)
                            .color(p.text_muted),
                    );
                }
            });

        ui.add_space(8.0);

        // Card 2: Unused Version Retention & Cache Cleanup
        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Cache & Unused Version Cleanup").strong());
                ui.add_space(4.0);
                ui.label(
                    RichText::new(self.t("storage.legacy.protected_hint"))
                        .size(11.0)
                        .color(p.text_muted),
                );
                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    let lbl = self.t("storage.legacy.jar_retention.label").to_string();
                    let suffix = self.t("storage.legacy.jar_retention.suffix").to_string();
                    ui.label(lbl);
                    if ui
                        .add(
                            egui::DragValue::new(&mut self.prefs.unused_version_retention_days)
                                .range(0..=365)
                                .suffix(suffix),
                        )
                        .changed()
                    {
                        *prefs_dirty = true;
                    }
                });

                ui.add_space(6.0);
                let unused_count = self.storage_unused_versions.len();
                let unused_bytes: u64 = self
                    .storage_unused_versions
                    .iter()
                    .map(|c| c.size_bytes)
                    .sum();
                ui.label(
                    RichText::new(format!(
                        "Eligible unused Minecraft version folder(s): {unused_count} ({})",
                        format_bytes(unused_bytes)
                    ))
                    .size(11.5)
                    .color(if unused_count > 0 {
                        p.warning
                    } else {
                        p.text_muted
                    }),
                );

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            !self.storage_cleaning,
                            Button::new("Clear API & Temp Cache"),
                        )
                        .clicked()
                    {
                        self.start_clear_cache(ctx);
                    }
                    let prune_lbl = if unused_count > 0 {
                        format!("Prune Unused Versions ({})", format_bytes(unused_bytes))
                    } else {
                        "Prune Unused Versions".to_string()
                    };
                    if ui
                        .add_enabled(
                            !self.storage_cleaning && unused_count > 0,
                            Button::new(RichText::new(prune_lbl).color(p.warning)),
                        )
                        .clicked()
                    {
                        self.start_prune_unused_versions(ctx);
                    }
                });
            });

        ui.add_space(8.0);

        // Card 3: Shared Content Store & Hardlink Deduplication
        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("Shared Content Store & Hardlink Deduplication").strong(),
                );
                ui.label(
                    RichText::new(
                        "Deduplicates identical immutable .jar mods and resource/shader packs across instances via SHA-256 content addressing in cache/content_store/.",
                    )
                    .size(11.5)
                    .color(p.text_muted),
                );
                ui.add_space(6.0);
                if let Some(ref st) = self.content_store_stats {
                    ui.label(
                        RichText::new(format!(
                            "Canonical blobs: {} ({}) · Deduplicated copies: {} (~{} saved)",
                            st.blob_count,
                            format_bytes(st.total_blob_bytes),
                            st.deduplicated_files,
                            format_bytes(st.estimated_saved_bytes)
                        ))
                        .monospace()
                        .size(11.5)
                        .color(p.text_muted),
                    );
                }
                ui.add_space(6.0);
                let dedup_lbl = if self.dedup_running {
                    "Deduplicating..."
                } else {
                    "Deduplicate Shared Mods & Packs"
                };
                if ui
                    .add_enabled(!self.dedup_running, Button::new(dedup_lbl))
                    .clicked()
                {
                    self.start_content_store_dedup(ctx);
                }
            });
    }

    pub(crate) fn render_settings_updates_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        prefs_dirty: &mut bool,
    ) {
        let p = self.palette;
        ui.label(
            RichText::new(self.t("launcher_settings.update.title"))
                .strong()
                .size(15.0)
                .color(p.text),
        );
        ui.label(
            RichText::new(self.t("launcher_settings.update.subtitle"))
                .size(11.5)
                .color(p.text_muted),
        );
        ui.add_space(10.0);

        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!("Current version: v{}", mcw_models::APP_VERSION))
                        .monospace()
                        .strong(),
                );
                ui.add_space(6.0);

                if !mcw_updater::UPDATES_ENABLED {
                    Frame::new()
                        .fill(p.elevated)
                        .stroke(Stroke::new(1.0_f32, p.warning))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new("Launcher updates are currently disabled.")
                                    .strong()
                                    .color(p.warning),
                            );
                            ui.label(
                                RichText::new(
                                    "The legacy MCW Launcher Python update feed has been completely removed. MCW Launcher: Rust Edition updates will be re-enabled once dedicated Rust Edition releases are published.",
                                )
                                .size(11.5)
                                .color(p.text_muted),
                            );
                        });
                    ui.add_space(6.0);
                }

                let auto_lbl = self.t("launcher_settings.update.auto_check").to_string();
                if ui
                    .add_enabled(
                        mcw_updater::UPDATES_ENABLED,
                        egui::Checkbox::new(&mut self.prefs.auto_check_updates, auto_lbl),
                    )
                    .changed()
                {
                    *prefs_dirty = true;
                }

                ui.add_space(6.0);
                ui.add_enabled_ui(mcw_updater::UPDATES_ENABLED, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(self.t("launcher_settings.update.channel"));
                        let mut chan = UpdateChannel::parse(&self.prefs.update_channel);
                        let chan_lbl = match chan {
                            UpdateChannel::Stable => self.t("update.channel.stable").to_string(),
                            UpdateChannel::Beta => self.t("update.channel.beta").to_string(),
                        };
                        egui::ComboBox::from_id_salt("launcher_settings_update_channel")
                            .selected_text(&chan_lbl)
                            .width(auto_combo_width(ui, &chan_lbl, 120.0, 200.0))
                            .truncate()
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut chan,
                                    UpdateChannel::Stable,
                                    self.t("update.channel.stable"),
                                );
                                ui.selectable_value(
                                    &mut chan,
                                    UpdateChannel::Beta,
                                    self.t("update.channel.beta"),
                                );
                            });
                        if chan.as_str() != self.prefs.update_channel {
                            self.prefs.update_channel = chan.as_str().to_string();
                            *prefs_dirty = true;
                        }
                    });
                });

                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            mcw_updater::UPDATES_ENABLED && !self.update_checking,
                            Button::new(self.t("launcher_settings.update.check")),
                        )
                        .clicked()
                    {
                        self.start_update_check(true, ctx);
                    }
                    if mcw_updater::UPDATES_ENABLED
                        && self
                            .last_update_check
                            .as_ref()
                            .is_some_and(|r| r.update_available && r.latest.is_some())
                    {
                        let open_btn = Button::new(
                            RichText::new("View & Install Update...")
                                .color(p.accent_text)
                                .strong(),
                        )
                        .fill(p.accent);
                        if ui.add(open_btn).clicked() {
                            self.show_update_dialog = true;
                        }
                    }
                });

                ui.add_space(6.0);
                if self.update_checking {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(&self.update_status_text).color(p.accent));
                    });
                } else {
                    ui.label(
                        RichText::new(format!("Status: {}", self.update_status_text))
                            .size(11.5)
                            .color(p.text_muted),
                    );
                }
            });
    }

    pub(crate) fn render_update_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_update_dialog {
            return;
        }
        let Some(ref check) = self.last_update_check.clone() else {
            self.show_update_dialog = false;
            return;
        };
        let Some(ref release) = check.latest else {
            self.show_update_dialog = false;
            return;
        };

        let p = self.palette;
        let mut open = self.show_update_dialog;
        let mut trigger_install: Option<ReleaseInfo> = None;
        let any_running = self
            .launch_states
            .values()
            .any(|s| matches!(s, LaunchState::Running { .. }));

        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_update_dialog");
        let title = format!("{} — v{}", self.t("update.dialog.title"), release.version);
        let builder = centered_viewport_builder(ctx, title, [560.0, 440.0], [440.0, 340.0]);

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| {
                i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)
            }) {
                open = false;
            }
            egui::TopBottomPanel::bottom("update_dialog_footer")
                .frame(
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(vp_ctx, |ui| {
                    ui.horizontal(|ui| {
                        let upd_btn = Button::new(
                            RichText::new(if self.update_downloading {
                                "Downloading..."
                            } else {
                                self.t("update.dialog.update_now")
                            })
                            .color(p.accent_text)
                            .strong(),
                        )
                        .fill(p.accent);
                        let can_install = !self.update_downloading
                            && !any_running
                            && release.download_url.is_some();
                        if ui.add_enabled(can_install, upd_btn).clicked() {
                            trigger_install = Some(release.clone());
                        }
                        if !release.html_url.is_empty()
                            && ui.button(self.t("update.dialog.open_release")).clicked()
                        {
                            open_external(&release.html_url);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(self.t("update.dialog.later")).clicked() {
                                open = false;
                            }
                        });
                    });
                });

            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    ui.label(
                        RichText::new(format!("MCW Launcher v{} is available", release.version))
                            .strong()
                            .size(16.0)
                            .color(p.text),
                    );
                    ui.label(
                        RichText::new(format!(
                            "Current: v{}  ->  New: v{} ({})",
                            check.current_version, release.version, release.source
                        ))
                        .monospace()
                        .size(11.5)
                        .color(p.accent),
                    );
                    if let Some(ref asset) = release.asset_name {
                        let size_str = release
                            .asset_size
                            .map(format_bytes)
                            .unwrap_or_else(|| "unknown size".to_string());
                        ui.label(
                            RichText::new(format!("Package: {asset} · {size_str}"))
                                .size(11.5)
                                .color(p.text_muted),
                        );
                    }
                    if any_running {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Close running Minecraft instance(s) before installing this update.",
                            )
                            .color(p.warning),
                        );
                    }
                    if release.download_url.is_none() {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "This release does not contain a direct downloadable package; use 'Open release page'.",
                            )
                            .color(p.warning),
                        );
                    }

                    ui.add_space(8.0);
                    ui.label(RichText::new(self.t("update.dialog.release_notes")).strong());
                    ui.add_space(4.0);

                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ScrollArea::vertical()
                                .id_salt("update_release_notes_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let notes = if release.notes.trim().is_empty() {
                                        self.t("update.dialog.no_release_notes").to_string()
                                    } else {
                                        release.notes.clone()
                                    };
                                    ui.label(RichText::new(notes).size(12.0));
                                });
                        });
                });
        });

        self.show_update_dialog = open;
        if let Some(rel) = trigger_install {
            self.start_update_download_and_install(rel, ctx);
        }
    }

    pub(crate) fn render_first_run_wizard(&mut self, ctx: &egui::Context) {
        if !self.show_first_run_wizard {
            return;
        }
        let p = self.palette;
        let mut open = self.show_first_run_wizard;
        let mut apply_clicked = false;
        let mut skip_clicked = false;
        let mut use_recommended_clicked = false;

        let first_run_title = format!("{} — MCW Launcher", self.t("first_run.title"));
        let finish_lbl = self.t("first_run.finish").to_string();
        let skip_lbl = self.t("first_run.skip").to_string();

        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_first_run_setup");
        let builder =
            centered_viewport_builder(ctx, first_run_title, [620.0, 520.0], [480.0, 400.0]);

        let available_langs: Vec<(String, String)> = self
            .lang
            .available_languages()
            .into_iter()
            .map(|(loc, name)| (loc.to_string(), name.to_string()))
            .collect();

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| {
                i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)
            }) {
                open = false;
            }
            egui::TopBottomPanel::bottom("first_run_footer")
                .frame(
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(vp_ctx, |ui| {
                    ui.horizontal(|ui| {
                        let apply_btn = Button::new(
                            RichText::new(&finish_lbl)
                                .color(p.accent_text)
                                .strong(),
                        )
                        .fill(p.accent);
                        if ui.add(apply_btn).clicked() {
                            apply_clicked = true;
                        }
                        if ui.button(&skip_lbl).clicked() {
                            skip_clicked = true;
                        }
                    });
                });

            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    ScrollArea::vertical()
                        .id_salt("first_run_wizard_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new(self.t("launcher_settings.first_run.title"))
                                    .strong()
                                    .size(16.0)
                                    .color(p.text),
                            );
                            ui.label(
                                RichText::new(self.t("launcher_settings.first_run.detail"))
                                    .size(11.5)
                                    .color(p.text_muted),
                            );
                            ui.add_space(10.0);

                            // Card 1: Hardware & Java Inspection
                            Frame::new()
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(egui::Margin::same(12))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new("Detected Hardware & Java").strong());
                                    ui.add_space(6.0);
                                    if self.first_run_scanning {
                                        ui.horizontal(|ui| {
                                            ui.spinner();
                                            ui.label(
                                                RichText::new(
                                                    self.t("launcher_settings.first_run.scanning"),
                                                )
                                                .color(p.accent),
                                            );
                                        });
                                    } else if let Some(ref rec) = self.first_run_rec {
                                        ui.label(
                                            RichText::new(format!(
                                                "Total RAM: {} MB  ·  Available: {} MB  ·  Recommended: {} MB - {} MB",
                                                rec.total_memory_mb,
                                                rec.available_memory_mb,
                                                rec.recommended_min_memory_mb,
                                                rec.recommended_max_memory_mb
                                            ))
                                            .monospace()
                                            .size(11.5),
                                        );
                                        ui.add_space(4.0);
                                        let java_summary = match &rec.recommended_java {
                                            Some(j) => format!(
                                                "Preferred Java: Java {} ({})",
                                                j.major,
                                                j.path.display()
                                            ),
                                            None => format!(
                                                "No system/managed Java detected yet (detected {} total). The launcher can provision Java automatically when launching.",
                                                rec.java_installations.len()
                                            ),
                                        };
                                        ui.label(
                                            RichText::new(java_summary)
                                                .size(11.5)
                                                .color(p.text_muted),
                                        );
                                    }
                                });

                            ui.add_space(8.0);

                            // Card 2: Recommended Defaults
                            Frame::new()
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(egui::Margin::same(12))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new("Default Launcher Settings").strong());
                                    ui.add_space(6.0);

                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(self.t("launcher_settings.language.title"));
                                        let cur_lbl = available_langs
                                            .iter()
                                            .find(|(loc, _)| loc == &self.first_run_language)
                                            .map(|(loc, name)| format!("{name} ({loc})"))
                                            .unwrap_or_else(|| self.first_run_language.clone());
                                        egui::ComboBox::from_id_salt("first_run_lang_combo")
                                            .selected_text(&cur_lbl)
                                            .width(auto_combo_width(ui, &cur_lbl, 180.0, 260.0))
                                            .truncate()
                                            .show_ui(ui, |ui| {
                                                for (loc, name) in &available_langs {
                                                    ui.selectable_value(
                                                        &mut self.first_run_language,
                                                        loc.clone(),
                                                        format!("{name} ({loc})"),
                                                    );
                                                }
                                            });
                                    });

                                    ui.add_space(6.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("Default Min RAM (MB):");
                                        ui.add(
                                            egui::DragValue::new(&mut self.first_run_min_ram)
                                                .range(256..=16384)
                                                .speed(128.0),
                                        );
                                        ui.label("Default Max RAM (MB):");
                                        ui.add(
                                            egui::DragValue::new(&mut self.first_run_max_ram)
                                                .range(512..=32768)
                                                .speed(128.0),
                                        );
                                        if self.first_run_rec.is_some()
                                            && ui
                                                .button(self.t("first_run.memory.use_recommended"))
                                                .clicked()
                                        {
                                            use_recommended_clicked = true;
                                        }
                                    });

                                    ui.add_space(6.0);
                                    let auto_upd_lbl = if mcw_updater::UPDATES_ENABLED {
                                        self.t("launcher_settings.update.auto_check").to_string()
                                    } else {
                                        format!(
                                            "{} (currently disabled)",
                                            self.t("launcher_settings.update.auto_check")
                                        )
                                    };
                                    ui.add_enabled(
                                        mcw_updater::UPDATES_ENABLED,
                                        egui::Checkbox::new(
                                            &mut self.first_run_auto_updates,
                                            auto_upd_lbl,
                                        ),
                                    );
                                    ui.checkbox(
                                        &mut self.first_run_discord_rpc,
                                        "Enable Discord Rich Presence",
                                    );
                                });
                        });
                });
        });

        if use_recommended_clicked {
            if let Some(ref rec) = self.first_run_rec {
                self.first_run_min_ram = rec.recommended_min_memory_mb;
                self.first_run_max_ram = rec.recommended_max_memory_mb;
            }
        }
        if apply_clicked {
            let min_ram = self
                .first_run_min_ram
                .max(256)
                .min(self.first_run_max_ram.max(512));
            let max_ram = self.first_run_max_ram.max(min_ram);
            self.prefs.default_min_memory = min_ram;
            self.prefs.default_max_memory = max_ram;
            self.prefs.auto_check_updates = self.first_run_auto_updates;
            self.prefs.discord_rpc_enabled = self.first_run_discord_rpc;
            self.prefs.first_run_completed = true;
            self.lang.set_language(&self.first_run_language);
            self.discord_rpc.configure(
                self.prefs.discord_rpc_enabled,
                self.prefs.discord_client_id.clone(),
            );
            self.save_prefs();
            self.show_first_run_wizard = false;
            self.notice_banner = Some((
                self.t("launcher_settings.first_run.completed").to_string(),
                false,
            ));
        } else if skip_clicked {
            self.prefs.first_run_completed = true;
            self.save_prefs();
            self.show_first_run_wizard = false;
            self.notice_banner = Some((
                self.t("launcher_settings.first_run.cancelled").to_string(),
                false,
            ));
        } else {
            self.show_first_run_wizard = open;
        }
    }
}
