use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_settings_appearance_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        prefs_dirty: &mut bool,
        search_q: Option<&str>,
    ) -> usize {
        let p = self.palette;
        let cat_title = self.t("settings.section.appearance").to_string();
        let cat_detail = self.t("settings.section.appearance_detail").to_string();
        if search_q.is_none() {
            ui.label(RichText::new(&cat_title).strong().size(15.0).color(p.text));
            ui.label(RichText::new(&cat_detail).size(11.5).color(p.text_muted));
            ui.add_space(10.0);
        }
        let mut matched = 0usize;

        // Card 1: Built-in + External JSON Theme Selector
        let theme_title = self.t("launcher_settings.theme.selector").to_string();
        let theme_reload_lbl = self.t("launcher_settings.theme.reload").to_string();
        if crate::app::views::settings_modal::settings_card_matches(
            search_q,
            &[
                &cat_title,
                &theme_title,
                &theme_reload_lbl,
                "theme selector open themes folder dark light obsidian emerald json color scheme",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                crate::app::views::settings_modal::render_settings_search_badge(ui, p, &cat_title);
            }
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
                                    if ui.selectable_label(selected, &def.name).clicked()
                                        && !selected
                                    {
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
                        if !self.theme_catalog.issues.is_empty()
                            && ui
                                .button(
                                    RichText::new(format!(
                                        "Theme Issues ({})",
                                        self.theme_catalog.issues.len()
                                    ))
                                    .color(p.warning),
                                )
                                .clicked()
                        {
                            self.show_theme_issues_window = true;
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
        }

        // Card 2: Custom Accent Hex & Motion Mode
        let accent_title = self.t("appearance.accent.mode.label").to_string();
        let accent_reset_lbl = self.t("appearance.accent.reset").to_string();
        if crate::app::views::settings_modal::settings_card_matches(
            search_q,
            &[
                &cat_title,
                &accent_title,
                &accent_reset_lbl,
                "custom accent colour color hex rrggbb apply reset ui motion animations full reduced off",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                crate::app::views::settings_modal::render_settings_search_badge(ui, p, &cat_title);
            }
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
        }

        // Card 3: Home background picture, panel transparency & dimming (no blur)
        if crate::app::views::settings_modal::settings_card_matches(
            search_q,
            &[
                &cat_title,
                "home background picture image wallpaper choose png jpeg webp use default panel opacity transparency dimming",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                crate::app::views::settings_modal::render_settings_search_badge(ui, p, &cat_title);
            }
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
                        "Pure black by default. Pick a PNG, JPEG or WebP picture if you want a custom image.",
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
                    let choose_bg_lbl = if self.bg_picker_open {
                        "Opening picker..."
                    } else {
                        "Choose a picture..."
                    };
                    if ui
                        .add_enabled(!self.bg_picker_open, Button::new(choose_bg_lbl))
                        .clicked()
                    {
                        self.bg_picker_open = true;
                        let tx = self.tx.clone();
                        let ctx_c = ctx.clone();
                        pick_file_dialog_async(
                            "Choose a background picture".to_string(),
                            "Pictures (*.png;*.jpg;*.jpeg;*.webp)|*.png;*.jpg;*.jpeg;*.webp|All Files (*.*)|*.*".to_string(),
                            move |path| {
                                let _ = tx.send(AppEvent::BackgroundPicturePicked(path));
                                ctx_c.request_repaint();
                            },
                        );
                    }
                    if ui
                        .add_enabled(
                            !self.prefs.background_image_path.is_empty(),
                            Button::new("Use Pure Black"),
                        )
                        .clicked()
                    {
                        self.prefs.background_image_path.clear();
                        *prefs_dirty = true;
                    }
                });
                let shown = if self.prefs.background_image_path.is_empty() {
                    "Default (Pure Black #000000)".to_string()
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
            ui.add_space(8.0);
        }

        // Card 4: Custom System Font, UI Scale & Font Size
        let typo_title = self.t("appearance.typography.title").to_string();
        let typo_detail = self.t("appearance.typography.detail").to_string();
        let font_lbl = self.t("appearance.font.label").to_string();
        let scale_lbl = self.t("appearance.scale.label").to_string();
        let font_size_lbl = self.t("appearance.font_size.label").to_string();
        if crate::app::views::settings_modal::settings_card_matches(
            search_q,
            &[
                &cat_title,
                &typo_title,
                &typo_detail,
                &font_lbl,
                &scale_lbl,
                &font_size_lbl,
                "font family custom typography ui scale zoom font size text size dpi",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                crate::app::views::settings_modal::render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&typo_title).strong());
                    ui.label(RichText::new(&typo_detail).size(11.0).color(p.text_muted));
                    ui.add_space(8.0);

                    let default_font_lbl = self.t("appearance.font.default").to_string();
                    let refresh_lbl = self.t("appearance.font.refresh").to_string();
                    let reset_lbl = self.t("appearance.reset").to_string();
                    let cur_font_disp = if self.prefs.custom_font_family.trim().is_empty() {
                        default_font_lbl.clone()
                    } else {
                        self.prefs.custom_font_family.clone()
                    };
                    let mut font_changed = false;
                    let mut picked_font = self.prefs.custom_font_family.clone();

                    ui.horizontal_wrapped(|ui| {
                        ui.label(&font_lbl);
                        let combo_w = auto_combo_width(ui, &cur_font_disp, 200.0, 340.0);
                        egui::ComboBox::from_id_salt("launcher_settings_font_combo")
                            .selected_text(&cur_font_disp)
                            .width(combo_w)
                            .truncate()
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(
                                        picked_font.trim().is_empty(),
                                        &default_font_lbl,
                                    )
                                    .clicked()
                                {
                                    picked_font.clear();
                                    font_changed = true;
                                }
                                let bundled_mono = crate::theme::BUNDLED_JETBRAINS_MONO_OPTION;
                                let bundled_selected =
                                    picked_font.eq_ignore_ascii_case(bundled_mono);
                                if ui
                                    .selectable_label(bundled_selected, bundled_mono)
                                    .clicked()
                                    && !bundled_selected
                                {
                                    picked_font = bundled_mono.to_string();
                                    font_changed = true;
                                }
                                for font in &self.system_fonts {
                                    let selected = picked_font.eq_ignore_ascii_case(&font.name);
                                    if ui.selectable_label(selected, &font.name).clicked()
                                        && !selected
                                    {
                                        picked_font = font.name.clone();
                                        font_changed = true;
                                    }
                                }
                            });
                        if ui.button(&refresh_lbl).clicked() {
                            self.system_fonts = crate::theme::scan_system_fonts();
                            font_changed = true;
                        }
                    });
                    if font_changed {
                        self.prefs.custom_font_family = picked_font;
                        self.apply_typography_and_scale(ctx);
                        *prefs_dirty = true;
                    }

                    ui.add_space(6.0);
                    let applied_scale = self.prefs.ui_scale.clamp(0.75, 1.75);
                    let scale_pending_diff =
                        ((self.pending_ui_scale_pct / 100.0) - applied_scale).abs() > 0.005;
                    let apply_lbl = self.t("appearance.scale.apply").to_string();
                    let high_warn = self.t("appearance.scale.high_warning").to_string();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(&scale_lbl);
                        ui.add(
                            egui::Slider::new(&mut self.pending_ui_scale_pct, 75.0..=175.0)
                                .step_by(5.0)
                                .suffix("%"),
                        );
                        if ui
                            .add_enabled(
                                scale_pending_diff,
                                Button::new(RichText::new(&apply_lbl).strong()),
                            )
                            .clicked()
                        {
                            self.prefs.ui_scale =
                                (self.pending_ui_scale_pct / 100.0).clamp(0.75, 1.75);
                            self.pending_ui_scale_pct = (self.prefs.ui_scale * 100.0).round();
                            self.apply_typography_and_scale(ctx);
                            *prefs_dirty = true;
                        }
                        if ((applied_scale - 1.0).abs() > 0.01
                            || (self.pending_ui_scale_pct - 100.0).abs() > 0.5)
                            && ui.button(format!("{reset_lbl} (100%)")).clicked()
                        {
                            self.pending_ui_scale_pct = 100.0;
                            self.prefs.ui_scale = 1.0;
                            self.apply_typography_and_scale(ctx);
                            *prefs_dirty = true;
                        }
                    });
                    if self.pending_ui_scale_pct > 125.0 {
                        ui.label(RichText::new(high_warn).size(11.5).color(p.warning));
                    }

                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(&font_size_lbl);
                        if ui
                            .add(
                                egui::Slider::new(&mut self.prefs.font_size, 10.0..=22.0)
                                    .step_by(0.5)
                                    .suffix(" px"),
                            )
                            .changed()
                        {
                            self.prefs.font_size = self.prefs.font_size.clamp(10.0, 22.0);
                            self.apply_current_theme(ctx);
                            *prefs_dirty = true;
                        }
                        if (self.prefs.font_size - 13.0).abs() > 0.1
                            && ui.button(format!("{reset_lbl} (13 px)")).clicked()
                        {
                            self.prefs.font_size = 13.0;
                            self.apply_current_theme(ctx);
                            *prefs_dirty = true;
                        }
                    });

                    ui.add_space(6.0);
                    let preview_txt = self.t("appearance.font.preview").to_string();
                    ui.label(
                        RichText::new(preview_txt)
                            .size(self.prefs.font_size)
                            .color(p.text_muted),
                    );
                });
            ui.add_space(8.0);
        }
        matched
    }

    pub(crate) fn render_settings_storage_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        prefs_dirty: &mut bool,
        search_q: Option<&str>,
    ) -> usize {
        let p = self.palette;
        let cat_title = self.t("storage.settings.section").to_string();
        let cat_detail = self.t("storage.settings.section_detail").to_string();
        if search_q.is_none() {
            ui.label(RichText::new(&cat_title).strong().size(15.0).color(p.text));
            ui.label(RichText::new(&cat_detail).size(11.5).color(p.text_muted));
            ui.add_space(10.0);
        }
        let mut matched = 0usize;

        // Card 1: Disk Usage Summary
        if crate::app::views::settings_modal::settings_card_matches(
            search_q,
            &[
                &cat_title,
                "launcher disk usage scan storage open launcher folder instances cache runtimes backups logs total",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                crate::app::views::settings_modal::render_settings_search_badge(ui, p, &cat_title);
            }
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
        }

        // Card 2: Unused Version Retention & Cache Cleanup
        let prot_hint = self.t("storage.legacy.protected_hint").to_string();
        let ret_lbl = self.t("storage.legacy.jar_retention.label").to_string();
        if crate::app::views::settings_modal::settings_card_matches(
            search_q,
            &[
                &cat_title,
                &prot_hint,
                &ret_lbl,
                "cache unused version cleanup retention days clear api temp prune unused versions",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                crate::app::views::settings_modal::render_settings_search_badge(ui, p, &cat_title);
            }
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
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .checkbox(
                                &mut self.prefs.auto_cleanup_reminders,
                                "Auto junk cleanup reminder popup (>= 50 MB)",
                            )
                            .changed()
                        {
                            if self.prefs.auto_cleanup_reminders {
                                self.prefs.junk_reminder_snooze_until = 0;
                            }
                            *prefs_dirty = true;
                        }
                        let cur_days = format!("{} days", self.prefs.junk_retention_days);
                        ui.label("Retention:");
                        egui::ComboBox::from_id_salt("launcher_settings_junk_retention_combo")
                            .selected_text(cur_days)
                            .width(105.0)
                            .show_ui(ui, |ui| {
                                for days in [7u32, 14, 30, 60] {
                                    if ui
                                        .selectable_value(
                                            &mut self.prefs.junk_retention_days,
                                            days,
                                            format!("{days} days"),
                                        )
                                        .changed()
                                    {
                                        *prefs_dirty = true;
                                    }
                                }
                            });
                        let scan_junk_lbl = if self.junk_scanning {
                            "Scanning Junk..."
                        } else {
                            "Scan Junk Now"
                        };
                        if ui
                            .add_enabled(
                                !self.junk_scanning && !self.junk_cleaning,
                                Button::new(scan_junk_lbl),
                            )
                            .clicked()
                        {
                            self.start_junk_scan(true, ctx);
                        }
                    });
                });

            ui.add_space(8.0);
        }

        // Card 3: Shared Content Store & Hardlink Deduplication
        if crate::app::views::settings_modal::settings_card_matches(
            search_q,
            &[
                &cat_title,
                "shared content store hardlink deduplication canonical blobs deduplicate mods packs sha-256",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                crate::app::views::settings_modal::render_settings_search_badge(ui, p, &cat_title);
            }
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
            ui.add_space(8.0);
        }

        // Card 4: Safe Import from MCW Launcher Python
        let py_imp_title = self.t("python_import.card_title").to_string();
        let py_imp_desc = self.t("python_import.card_detail").to_string();
        let py_imp_btn = self.t("python_import.open_button").to_string();
        if crate::app::views::settings_modal::settings_card_matches(
            search_q,
            &[
                &cat_title,
                &py_imp_title,
                &py_imp_desc,
                &py_imp_btn,
                "import migrate python mcw launcher old instances settings java themes",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                crate::app::views::settings_modal::render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&py_imp_title).strong());
                    ui.label(RichText::new(&py_imp_desc).size(11.5).color(p.text_muted));
                    ui.add_space(8.0);
                    let btn = Button::new(RichText::new(&py_imp_btn).color(p.accent_text).strong())
                        .fill(p.accent);
                    if ui.add(btn).clicked() {
                        self.open_python_import_window(ctx);
                    }
                });
            ui.add_space(8.0);
        }

        // Card 5: Universal Launcher Importer (Prism / MultiMC / Modrinth App / CurseForge App / Vanilla)
        let univ_title = "Universal Launcher Importer";
        let univ_desc = "Copy instances, mods, worlds, resource packs, and shaders safely from Prism Launcher, MultiMC, Modrinth App, CurseForge App, or Vanilla .minecraft (source stays read-only).";
        let univ_btn = "Import from Other Launchers...";
        if crate::app::views::settings_modal::settings_card_matches(
            search_q,
            &[
                &cat_title,
                univ_title,
                univ_desc,
                univ_btn,
                "prism multimc modrinth curseforge vanilla import",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                crate::app::views::settings_modal::render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(univ_title).strong());
                    ui.label(RichText::new(univ_desc).size(11.5).color(p.text_muted));
                    ui.add_space(8.0);
                    let btn = Button::new(RichText::new(univ_btn).color(p.accent_text).strong())
                        .fill(p.accent);
                    if ui.add(btn).clicked() {
                        self.open_universal_import_modal(ctx);
                    }
                });
            ui.add_space(8.0);
        }
        matched
    }

    pub(crate) fn render_settings_updates_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        prefs_dirty: &mut bool,
        search_q: Option<&str>,
    ) -> usize {
        let p = self.palette;
        let upd_title = self.t("launcher_settings.update.title").to_string();
        let upd_sub = self.t("launcher_settings.update.subtitle").to_string();
        let auto_lbl = self.t("launcher_settings.update.auto_check").to_string();
        let chan_lbl = self.t("launcher_settings.update.channel").to_string();
        let chk_lbl = self.t("launcher_settings.update.check").to_string();
        if search_q.is_none() {
            ui.label(RichText::new(&upd_title).strong().size(15.0).color(p.text));
            ui.label(RichText::new(&upd_sub).size(11.5).color(p.text_muted));
            ui.add_space(10.0);
        }
        if !crate::app::views::settings_modal::settings_card_matches(
            search_q,
            &[
                "Updates",
                &upd_title,
                &upd_sub,
                &auto_lbl,
                &chan_lbl,
                &chk_lbl,
                "updates current version check automatic channel stable beta",
            ],
        ) {
            return 0;
        }
        if search_q.is_some() {
            crate::app::views::settings_modal::render_settings_search_badge(ui, p, "Updates");
        }

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
        1
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

        let title = format!("{} - v{}", self.t("update.dialog.title"), release.version);
        let notice = self.modal_notice_for("mcw_modal_update_dialog");

        let (keep_open, dismiss_notice) = super::dialogs::render_in_app_modal(
            ctx,
            "mcw_modal_update_dialog",
            &title,
            [580.0, 460.0],
            [440.0, 340.0],
            p,
            notice.as_ref(),
            |ui| {
                egui::TopBottomPanel::bottom("update_dialog_footer")
                    .frame(
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .inner_margin(egui::Margin::same(10)),
                    )
                    .show_inside(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
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
                                render_release_markdown(ui, &notes, p);
                            });
                    });
            },
        );
        if !keep_open {
            open = false;
        }
        if dismiss_notice {
            self.notice_banner = None;
        }

        self.show_update_dialog = open;
        if let Some(rel) = trigger_install {
            self.start_update_download_and_install(rel, ctx);
        }
    }

    /// Python-parity 6-step First-Run Setup Wizard (`first_run_setup_dialog.py`):
    /// Welcome -> Account -> Java -> Memory -> Graphics -> Finish.
    pub(crate) fn render_first_run_wizard(&mut self, ctx: &egui::Context) {
        if !self.show_first_run_wizard {
            return;
        }
        let p = self.palette;
        let clean_dash = |s: String| s.replace(['\u{2014}', '\u{2013}'], "-");
        let mut open = self.show_first_run_wizard;
        let mut back_clicked = false;
        let mut next_clicked = false;
        let mut finish_clicked = false;
        let mut use_defaults_clicked = false;
        let mut use_recommended_clicked = false;
        let mut rescan_java_clicked = false;
        let mut create_offline_clicked = false;
        let mut activate_uuid: Option<String> = None;
        let mut start_ms_pkce = false;
        let mut start_ms_device = false;
        let mut cancel_ms = false;
        let mut lang_changed: Option<String> = None;

        let page = self.first_run_page.min(5);
        self.first_run_page = page;

        let first_run_title = format!("{} - MCW Launcher", self.t("first_run.title"));
        let step_keys = [
            "first_run.welcome.title",
            "first_run.account.title",
            "first_run.java.title",
            "first_run.memory.title",
            "first_run.graphics.title",
            "first_run.finish.title",
        ];
        let step_title = clean_dash(self.t(step_keys[page]).to_string());
        let step_counter = clean_dash(self.lang.tr_fmt(
            "first_run.page",
            &[("current", &(page + 1).to_string()), ("total", "6")],
        ));

        let finish_lbl = clean_dash(self.t("first_run.finish").to_string());
        let next_lbl = clean_dash(self.t("first_run.next").to_string());
        let back_lbl = clean_dash(self.t("first_run.back").to_string());
        let skip_lbl = clean_dash(self.t("first_run.skip").to_string());

        let available_langs: Vec<(String, String)> = self
            .lang
            .available_languages()
            .into_iter()
            .map(|(loc, name)| (loc.to_string(), name.to_string()))
            .collect();

        let notice = self.modal_notice_for("mcw_modal_first_run_setup");
        let (keep_open, dismiss_notice) = super::dialogs::render_in_app_modal(
            ctx,
            "mcw_modal_first_run_setup",
            &first_run_title,
            [780.0, 580.0],
            [600.0, 440.0],
            p,
            notice.as_ref(),
            |ui| {
                egui::TopBottomPanel::bottom("first_run_footer")
                    .frame(
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .inner_margin(egui::Margin::same(10)),
                    )
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button(&skip_lbl).clicked() {
                                use_defaults_clicked = true;
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if page >= 5 {
                                    let finish_btn = Button::new(
                                        RichText::new(&finish_lbl).color(p.accent_text).strong(),
                                    )
                                    .fill(p.accent);
                                    if ui.add(finish_btn).clicked() {
                                        finish_clicked = true;
                                    }
                                } else {
                                    let next_btn = Button::new(
                                        RichText::new(&next_lbl).color(p.accent_text).strong(),
                                    )
                                    .fill(p.accent);
                                    if ui.add(next_btn).clicked() {
                                        next_clicked = true;
                                    }
                                }
                                if ui.add_enabled(page > 0, Button::new(&back_lbl)).clicked() {
                                    back_clicked = true;
                                }
                            });
                        });
                    });

                // Step Header + Progress
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&step_title).strong().size(18.0).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(&step_counter)
                                .monospace()
                                .size(11.5)
                                .color(p.text_muted),
                        );
                    });
                });
                ui.add_space(6.0);
                ui.add(
                    egui::ProgressBar::new((page + 1) as f32 / 6.0)
                        .desired_height(6.0)
                        .fill(p.accent),
                );
                ui.add_space(12.0);

                ScrollArea::vertical()
                        .id_salt("first_run_body_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            match page {
                                // Step 1: Welcome
                                0 => {
                                    ui.label(
                                        RichText::new(clean_dash(
                                            self.t("first_run.welcome.detail.extended").to_string(),
                                        ))
                                        .size(12.5)
                                        .color(p.text_muted),
                                    );
                                    ui.add_space(12.0);
                                    Frame::new()
                                        .fill(p.surface)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .corner_radius(CornerRadius::same(12))
                                        .inner_margin(egui::Margin::same(12))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label(self.t("first_run.language.label"));
                                                let fmt_lang_opt = |loc: &str, name: &str| {
                                                    let (_, _, pct) =
                                                        self.lang.translation_completion(loc);
                                                    if pct < 100 {
                                                        format!("{name} ({loc}) - {pct}%")
                                                    } else {
                                                        format!("{name} ({loc})")
                                                    }
                                                };
                                                let cur_lbl = available_langs
                                                    .iter()
                                                    .find(|(loc, _)| {
                                                        loc == &self.first_run_language
                                                    })
                                                    .map(|(loc, name)| fmt_lang_opt(loc, name))
                                                    .unwrap_or_else(|| {
                                                        self.first_run_language.clone()
                                                    });
                                                let prev_lang = self.first_run_language.clone();
                                                egui::ComboBox::from_id_salt("first_run_lang_combo")
                                                    .selected_text(&cur_lbl)
                                                    .width(auto_combo_width(
                                                        ui,
                                                        &cur_lbl,
                                                        210.0,
                                                        310.0,
                                                    ))
                                                    .truncate()
                                                    .show_ui(ui, |ui| {
                                                        for (loc, name) in &available_langs {
                                                            ui.selectable_value(
                                                                &mut self.first_run_language,
                                                                loc.clone(),
                                                                fmt_lang_opt(loc, name),
                                                            );
                                                        }
                                                    });
                                                if self.first_run_language != prev_lang {
                                                    lang_changed =
                                                        Some(self.first_run_language.clone());
                                                }
                                            });
                                            let (tr_cnt, tot_cnt, pct) = self
                                                .lang
                                                .translation_completion(&self.first_run_language);
                                            if pct < 100 {
                                                ui.add_space(6.0);
                                                let notice_key = if self
                                                    .first_run_language
                                                    .eq_ignore_ascii_case("vi-VN")
                                                {
                                                    "first_run.language.incomplete_notice"
                                                } else {
                                                    "first_run.language.incomplete_generic"
                                                };
                                                let msg = self.lang.tr_fmt(
                                                    notice_key,
                                                    &[
                                                        ("percent", &pct.to_string()),
                                                        ("translated", &tr_cnt.to_string()),
                                                        ("total", &tot_cnt.to_string()),
                                                    ],
                                                );
                                                Frame::new()
                                                    .fill(p.elevated)
                                                    .stroke(Stroke::new(1.0_f32, p.warning))
                                                    .corner_radius(CornerRadius::same(8))
                                                    .inner_margin(egui::Margin::symmetric(10, 6))
                                                    .show(ui, |ui| {
                                                        ui.set_width(ui.available_width());
                                                        ui.add(
                                                            egui::Label::new(
                                                                RichText::new(msg)
                                                                    .size(11.5)
                                                                    .color(p.warning),
                                                            )
                                                            .wrap(),
                                                        );
                                                    });
                                            }
                                            ui.add_space(8.0);
                                            let conc_lbl =
                                                self.t("network.concurrency.label").to_string();
                                            let auto_conc =
                                                self.t("network.concurrency.auto").to_string();
                                            let cur_conc_lbl =
                                                if self.first_run_download_concurrency == 0 {
                                                    auto_conc.clone()
                                                } else {
                                                    format!(
                                                        "{} concurrent",
                                                        self.first_run_download_concurrency
                                                    )
                                                };
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label(&conc_lbl);
                                                egui::ComboBox::from_id_salt("first_run_conc_combo")
                                                    .selected_text(&cur_conc_lbl)
                                                    .width(auto_combo_width(
                                                        ui,
                                                        &cur_conc_lbl,
                                                        170.0,
                                                        260.0,
                                                    ))
                                                    .show_ui(ui, |ui| {
                                                        ui.selectable_value(
                                                            &mut self.first_run_download_concurrency,
                                                            0,
                                                            &auto_conc,
                                                        );
                                                        for v in [2u32, 4, 6, 8, 12, 16] {
                                                            ui.selectable_value(
                                                                &mut self
                                                                    .first_run_download_concurrency,
                                                                v,
                                                                format!("{v} concurrent"),
                                                            );
                                                        }
                                                    });
                                            });
                                            ui.add_space(8.0);
                                            let auto_upd_lbl = if mcw_updater::UPDATES_ENABLED {
                                                self.t("first_run.updates.toggle").to_string()
                                            } else {
                                                format!(
                                                    "{} (currently disabled)",
                                                    self.t("first_run.updates.toggle")
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
                                    ui.add_space(8.0);
                                    let py_title =
                                        self.t("first_run.python_import.title").to_string();
                                    let py_detail =
                                        self.t("first_run.python_import.detail").to_string();
                                    let py_btn = self.t("python_import.open_button").to_string();
                                    let mut open_py = false;
                                    Frame::new()
                                        .fill(p.surface)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .corner_radius(CornerRadius::same(12))
                                        .inner_margin(egui::Margin::same(12))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.label(RichText::new(py_title).strong().color(p.text));
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(py_detail)
                                                        .size(12.0)
                                                        .color(p.text_muted),
                                                )
                                                .wrap(),
                                            );
                                            ui.add_space(6.0);
                                            if ui.button(py_btn).clicked() {
                                                open_py = true;
                                            }
                                        });
                                    if open_py {
                                        self.open_python_import_window(ctx);
                                    }
                                }
                                // Step 2: Account
                                1 => {
                                    ui.label(
                                        RichText::new(clean_dash(
                                            self.t("first_run.account.detail").to_string(),
                                        ))
                                        .size(12.5)
                                        .color(p.text_muted),
                                    );
                                    ui.add_space(10.0);

                                    // Current / Saved Accounts Card
                                    Frame::new()
                                        .fill(p.surface)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .corner_radius(CornerRadius::same(12))
                                        .inner_margin(egui::Margin::same(12))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            if let Some(acc) = self.active_account() {
                                                let acc_type = match acc.account_type {
                                                    AccountSource::Microsoft => "Microsoft",
                                                    AccountSource::Offline => "Offline",
                                                };
                                                let active_txt = clean_dash(self.lang.tr_fmt(
                                                    "first_run.account.active",
                                                    &[
                                                        ("username", &acc.username),
                                                        ("type", acc_type),
                                                    ],
                                                ));
                                                ui.label(
                                                    RichText::new(active_txt)
                                                        .strong()
                                                        .color(p.accent),
                                                );
                                                if self.accounts.len() > 1 {
                                                    ui.add_space(6.0);
                                                    for item in &self.accounts {
                                                        let t_str = match item.account_type {
                                                            AccountSource::Microsoft => "Microsoft",
                                                            AccountSource::Offline => "Offline",
                                                        };
                                                        ui.horizontal(|ui| {
                                                            ui.label(format!(
                                                                "{} ({t_str}){}",
                                                                item.username,
                                                                if item.active {
                                                                    " [Active]"
                                                                } else {
                                                                    ""
                                                                }
                                                            ));
                                                            if !item.active
                                                                && ui.button("Set Active").clicked()
                                                            {
                                                                activate_uuid =
                                                                    Some(item.uuid.clone());
                                                            }
                                                        });
                                                    }
                                                }
                                            } else {
                                                ui.label(
                                                    RichText::new(clean_dash(
                                                        self.t("first_run.account.none")
                                                            .to_string(),
                                                    ))
                                                    .color(p.text_muted),
                                                );
                                            }
                                        });

                                    ui.add_space(8.0);

                                    // Offline Account Card
                                    Frame::new()
                                        .fill(p.surface)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .corner_radius(CornerRadius::same(12))
                                        .inner_margin(egui::Margin::same(12))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.label(
                                                RichText::new(
                                                    self.t("first_run.account.offline.title"),
                                                )
                                                .strong(),
                                            );
                                            ui.add_space(6.0);
                                            ui.horizontal_wrapped(|ui| {
                                                let te = ui.add(
                                                    TextEdit::singleline(
                                                        &mut self.first_run_offline_username,
                                                    )
                                                    .hint_text("Username (3-16 chars)")
                                                    .desired_width(220.0),
                                                );
                                                let enter = te.lost_focus()
                                                    && ui
                                                        .ctx()
                                                        .input(|i| i.key_pressed(egui::Key::Enter));
                                                if ui.button("Add Offline").clicked() || enter {
                                                    create_offline_clicked = true;
                                                }
                                            });
                                            if let Some((ref msg, is_err)) =
                                                self.first_run_offline_msg
                                            {
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new(msg).size(11.5).color(if is_err {
                                                        p.danger
                                                    } else {
                                                        p.accent
                                                    }),
                                                );
                                            }
                                        });

                                    ui.add_space(8.0);

                                    // Microsoft Sign-in Card
                                    Frame::new()
                                        .fill(p.surface)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .corner_radius(CornerRadius::same(12))
                                        .inner_margin(egui::Margin::same(12))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.label(
                                                RichText::new(
                                                    self.t("first_run.account.microsoft.title"),
                                                )
                                                .strong(),
                                            );
                                            ui.add_space(6.0);
                                            ui.horizontal_wrapped(|ui| {
                                                let pkce_btn = Button::new(
                                                    RichText::new("Sign in with Browser (PKCE)")
                                                        .color(p.accent_text)
                                                        .strong(),
                                                )
                                                .fill(p.accent);
                                                if ui
                                                    .add_enabled(
                                                        !self.ms_login_in_progress,
                                                        pkce_btn,
                                                    )
                                                    .clicked()
                                                {
                                                    start_ms_pkce = true;
                                                }
                                                if ui
                                                    .add_enabled(
                                                        !self.ms_login_in_progress,
                                                        Button::new("Sign in with Device Code"),
                                                    )
                                                    .clicked()
                                                {
                                                    start_ms_device = true;
                                                }
                                                if self.ms_login_in_progress
                                                    && ui
                                                        .button(
                                                            RichText::new("Cancel").color(p.danger),
                                                        )
                                                        .clicked()
                                                {
                                                    cancel_ms = true;
                                                }
                                            });
                                            if self.ms_login_in_progress
                                                && self.ms_login_mode == Some("pkce")
                                            {
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new("Waiting for Microsoft sign-in in your browser (localhost:8400)...")
                                                        .size(11.5)
                                                        .color(p.accent),
                                                );
                                            }
                                            if let Some(ref dc) = self.ms_device_code {
                                                ui.add_space(6.0);
                                                Frame::new()
                                                    .fill(p.elevated)
                                                    .stroke(Stroke::new(1.0_f32, p.accent))
                                                    .corner_radius(CornerRadius::same(10))
                                                    .inner_margin(egui::Margin::same(10))
                                                    .show(ui, |ui| {
                                                        ui.label("Enter this code in your browser:");
                                                        ui.horizontal_wrapped(|ui| {
                                                            ui.label(
                                                                RichText::new(&dc.user_code)
                                                                    .monospace()
                                                                    .size(17.0)
                                                                    .strong()
                                                                    .color(p.accent),
                                                            );
                                                            if ui.button("Copy Code").clicked() {
                                                                ui.ctx()
                                                                    .copy_text(dc.user_code.clone());
                                                            }
                                                            if ui.button("Copy URL").clicked() {
                                                                ui.ctx().copy_text(
                                                                    dc.verification_uri.clone(),
                                                                );
                                                            }
                                                        });
                                                    });
                                            }
                                        });

                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(clean_dash(
                                            self.t("first_run.account.skip_hint").to_string(),
                                        ))
                                        .size(11.5)
                                        .color(p.text_muted),
                                    );
                                }
                                // Step 3: Java
                                2 => {
                                    ui.label(
                                        RichText::new(clean_dash(
                                            self.t("first_run.java.detail").to_string(),
                                        ))
                                        .size(12.5)
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
                                            if self.first_run_scanning {
                                                ui.horizontal(|ui| {
                                                    ui.spinner();
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "launcher_settings.first_run.scanning",
                                                        ))
                                                        .color(p.accent),
                                                    );
                                                });
                                            } else if let Some(ref rec) = self.first_run_rec {
                                                let majors = rec.java_majors();
                                                let status_txt = if rec.java_installations.is_empty()
                                                {
                                                    clean_dash(self.t("first_run.java.none").to_string())
                                                } else {
                                                    let majors_str = if majors.is_empty() {
                                                        "unknown".to_string()
                                                    } else {
                                                        majors
                                                            .iter()
                                                            .map(|m| m.to_string())
                                                            .collect::<Vec<_>>()
                                                            .join(", ")
                                                    };
                                                    clean_dash(self.lang.tr_fmt(
                                                        "first_run.java.detected",
                                                        &[
                                                            (
                                                                "count",
                                                                &rec.java_installations
                                                                    .len()
                                                                    .to_string(),
                                                            ),
                                                            ("majors", &majors_str),
                                                        ],
                                                    ))
                                                };
                                                ui.label(
                                                    RichText::new(status_txt)
                                                        .color(if rec.java_installations.is_empty() {
                                                            p.warning
                                                        } else {
                                                            p.text
                                                        }),
                                                );
                                                ui.add_space(8.0);
                                                let auto_lbl = clean_dash(
                                                    self.t("first_run.java.auto").to_string(),
                                                );
                                                let selected_lbl = if self
                                                    .first_run_java_path
                                                    .trim()
                                                    .is_empty()
                                                {
                                                    auto_lbl.clone()
                                                } else {
                                                    rec.java_installations
                                                        .iter()
                                                        .find(|j| {
                                                            j.path.display().to_string()
                                                                == self.first_run_java_path
                                                        })
                                                        .map(|j| {
                                                            format!(
                                                                "Java {} ({}) - {}",
                                                                j.major,
                                                                j.version_string,
                                                                j.path.display()
                                                            )
                                                        })
                                                        .unwrap_or_else(|| {
                                                            self.first_run_java_path.clone()
                                                        })
                                                };
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label("Default Java:");
                                                    let cw = auto_combo_width(
                                                        ui,
                                                        &selected_lbl,
                                                        260.0,
                                                        480.0,
                                                    );
                                                    egui::ComboBox::from_id_salt(
                                                        "first_run_java_combo",
                                                    )
                                                    .selected_text(&selected_lbl)
                                                    .width(cw)
                                                    .truncate()
                                                    .show_ui(ui, |ui| {
                                                        ui.selectable_value(
                                                            &mut self.first_run_java_path,
                                                            String::new(),
                                                            &auto_lbl,
                                                        );
                                                        for j in &rec.java_installations {
                                                            let p_str = j.path.display().to_string();
                                                            let disp = format!(
                                                                "Java {} ({}) - {p_str}",
                                                                j.major, j.version_string
                                                            );
                                                            ui.selectable_value(
                                                                &mut self.first_run_java_path,
                                                                p_str,
                                                                disp,
                                                            );
                                                        }
                                                    });
                                                });
                                            }
                                            ui.add_space(8.0);
                                            if ui
                                                .add_enabled(
                                                    !self.first_run_scanning,
                                                    Button::new(self.t("first_run.java.rescan")),
                                                )
                                                .clicked()
                                            {
                                                rescan_java_clicked = true;
                                            }
                                        });
                                }
                                // Step 4: Memory
                                3 => {
                                    ui.label(
                                        RichText::new(clean_dash(
                                            self.t("first_run.memory.detail").to_string(),
                                        ))
                                        .size(12.5)
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
                                            let slider_cap = self
                                                .first_run_rec
                                                .as_ref()
                                                .map(|r| r.total_memory_mb.clamp(2048, 32768))
                                                .unwrap_or(16384)
                                                .max(4096);
                                            if let Some(ref rec) = self.first_run_rec {
                                                let mem_status = self.lang.tr_fmt(
                                                    "first_run.memory.status",
                                                    &[
                                                        (
                                                            "total",
                                                            &format!("{} MB", rec.total_memory_mb),
                                                        ),
                                                        (
                                                            "available",
                                                            &format!(
                                                                "{} MB",
                                                                rec.available_memory_mb
                                                            ),
                                                        ),
                                                        (
                                                            "recommended",
                                                            &format!(
                                                                "{} MB",
                                                                rec.recommended_max_memory_mb
                                                            ),
                                                        ),
                                                    ],
                                                );
                                                ui.label(
                                                    RichText::new(mem_status)
                                                        .monospace()
                                                        .size(11.5),
                                                );
                                                ui.add_space(8.0);
                                            }
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label("Default Max RAM:");
                                                ui.add(
                                                    egui::Slider::new(
                                                        &mut self.first_run_max_ram,
                                                        512..=slider_cap,
                                                    )
                                                    .step_by(256.0)
                                                    .suffix(" MB"),
                                                );
                                            });
                                            self.first_run_min_ram = self
                                                .first_run_min_ram
                                                .min(self.first_run_max_ram)
                                                .max(256);
                                            ui.add_space(6.0);
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label("Default Min RAM (MB):");
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut self.first_run_min_ram,
                                                    )
                                                    .range(
                                                        256..=self.first_run_max_ram.max(256),
                                                    )
                                                    .speed(128.0),
                                                );
                                                if self.first_run_rec.is_some()
                                                    && ui
                                                        .button(
                                                            self.t(
                                                                "first_run.memory.use_recommended",
                                                            ),
                                                        )
                                                        .clicked()
                                                {
                                                    use_recommended_clicked = true;
                                                }
                                            });
                                        });
                                }
                                // Step 5: Graphics
                                4 => {
                                    ui.label(
                                        RichText::new(clean_dash(
                                            self.t("first_run.graphics.detail").to_string(),
                                        ))
                                        .size(12.5)
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
                                            let gpu_det = self
                                                .first_run_rec
                                                .as_ref()
                                                .map(|r| &r.gpu_detection);
                                            let has_dedicated = gpu_det
                                                .is_some_and(|g| g.supported && g.has_dedicated_gpu());
                                            if !has_dedicated {
                                                self.first_run_prefer_dedicated_gpu = false;
                                            }
                                            let gpu_toggle_lbl =
                                                self.t("gpu.preference.toggle").to_string();
                                            ui.add_enabled(
                                                has_dedicated,
                                                egui::Checkbox::new(
                                                    &mut self.first_run_prefer_dedicated_gpu,
                                                    gpu_toggle_lbl,
                                                ),
                                            );
                                            ui.add_space(6.0);
                                            if let Some(det) = gpu_det {
                                                if !det.supported {
                                                    ui.label(
                                                        RichText::new(
                                                            self.t("gpu.preference.unsupported"),
                                                        )
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                } else if det.has_dedicated_gpu() {
                                                    let names: Vec<String> = det
                                                        .dedicated_adapters()
                                                        .into_iter()
                                                        .map(|a| a.name.clone())
                                                        .collect();
                                                    let txt = self.lang.tr_fmt(
                                                        "gpu.preference.detected",
                                                        &[("adapters", &names.join(", "))],
                                                    );
                                                    ui.label(
                                                        RichText::new(txt)
                                                            .size(11.5)
                                                            .color(p.accent),
                                                    );
                                                } else {
                                                    ui.label(
                                                        RichText::new(
                                                            self.t("gpu.preference.not_detected"),
                                                        )
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                }
                                                if !det.adapters.is_empty() {
                                                    ui.add_space(6.0);
                                                    for adapter in &det.adapters {
                                                        let kind_tag = if adapter.dedicated {
                                                            "[Dedicated]"
                                                        } else {
                                                            "[Integrated]"
                                                        };
                                                        let line = if adapter.vendor.is_empty() {
                                                            format!("{} {kind_tag}", adapter.name)
                                                        } else {
                                                            format!(
                                                                "{} ({}) {kind_tag}",
                                                                adapter.name, adapter.vendor
                                                            )
                                                        };
                                                        ui.label(
                                                            RichText::new(line)
                                                                .monospace()
                                                                .size(11.0)
                                                                .color(p.text_muted),
                                                        );
                                                    }
                                                }
                                            }
                                            ui.add_space(6.0);
                                            ui.label(
                                                RichText::new(self.t("gpu.preference.notice"))
                                                    .size(11.0)
                                                    .color(p.text_muted),
                                            );
                                        });
                                }
                                // Step 6: Finish Summary
                                _ => {
                                    ui.label(
                                        RichText::new(clean_dash(
                                            self.t("first_run.finish.detail").to_string(),
                                        ))
                                        .size(12.5)
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
                                            let lang_disp = available_langs
                                                .iter()
                                                .find(|(loc, _)| loc == &self.first_run_language)
                                                .map(|(loc, name)| format!("{name} ({loc})"))
                                                .unwrap_or_else(|| {
                                                    self.first_run_language.clone()
                                                });
                                            let acct_disp = match self.active_account() {
                                                Some(a) => {
                                                    let t_str = match a.account_type {
                                                        AccountSource::Microsoft => "Microsoft",
                                                        AccountSource::Offline => "Offline",
                                                    };
                                                    format!("{} ({t_str})", a.username)
                                                }
                                                None => clean_dash(
                                                    self.t("first_run.account.none").to_string(),
                                                ),
                                            };
                                            let java_disp = if self
                                                .first_run_java_path
                                                .trim()
                                                .is_empty()
                                            {
                                                clean_dash(
                                                    self.t("first_run.java.auto").to_string(),
                                                )
                                            } else {
                                                self.first_run_java_path.clone()
                                            };
                                            let mem_disp = format!(
                                                "{} MB - {} MB",
                                                self.first_run_min_ram, self.first_run_max_ram
                                            );
                                            let gpu_disp = if self.first_run_prefer_dedicated_gpu {
                                                self.t("common.enabled").to_string()
                                            } else {
                                                self.t("common.disabled").to_string()
                                            };
                                            let upd_disp = if self.first_run_auto_updates
                                                && mcw_updater::UPDATES_ENABLED
                                            {
                                                self.t("common.enabled").to_string()
                                            } else {
                                                self.t("common.disabled").to_string()
                                            };
                                            let summary = clean_dash(self.lang.tr_fmt(
                                                "first_run.summary.extended",
                                                &[
                                                    ("language", &lang_disp),
                                                    ("account", &acct_disp),
                                                    ("java", &java_disp),
                                                    ("memory", &mem_disp),
                                                    ("gpu", &gpu_disp),
                                                    ("updates", &upd_disp),
                                                ],
                                            ));
                                            ui.label(
                                                RichText::new(summary).monospace().size(12.0),
                                            );
                                        });
                                }
                            }
                        });
            },
        );
        if !keep_open {
            open = false;
        }
        if dismiss_notice {
            self.notice_banner = None;
        }

        if let Some(loc) = lang_changed {
            self.lang.set_language(&loc);
        }
        if let Some(uuid) = activate_uuid {
            if let Some(acc) = self.accounts.iter().find(|a| a.uuid == uuid).cloned() {
                let _ = AccountStore::new(&self.paths).upsert(acc, true);
                self.reload_accounts();
            }
        }
        if create_offline_clicked {
            match OfflineAuth::authenticate(&self.first_run_offline_username) {
                Ok(auth) => {
                    let uname = auth.player_name.clone();
                    let stored = StoredAccount {
                        uuid: auth.uuid,
                        username: uname.clone(),
                        account_type: AccountSource::Offline,
                        skin_url: None,
                        active: true,
                    };
                    let _ = AccountStore::new(&self.paths).upsert(stored, true);
                    self.first_run_offline_username.clear();
                    self.reload_accounts();
                    self.first_run_offline_msg =
                        Some((format!("Signed in offline as '{uname}'."), false));
                }
                Err(err) => {
                    self.first_run_offline_msg = Some((err.to_string(), true));
                }
            }
        }
        if start_ms_pkce {
            self.start_microsoft_login(true, ctx);
        }
        if start_ms_device {
            self.start_microsoft_login(false, ctx);
        }
        if cancel_ms {
            self.cancel_microsoft_login();
        }
        if rescan_java_clicked {
            self.start_first_run_scan(ctx);
        }
        if use_recommended_clicked {
            if let Some(ref rec) = self.first_run_rec {
                self.first_run_min_ram = rec.recommended_min_memory_mb;
                self.first_run_max_ram = rec.recommended_max_memory_mb;
            }
        }
        if back_clicked {
            self.first_run_page = self.first_run_page.saturating_sub(1);
        } else if next_clicked {
            self.first_run_page = (self.first_run_page + 1).min(5);
        }
        if finish_clicked {
            let min_ram = self
                .first_run_min_ram
                .max(256)
                .min(self.first_run_max_ram.max(512));
            let max_ram = self.first_run_max_ram.max(min_ram);
            self.prefs.language = self.first_run_language.clone();
            self.prefs.auto_check_updates = self.first_run_auto_updates;
            self.prefs.discord_rpc_enabled = self.first_run_discord_rpc;
            self.prefs.download_concurrency = self.first_run_download_concurrency;
            self.prefs.default_java_path = self.first_run_java_path.trim().to_string();
            self.prefs.default_min_memory = min_ram;
            self.prefs.default_max_memory = max_ram;
            self.prefs.prefer_dedicated_gpu = self.first_run_prefer_dedicated_gpu;
            self.prefs.first_run_completed = true;
            self.lang.set_language(&self.prefs.normalized_locale());
            self.discord_rpc.configure(
                self.prefs.discord_rpc_enabled,
                self.prefs.discord_client_id.clone(),
            );
            self.save_prefs();
            if self.ms_login_in_progress {
                self.cancel_microsoft_login();
            }
            self.show_first_run_wizard = false;
            self.notice_banner = Some((
                self.t("launcher_settings.first_run.completed").to_string(),
                false,
            ));
        } else if use_defaults_clicked {
            if let Some(ref rec) = self.first_run_rec {
                self.prefs.default_min_memory = rec.recommended_min_memory_mb;
                self.prefs.default_max_memory = rec.recommended_max_memory_mb;
                self.prefs.prefer_dedicated_gpu =
                    rec.gpu_detection.supported && rec.gpu_detection.has_dedicated_gpu();
            }
            self.prefs.first_run_completed = true;
            self.save_prefs();
            if self.ms_login_in_progress {
                self.cancel_microsoft_login();
            }
            self.show_first_run_wizard = false;
            self.notice_banner = Some((
                self.t("launcher_settings.first_run.cancelled").to_string(),
                false,
            ));
        } else {
            self.show_first_run_wizard = open;
            if !open && self.ms_login_in_progress {
                self.cancel_microsoft_login();
            }
        }
    }
}

