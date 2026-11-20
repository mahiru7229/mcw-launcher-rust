use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    /// Horizon-style in-app centered Launcher Settings modal overlay
    /// (left category list + search filter + scrollable tab body).
    pub(crate) fn render_launcher_settings_modal(&mut self, ctx: &egui::Context) {
        if !self.show_launcher_settings {
            return;
        }
        let p = self.palette;
        let mut open = self.show_launcher_settings;
        let mut prefs_dirty = false;
        let mut reset_defaults = false;
        let mut reload_langs = false;
        let mut scan_java = false;
        let mut discord_changed = false;
        let mut open_first_run = false;
        let trigger_storage_scan = self.launcher_settings_tab == LauncherSettingsTab::Storage
            && self.storage_summary.is_none()
            && !self.storage_scanning;
        let mut install_java_major: Option<u32> = None;
        let mut uninstall_java_exe: Option<PathBuf> = None;
        let win_title = self.t("launcher_settings.page.title").to_string();
        let nav_tabs = [
            (
                LauncherSettingsTab::General,
                self.t("settings.section.general").to_string(),
            ),
            (
                LauncherSettingsTab::Downloads,
                self.t("settings.section.downloads").to_string(),
            ),
            (
                LauncherSettingsTab::Runtime,
                self.t("settings.section.runtime").to_string(),
            ),
            (
                LauncherSettingsTab::Appearance,
                self.t("settings.section.appearance").to_string(),
            ),
            (
                LauncherSettingsTab::Storage,
                self.t("storage.settings.section").to_string(),
            ),
            (LauncherSettingsTab::Updates, "Updates".to_string()),
        ];

        let vp_ctx = ctx;
        let screen = ctx.screen_rect();
        let modal_w = (screen.width() - 80.0).clamp(420.0, 900.0);
        let modal_h = (screen.height() - 90.0).clamp(320.0, 620.0);
        let opened_at = *self.settings_open_since.get_or_insert_with(Instant::now);
        let fade_secs = self.motion_secs(0.15);
        let fade = if fade_secs <= 0.0 {
            1.0
        } else {
            (opened_at.elapsed().as_secs_f32() / fade_secs).clamp(0.0, 1.0)
        };
        if fade < 1.0 {
            ctx.request_repaint();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        let close_lbl = self.t("common.close").to_string();
        let search_hint = self.t("Search settings").to_string();
        let settings_q = self.settings_search.trim().to_lowercase();

        egui::Area::new(egui::Id::new("mcw_settings_backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    0.0,
                    Color32::from_black_alpha((150.0 * fade) as u8),
                );
                if resp.clicked() {
                    open = false;
                }
            });

        egui::Window::new(win_title.clone())
            .id(egui::Id::new("mcw_settings_modal"))
            .order(egui::Order::Foreground)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([modal_w, modal_h])
            .frame(
                Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(
                        p.canvas.r(),
                        p.canvas.g(),
                        p.canvas.b(),
                        246,
                    ))
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(18))
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.set_opacity(0.4 + 0.6 * fade);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&win_title).strong().size(20.0).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                Button::new(close_lbl.as_str())
                                    .corner_radius(CornerRadius::same(10)),
                            )
                            .clicked()
                        {
                            open = false;
                        }
                    });
                });
                ui.add_space(6.0);
                let body_h = ui.available_height();
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(190.0, body_h),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.add(
                                TextEdit::singleline(&mut self.settings_search)
                                    .hint_text(search_hint.as_str())
                                    .desired_width(ui.available_width()),
                            );
                            ui.add_space(8.0);
                            for (tab, label) in &nav_tabs {
                                if !settings_q.is_empty()
                                    && !label.to_lowercase().contains(&settings_q)
                                {
                                    continue;
                                }
                                let active = self.launcher_settings_tab == *tab;
                                let btn = Button::new(
                                    RichText::new(label).strong().size(13.0).color(
                                        if active { p.text } else { p.text_muted },
                                    ),
                                )
                                .fill(if active {
                                    p.accent.gamma_multiply(0.30)
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .stroke(if active {
                                    Stroke::new(1.0_f32, p.accent)
                                } else {
                                    Stroke::NONE
                                })
                                .min_size(Vec2::new(ui.available_width(), 36.0))
                                .corner_radius(CornerRadius::same(10));
                                if ui.add(btn).clicked() {
                                    self.launcher_settings_tab = *tab;
                                }
                                ui.add_space(4.0);
                            }
                        },
                    );
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), body_h),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ScrollArea::vertical()
                                .id_salt("launcher_settings_content_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    match self.launcher_settings_tab {
                                        LauncherSettingsTab::General => {
                                            ui.label(
                                                RichText::new(self.t("settings.section.general"))
                                                    .strong()
                                                    .size(15.0)
                                                    .color(p.text),
                                            );
                                            ui.label(
                                                RichText::new(
                                                    self.t("settings.section.general_detail"),
                                                )
                                                .size(11.5)
                                                .color(p.text_muted),
                                            );
                                            ui.add_space(10.0);

                                            // Card 1: Startup and behavior
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new(
                                                            self.t("Startup and behavior"),
                                                        )
                                                        .strong(),
                                                    );
                                                    ui.add_space(6.0);
                                                    let snap_lbl = self
                                                        .t("Show non-release versions by default")
                                                        .to_string();
                                                    if ui
                                                        .checkbox(
                                                            &mut self.prefs.show_snapshots,
                                                            snap_lbl,
                                                        )
                                                        .changed()
                                                    {
                                                        self.create_show_snapshots =
                                                            self.prefs.show_snapshots;
                                                        prefs_dirty = true;
                                                    }
                                                    let dbg_lbl = self
                                                        .t("Enable debug launch information")
                                                        .to_string();
                                                    if ui
                                                        .checkbox(
                                                            &mut self.prefs.debug_mode,
                                                            dbg_lbl,
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.prefs.minimize_on_launch,
                                                            "Minimize launcher window when Minecraft launches",
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.prefs.remember_window_size,
                                                            "Remember launcher window size across sessions",
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.prefs.prefer_dedicated_gpu,
                                                            "Prefer dedicated GPU for Java / Minecraft launches",
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                    ui.add_space(4.0);
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.label(
                                                            "Notification auto-dismiss (seconds, 0 = disabled):",
                                                        );
                                                        if ui
                                                            .add(
                                                                egui::DragValue::new(
                                                                    &mut self
                                                                        .prefs
                                                                        .notification_timeout_secs,
                                                                )
                                                                .range(0..=60)
                                                                .suffix("s"),
                                                            )
                                                            .changed()
                                                        {
                                                            prefs_dirty = true;
                                                        }
                                                    });
                                                });

                                            ui.add_space(8.0);

                                            // Card 2: Language (official Python LanguageManager — Dropdown)
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "launcher_settings.language.title",
                                                        ))
                                                        .strong(),
                                                    );
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "launcher_settings.language.detail",
                                                        ))
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                    ui.add_space(8.0);
                                                    let available: Vec<(String, String)> = self
                                                        .lang
                                                        .available_languages()
                                                        .into_iter()
                                                        .map(|(loc, name)| {
                                                            (loc.to_string(), name.to_string())
                                                        })
                                                        .collect();
                                                    let current_loc =
                                                        self.lang.current_locale().to_string();
                                                    let current_lang_label = available
                                                        .iter()
                                                        .find(|(loc, _)| loc == &current_loc)
                                                        .map(|(loc, name)| format!("{name} ({loc})"))
                                                        .unwrap_or_else(|| current_loc.clone());
                                                    let mut picked_locale: Option<String> = None;
                                                    ui.horizontal_wrapped(|ui| {
                                                        egui::ComboBox::from_id_salt(
                                                            "launcher_settings_lang_combo",
                                                        )
                                                        .selected_text(current_lang_label)
                                                        .width(240.0)
                                                        .show_ui(ui, |ui| {
                                                            for (loc, name) in &available {
                                                                let selected = loc == &current_loc;
                                                                let label =
                                                                    format!("{name} ({loc})");
                                                                if ui
                                                                    .selectable_label(
                                                                        selected, label,
                                                                    )
                                                                    .clicked()
                                                                    && !selected
                                                                {
                                                                    picked_locale =
                                                                        Some(loc.clone());
                                                                }
                                                            }
                                                        });
                                                        let reload_lbl = self
                                                            .t("launcher_settings.language.reload")
                                                            .to_string();
                                                        if ui.button(reload_lbl).clicked() {
                                                            reload_langs = true;
                                                        }
                                                    });
                                                    if let Some(loc) = picked_locale {
                                                        self.lang.set_language(&loc);
                                                        prefs_dirty = true;
                                                    }
                                                });

                                            ui.add_space(8.0);

                                            // Card 3: Discord Rich Presence
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new("Discord Rich Presence")
                                                            .strong(),
                                                    );
                                                    ui.label(
                                                        RichText::new(
                                                            "Show active instance status in Discord while browsing or playing.",
                                                        )
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                    ui.add_space(6.0);
                                                    if ui
                                                        .checkbox(
                                                            &mut self.prefs.discord_rpc_enabled,
                                                            "Enable Discord Rich Presence",
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                        discord_changed = true;
                                                    }
                                                    ui.add_space(4.0);
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.label("Application ID (optional):");
                                                        ui.add(
                                                            TextEdit::singleline(
                                                                &mut self.prefs.discord_client_id,
                                                            )
                                                            .hint_text(DEFAULT_DISCORD_CLIENT_ID)
                                                            .desired_width(210.0),
                                                        );
                                                        if ui.button("Apply").clicked() {
                                                            prefs_dirty = true;
                                                            discord_changed = true;
                                                        }
                                                    });
                                                });

                                            ui.add_space(8.0);

                                            // Card 4: First-Run Setup Wizard
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "launcher_settings.first_run.title",
                                                        ))
                                                        .strong(),
                                                    );
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "launcher_settings.first_run.detail",
                                                        ))
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                    ui.add_space(6.0);
                                                    let btn_lbl = self
                                                        .t("launcher_settings.first_run.button")
                                                        .to_string();
                                                    if ui.button(btn_lbl).clicked() {
                                                        open_first_run = true;
                                                    }
                                                });

                                            ui.add_space(8.0);

                                            // Card 5: Reset to defaults
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "launcher_settings.reset_defaults.title",
                                                        ))
                                                        .strong(),
                                                    );
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "launcher_settings.reset_defaults.detail",
                                                        ))
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                    ui.add_space(6.0);
                                                    let reset_lbl = self
                                                        .t("launcher_settings.reset")
                                                        .to_string();
                                                    let btn = Button::new(
                                                        RichText::new(reset_lbl).color(p.danger),
                                                    )
                                                    .stroke(Stroke::new(1.0_f32, p.danger));
                                                    if ui.add(btn).clicked() {
                                                        reset_defaults = true;
                                                    }
                                                });
                                        }
                                        LauncherSettingsTab::Downloads => {
                                            ui.label(
                                                RichText::new(
                                                    self.t("settings.section.downloads"),
                                                )
                                                .strong()
                                                .size(15.0)
                                                .color(p.text),
                                            );
                                            ui.label(
                                                RichText::new(
                                                    self.t("settings.section.downloads_detail"),
                                                )
                                                .size(11.5)
                                                .color(p.text_muted),
                                            );
                                            ui.add_space(10.0);

                                            // Card 1: Download concurrency
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new(self.t("Download bandwidth"))
                                                            .strong(),
                                                    );
                                                    ui.add_space(6.0);
                                                    let conc_label = self
                                                        .t("network.concurrency.label")
                                                        .to_string();
                                                    let auto_label = self
                                                        .t("network.concurrency.auto")
                                                        .to_string();
                                                    let cur_text =
                                                        if self.prefs.download_concurrency == 0 {
                                                            auto_label.clone()
                                                        } else {
                                                            format!(
                                                                "{} concurrent",
                                                                self.prefs.download_concurrency
                                                            )
                                                        };
                                                    egui::ComboBox::from_label(conc_label)
                                                        .selected_text(cur_text)
                                                        .show_ui(ui, |ui| {
                                                            if ui
                                                                .selectable_value(
                                                                    &mut self
                                                                        .prefs
                                                                        .download_concurrency,
                                                                    0,
                                                                    auto_label,
                                                                )
                                                                .changed()
                                                            {
                                                                prefs_dirty = true;
                                                            }
                                                            for v in [2u32, 4, 6, 8, 12, 16] {
                                                                if ui
                                                                    .selectable_value(
                                                                        &mut self
                                                                            .prefs
                                                                            .download_concurrency,
                                                                        v,
                                                                        format!("{v} concurrent"),
                                                                    )
                                                                    .changed()
                                                                {
                                                                    prefs_dirty = true;
                                                                }
                                                            }
                                                        });
                                                    ui.add_space(6.0);
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.label(
                                                            "Speed limit (MB/s, 0 = Unlimited):",
                                                        );
                                                        if ui
                                                            .add(
                                                                egui::DragValue::new(
                                                                    &mut self
                                                                        .prefs
                                                                        .download_limit_mbps,
                                                                )
                                                                .range(0.0..=1024.0)
                                                                .speed(0.5),
                                                            )
                                                            .changed()
                                                        {
                                                            let normalized =
                                                                mcw_net::set_global_bandwidth_limit_mbps(
                                                                    self.prefs.download_limit_mbps,
                                                                );
                                                            self.prefs.download_limit_mbps =
                                                                normalized;
                                                            prefs_dirty = true;
                                                        }
                                                    });
                                                });

                                            ui.add_space(8.0);

                                            // Card 1b: Custom CurseForge API Key
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new("CurseForge API Key (Optional Override)")
                                                            .strong(),
                                                    );
                                                    ui.label(
                                                        RichText::new(
                                                            "Leave blank to use the built-in bundled key, or save a custom key into config/curseforge.json.",
                                                        )
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                    ui.add_space(6.0);
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.add(
                                                            TextEdit::singleline(
                                                                &mut self.cf_api_key_input,
                                                            )
                                                            .password(!self.cf_api_key_revealed)
                                                            .hint_text("$2a$10$...")
                                                            .desired_width(240.0),
                                                        );
                                                        let reveal_lbl =
                                                            if self.cf_api_key_revealed {
                                                                "Hide"
                                                            } else {
                                                                "Reveal"
                                                            };
                                                        if ui.button(reveal_lbl).clicked() {
                                                            self.cf_api_key_revealed =
                                                                !self.cf_api_key_revealed;
                                                        }
                                                        if ui.button("Save Key").clicked() {
                                                            match save_custom_curseforge_api_key(
                                                                &self.paths.config_root,
                                                                &self.cf_api_key_input,
                                                            ) {
                                                                Ok(()) => {
                                                                    self.notice_banner = Some((
                                                                        "Saved custom CurseForge API key."
                                                                            .to_string(),
                                                                        false,
                                                                    ));
                                                                }
                                                                Err(e) => {
                                                                    self.notice_banner = Some((
                                                                        format!(
                                                                            "Failed to save key: {e}"
                                                                        ),
                                                                        true,
                                                                    ));
                                                                }
                                                            }
                                                        }
                                                        if !self.cf_api_key_input.trim().is_empty()
                                                            && ui.button("Use Bundled Key").clicked()
                                                        {
                                                            self.cf_api_key_input.clear();
                                                            let _ = save_custom_curseforge_api_key(
                                                                &self.paths.config_root,
                                                                "",
                                                            );
                                                            self.notice_banner = Some((
                                                                "Cleared custom CurseForge API key; reverted to default."
                                                                    .to_string(),
                                                                false,
                                                            ));
                                                        }
                                                    });
                                                });

                                            ui.add_space(8.0);

                                            // Card 2: Content browser
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new(
                                                            self.t("content.settings.title"),
                                                        )
                                                        .strong(),
                                                    );
                                                    ui.label(
                                                        RichText::new(
                                                            self.t("content.settings.detail"),
                                                        )
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                    ui.add_space(6.0);
                                                    let desc_lbl = self
                                                        .t("content.settings.show_descriptions")
                                                        .to_string();
                                                    if ui
                                                        .checkbox(
                                                            &mut self
                                                                .prefs
                                                                .show_content_descriptions,
                                                            desc_lbl,
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                    let beta_lbl = self
                                                        .t("Include Beta mod and modpack versions")
                                                        .to_string();
                                                    if ui
                                                        .checkbox(
                                                            &mut self.prefs.modrinth_include_beta,
                                                            beta_lbl,
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                    let alpha_lbl = self
                                                        .t("Include Alpha mod and modpack versions")
                                                        .to_string();
                                                    if ui
                                                        .checkbox(
                                                            &mut self.prefs.modrinth_include_alpha,
                                                            alpha_lbl,
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                });

                                            ui.add_space(8.0);

                                            // Card 3: Managed Content Failure Policies
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new(
                                                            self.t("managed_content.launcher.title"),
                                                        )
                                                        .strong(),
                                                    );
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "managed_content.launcher.detail",
                                                        ))
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                    ui.add_space(6.0);
                                                    let block_txt = self
                                                        .t("managed_content.policy.block")
                                                        .to_string();
                                                    let allow_txt = self
                                                        .t("managed_content.policy.allow")
                                                        .to_string();
                                                    let warn_txt =
                                                        "Warn in logs & continue".to_string();
                                                    let policy_label = |val: &str| match val
                                                        .trim()
                                                        .to_ascii_lowercase()
                                                        .as_str()
                                                    {
                                                        "warn" | "ask" => warn_txt.as_str(),
                                                        "allow" => allow_txt.as_str(),
                                                        _ => block_txt.as_str(),
                                                    };

                                                    let mr_lbl = self
                                                        .t("managed_content.modrinth.label")
                                                        .to_string();
                                                    let cur_mr = policy_label(
                                                        &self.prefs.modrinth_failure_policy,
                                                    );
                                                    let mr_w = auto_combo_width(
                                                        ui, cur_mr, 180.0, 280.0,
                                                    );
                                                    egui::ComboBox::from_label(mr_lbl)
                                                        .selected_text(cur_mr)
                                                        .width(mr_w)
                                                        .truncate()
                                                        .show_ui(ui, |ui| {
                                                            for (v, lbl) in [
                                                                ("block", &block_txt),
                                                                ("warn", &warn_txt),
                                                                ("allow", &allow_txt),
                                                            ] {
                                                                if ui
                                                                    .selectable_value(
                                                                        &mut self
                                                                            .prefs
                                                                            .modrinth_failure_policy,
                                                                        v.to_string(),
                                                                        lbl,
                                                                    )
                                                                    .changed()
                                                                {
                                                                    prefs_dirty = true;
                                                                }
                                                            }
                                                        });

                                                    ui.add_space(4.0);
                                                    let cf_lbl = self
                                                        .t("managed_content.curseforge.label")
                                                        .to_string();
                                                    let cur_cf = policy_label(
                                                        &self.prefs.curseforge_failure_policy,
                                                    );
                                                    let cf_w = auto_combo_width(
                                                        ui, cur_cf, 180.0, 280.0,
                                                    );
                                                    egui::ComboBox::from_label(cf_lbl)
                                                        .selected_text(cur_cf)
                                                        .width(cf_w)
                                                        .truncate()
                                                        .show_ui(ui, |ui| {
                                                            for (v, lbl) in [
                                                                ("block", &block_txt),
                                                                ("warn", &warn_txt),
                                                                ("allow", &allow_txt),
                                                            ] {
                                                                if ui
                                                                    .selectable_value(
                                                                        &mut self
                                                                            .prefs
                                                                            .curseforge_failure_policy,
                                                                        v.to_string(),
                                                                        lbl,
                                                                    )
                                                                    .changed()
                                                                {
                                                                    prefs_dirty = true;
                                                                }
                                                            }
                                                        });
                                                });
                                        }
                                        LauncherSettingsTab::Runtime => {
                                            ui.label(
                                                RichText::new(self.t("settings.section.runtime"))
                                                    .strong()
                                                    .size(15.0)
                                                    .color(p.text),
                                            );
                                            ui.label(
                                                RichText::new(
                                                    self.t("settings.section.runtime_detail"),
                                                )
                                                .size(11.5)
                                                .color(p.text_muted),
                                            );
                                            ui.add_space(10.0);

                                            // Card 1: Java runtimes
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new(
                                                            self.t("launcher_settings.java.title"),
                                                        )
                                                        .strong(),
                                                    );
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "launcher_settings.java.description",
                                                        ))
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                    ui.add_space(6.0);
                                                    if self.java_scanning {
                                                        ui.horizontal(|ui| {
                                                            ui.spinner();
                                                            ui.label(
                                                                RichText::new(
                                                                    self.t("java.scan.progress"),
                                                                )
                                                                .color(p.accent),
                                                            );
                                                        });
                                                    } else if !self.java_diagnostics.is_empty() {
                                                        for diag in &self.java_diagnostics {
                                                            let vendor = if diag.vendor.is_empty() {
                                                                "Java".to_string()
                                                            } else {
                                                                diag.vendor.clone()
                                                            };
                                                            let arch =
                                                                if diag.architecture.is_empty() {
                                                                    format!("{}-bit", diag.bitness)
                                                                } else {
                                                                    format!(
                                                                        "{} ({}-bit)",
                                                                        diag.architecture,
                                                                        diag.bitness
                                                                    )
                                                                };
                                                            ui.horizontal_wrapped(|ui| {
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "• Java {} ({}) · {} · {} · [{}] — {}",
                                                                        diag.major_version,
                                                                        diag.version_string,
                                                                        vendor,
                                                                        arch,
                                                                        diag.source,
                                                                        diag.executable.display()
                                                                    ))
                                                                    .monospace()
                                                                    .size(11.0)
                                                                    .color(if diag.valid {
                                                                        p.text_muted
                                                                    } else {
                                                                        p.warning
                                                                    }),
                                                                );
                                                                if ui
                                                                    .small_button("Open Folder")
                                                                    .clicked()
                                                                {
                                                                    let root = JavaDiagnosticsManager::java_root_dir(
                                                                        &diag.executable,
                                                                    );
                                                                    open_external(
                                                                        &root.display().to_string(),
                                                                    );
                                                                }
                                                                if diag.is_managed
                                                                    && ui
                                                                        .small_button(
                                                                            RichText::new(
                                                                                "Uninstall",
                                                                            )
                                                                            .color(p.danger),
                                                                        )
                                                                        .clicked()
                                                                {
                                                                    uninstall_java_exe = Some(
                                                                        diag.executable.clone(),
                                                                    );
                                                                }
                                                            });
                                                        }
                                                    } else if self.java_installations.is_empty() {
                                                        ui.label(
                                                            RichText::new(self.t(
                                                                "launcher_settings.java.none",
                                                            ))
                                                            .color(p.warning),
                                                        );
                                                    } else {
                                                        for j in &self.java_installations {
                                                            ui.label(
                                                                RichText::new(format!(
                                                                    "• Java {} ({}) — {}",
                                                                    j.major,
                                                                    j.version_string,
                                                                    j.path.display()
                                                                ))
                                                                .monospace()
                                                                .size(11.0)
                                                                .color(p.text_muted),
                                                            );
                                                        }
                                                    }
                                                    let recovery_cnt =
                                                        JavaRecoveryDiagnostics::snapshot().len();
                                                    if recovery_cnt > 0 {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "Java auto-selection events recorded this session: {recovery_cnt}"
                                                            ))
                                                            .size(10.5)
                                                            .color(p.text_muted),
                                                        );
                                                    }
                                                    ui.add_space(6.0);
                                                    ui.horizontal_wrapped(|ui| {
                                                        let scan_lbl = self
                                                            .t("Scan Java installations")
                                                            .to_string();
                                                        if ui.button(scan_lbl).clicked() {
                                                            scan_java = true;
                                                        }
                                                        for major in [8u32, 17, 21, 25] {
                                                            let busy = self
                                                                .installing_java_majors
                                                                .contains(&major);
                                                            let lbl = if busy {
                                                                format!("Installing Java {major}...")
                                                            } else {
                                                                format!(
                                                                    "Install Temurin Java {major}"
                                                                )
                                                            };
                                                            if ui
                                                                .add_enabled(
                                                                    !busy,
                                                                    Button::new(lbl),
                                                                )
                                                                .clicked()
                                                            {
                                                                install_java_major = Some(major);
                                                            }
                                                        }
                                                    });
                                                });

                                            ui.add_space(8.0);

                                            // Card 2: Default settings for new instances
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "instance_defaults.launcher.title",
                                                        ))
                                                        .strong(),
                                                    );
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "instance_defaults.launcher.description",
                                                        ))
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                    ui.add_space(6.0);
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.label("Min RAM (MB):");
                                                        if ui
                                                            .add(
                                                                egui::DragValue::new(
                                                                    &mut self
                                                                        .prefs
                                                                        .default_min_memory,
                                                                )
                                                                .range(256..=32768),
                                                            )
                                                            .changed()
                                                        {
                                                            prefs_dirty = true;
                                                        }
                                                        ui.label("Max RAM (MB):");
                                                        if ui
                                                            .add(
                                                                egui::DragValue::new(
                                                                    &mut self
                                                                        .prefs
                                                                        .default_max_memory,
                                                                )
                                                                .range(512..=65536),
                                                            )
                                                            .changed()
                                                        {
                                                            prefs_dirty = true;
                                                        }
                                                    });
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.label("Width:");
                                                        if ui
                                                            .add(
                                                                egui::DragValue::new(
                                                                    &mut self.prefs.default_width,
                                                                )
                                                                .range(320..=7680),
                                                            )
                                                            .changed()
                                                        {
                                                            prefs_dirty = true;
                                                        }
                                                        ui.label("Height:");
                                                        if ui
                                                            .add(
                                                                egui::DragValue::new(
                                                                    &mut self.prefs.default_height,
                                                                )
                                                                .range(240..=4320),
                                                            )
                                                            .changed()
                                                        {
                                                            prefs_dirty = true;
                                                        }
                                                        if ui
                                                            .checkbox(
                                                                &mut self.prefs.default_fullscreen,
                                                                "Fullscreen",
                                                            )
                                                            .changed()
                                                        {
                                                            prefs_dirty = true;
                                                        }
                                                    });
                                                });

                                            ui.add_space(8.0);

                                            // Card 3: Forge/NeoForge Preflight Failure Policy
                                            Frame::new()
                                                .fill(p.surface)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "forge_preflight.launcher.title",
                                                        ))
                                                        .strong(),
                                                    );
                                                    ui.label(
                                                        RichText::new(self.t(
                                                            "forge_preflight.launcher.detail",
                                                        ))
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                    ui.add_space(6.0);
                                                    let blk_lbl = self
                                                        .t("forge_preflight.policy.block")
                                                        .to_string();
                                                    let alw_lbl = self
                                                        .t("forge_preflight.policy.allow")
                                                        .to_string();
                                                    let wrn_lbl =
                                                        "Warn in logs & continue".to_string();
                                                    let cur_fp = match self
                                                        .prefs
                                                        .forge_preflight_failure_policy
                                                        .trim()
                                                        .to_ascii_lowercase()
                                                        .as_str()
                                                    {
                                                        "block" => blk_lbl.as_str(),
                                                        "allow" => alw_lbl.as_str(),
                                                        _ => wrn_lbl.as_str(),
                                                    };
                                                    let fp_w =
                                                        auto_combo_width(ui, cur_fp, 210.0, 320.0);
                                                    egui::ComboBox::from_label(
                                                        self.t("instance_defaults.policy.forge"),
                                                    )
                                                    .selected_text(cur_fp)
                                                    .width(fp_w)
                                                    .truncate()
                                                    .show_ui(ui, |ui| {
                                                        for (v, lbl) in [
                                                            ("warn", &wrn_lbl),
                                                            ("block", &blk_lbl),
                                                            ("allow", &alw_lbl),
                                                        ] {
                                                            if ui
                                                                .selectable_value(
                                                                    &mut self
                                                                        .prefs
                                                                        .forge_preflight_failure_policy,
                                                                    v.to_string(),
                                                                    lbl,
                                                                )
                                                                .changed()
                                                            {
                                                                prefs_dirty = true;
                                                            }
                                                        }
                                                    });
                                                });
                                        }
                                        LauncherSettingsTab::Appearance => {
                                            self.render_settings_appearance_tab(
                                                ui,
                                                vp_ctx,
                                                &mut prefs_dirty,
                                            );
                                        }
                                        LauncherSettingsTab::Storage => {
                                            self.render_settings_storage_tab(
                                                ui,
                                                vp_ctx,
                                                &mut prefs_dirty,
                                            );
                                        }
                                        LauncherSettingsTab::Updates => {
                                            self.render_settings_updates_tab(
                                                ui,
                                                vp_ctx,
                                                &mut prefs_dirty,
                                            );
                                        }
                                    }
                                });
                        },
                    );
                });
            });

        self.show_launcher_settings = open;
        if reload_langs {
            self.lang.reload(&self.paths);
            self.notice_banner = Some(("Reloaded language packs.".to_string(), false));
        }
        if scan_java {
            self.reload_java();
            self.start_java_diagnostics_scan(ctx);
            self.notice_banner = Some((
                format!(
                    "Detected {} Java runtime(s).",
                    self.java_installations.len()
                ),
                false,
            ));
        }
        if let Some(exe) = uninstall_java_exe {
            self.uninstall_managed_java(exe, ctx);
        }
        if discord_changed {
            self.discord_rpc.configure(
                self.prefs.discord_rpc_enabled,
                self.prefs.discord_client_id.clone(),
            );
        }
        if open_first_run {
            self.open_first_run_wizard(ctx);
        }
        if trigger_storage_scan {
            self.start_storage_scan(ctx);
        }
        if let Some(major) = install_java_major {
            self.start_java_install(major, ctx);
        }
        if reset_defaults {
            self.prefs = LauncherPrefs::default();
            self.custom_accent_input = self.prefs.custom_accent_hex.clone();
            self.apply_current_theme(ctx);
            self.lang.set_language(&self.prefs.normalized_locale());
            self.create_show_snapshots = self.prefs.show_snapshots;
            self.discord_rpc.configure(
                self.prefs.discord_rpc_enabled,
                self.prefs.discord_client_id.clone(),
            );
            self.save_prefs();
            mcw_net::set_global_bandwidth_limit_mbps(self.prefs.download_limit_mbps);
            self.notice_banner =
                Some(("Launcher settings restored to defaults.".to_string(), false));
        } else if prefs_dirty {
            self.save_prefs();
        }
    }
}