fn md_with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn render_release_code_block(ui: &mut egui::Ui, lines: &[&str], p: ThemePalette) {
    if lines.is_empty() {
        return;
    }
    Frame::new()
        .fill(p.canvas)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    RichText::new(lines.join("\n"))
                        .monospace()
                        .size(11.5)
                        .color(p.text),
                )
                .wrap(),
            );
        });
    ui.add_space(4.0);
}

fn append_inline_markdown(
    job: &mut egui::text::LayoutJob,
    mut rest: &str,
    base_size: f32,
    base_color: Color32,
    default_bold: bool,
    p: ThemePalette,
) {
    let normal_fmt = egui::TextFormat {
        font_id: egui::FontId::proportional(if default_bold {
            base_size + 0.3
        } else {
            base_size
        }),
        color: base_color,
        extra_letter_spacing: if default_bold { 0.15 } else { 0.0 },
        ..Default::default()
    };
    let bold_fmt = egui::TextFormat {
        font_id: egui::FontId::proportional(base_size + 0.4),
        color: p.text,
        extra_letter_spacing: 0.2,
        ..Default::default()
    };
    let italic_fmt = egui::TextFormat {
        font_id: egui::FontId::proportional(base_size),
        color: base_color,
        italics: true,
        ..Default::default()
    };
    let code_fmt = egui::TextFormat {
        font_id: egui::FontId::monospace((base_size - 0.5).max(10.5)),
        color: p.accent,
        background: md_with_alpha(p.elevated, 220),
        ..Default::default()
    };
    let link_fmt = egui::TextFormat {
        font_id: egui::FontId::proportional(base_size),
        color: p.accent,
        underline: Stroke::new(1.0_f32, p.accent),
        ..Default::default()
    };

    while !rest.is_empty() {
        let next = [
            rest.find('`').map(|i| (i, 0u8)),
            rest.find("**").map(|i| (i, 1u8)),
            rest.find('[').map(|i| (i, 2u8)),
            rest.find('*').map(|i| (i, 3u8)),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|(idx, kind)| (*idx, *kind));

        let Some((pos, kind)) = next else {
            job.append(rest, 0.0, normal_fmt);
            break;
        };
        if pos > 0 {
            job.append(&rest[..pos], 0.0, normal_fmt.clone());
            rest = &rest[pos..];
            continue;
        }

        match kind {
            0 => {
                if let Some(end) = rest[1..].find('`') {
                    let inner = &rest[1..1 + end];
                    if !inner.is_empty() {
                        job.append(&format!(" {inner} "), 0.0, code_fmt.clone());
                    }
                    rest = &rest[1 + end + 1..];
                    continue;
                }
            }
            1 => {
                if let Some(end) = rest[2..].find("**") {
                    let inner = &rest[2..2 + end];
                    if !inner.is_empty() {
                        job.append(inner, 0.0, bold_fmt.clone());
                        rest = &rest[2 + end + 2..];
                        continue;
                    }
                }
            }
            2 => {
                if let Some(close_bracket) = rest[1..].find("](") {
                    let after_paren = 1 + close_bracket + 2;
                    if let Some(close_paren) = rest[after_paren..].find(')') {
                        let label = &rest[1..1 + close_bracket];
                        let url = &rest[after_paren..after_paren + close_paren];
                        if !label.is_empty() && !url.is_empty() {
                            job.append(label, 0.0, link_fmt.clone());
                            rest = &rest[after_paren + close_paren + 1..];
                            continue;
                        }
                    }
                }
            }
            _ => {
                if !rest.starts_with("**") {
                    if let Some(end) = rest[1..].find('*') {
                        let inner = &rest[1..1 + end];
                        if !inner.is_empty() && !inner.starts_with(' ') {
                            job.append(inner, 0.0, italic_fmt.clone());
                            rest = &rest[1 + end + 1..];
                            continue;
                        }
                    }
                }
            }
        }

        let ch_len = rest.chars().next().map(char::len_utf8).unwrap_or(1);
        job.append(&rest[..ch_len], 0.0, normal_fmt.clone());
        rest = &rest[ch_len..];
    }
}

fn render_inline_markdown_label(
    ui: &mut egui::Ui,
    text: &str,
    base_size: f32,
    base_color: Color32,
    default_bold: bool,
    p: ThemePalette,
) {
    let mut job = egui::text::LayoutJob::default();
    append_inline_markdown(&mut job, text, base_size, base_color, default_bold, p);
    ui.add(egui::Label::new(job).wrap());
}

fn parse_numbered_list_item(trimmed: &str) -> Option<(&str, &str)> {
    let dot = trimmed.find(". ")?;
    if dot == 0 || dot > 4 {
        return None;
    }
    let prefix = &trimmed[..dot];
    if !prefix.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((prefix, &trimmed[dot + 2..]))
}

pub(crate) fn render_release_markdown(ui: &mut egui::Ui, markdown: &str, p: ThemePalette) {
    let mut in_code = false;
    let mut code_lines: Vec<&str> = Vec::new();
    let body_color = md_with_alpha(p.text, 232);

    for raw_line in markdown.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();

        if trimmed.starts_with("```") {
            if in_code {
                render_release_code_block(ui, &code_lines, p);
                code_lines.clear();
                in_code = false;
            } else {
                in_code = true;
            }
            continue;
        }
        if in_code {
            code_lines.push(line);
            continue;
        }

        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }

        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            ui.add_space(2.0);
            ui.separator();
            ui.add_space(2.0);
            continue;
        }

        if let Some(heading) = trimmed.strip_prefix("#### ") {
            ui.add_space(3.0);
            render_inline_markdown_label(ui, heading.trim(), 12.5, p.text, true, p);
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("### ") {
            ui.add_space(4.0);
            render_inline_markdown_label(ui, heading.trim(), 13.5, p.text, true, p);
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("## ") {
            ui.add_space(6.0);
            render_inline_markdown_label(ui, heading.trim(), 15.0, p.accent, true, p);
            ui.add_space(1.0);
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("# ") {
            ui.add_space(6.0);
            render_inline_markdown_label(ui, heading.trim(), 16.5, p.text, true, p);
            ui.add_space(2.0);
            continue;
        }

        if let Some(quote) = trimmed.strip_prefix('>') {
            let quote_text = quote.trim_start();
            Frame::new()
                .fill(md_with_alpha(p.accent, 14))
                .stroke(Stroke::new(1.0_f32, md_with_alpha(p.accent, 80)))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(10, 5))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    render_inline_markdown_label(ui, quote_text, 12.0, p.text_muted, false, p);
                });
            ui.add_space(2.0);
            continue;
        }

        let leading_spaces = line.chars().take_while(|c| *c == ' ').count();
        let indent_level = (leading_spaces / 2).min(4) as f32;
        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            ui.horizontal_top(|ui| {
                ui.add_space(indent_level * 12.0 + 4.0);
                ui.label(RichText::new("•").size(12.5).color(p.accent).strong());
                render_inline_markdown_label(ui, item.trim(), 12.0, body_color, false, p);
            });
            continue;
        }

        if let Some((num, item)) = parse_numbered_list_item(trimmed) {
            ui.horizontal_top(|ui| {
                ui.add_space(indent_level * 12.0 + 4.0);
                ui.label(
                    RichText::new(format!("{num}."))
                        .size(12.0)
                        .color(p.accent)
                        .strong(),
                );
                render_inline_markdown_label(ui, item.trim(), 12.0, body_color, false, p);
            });
            continue;
        }

        render_inline_markdown_label(ui, trimmed, 12.0, body_color, false, p);
    }

    if in_code && !code_lines.is_empty() {
        render_release_code_block(ui, &code_lines, p);
    }
}
