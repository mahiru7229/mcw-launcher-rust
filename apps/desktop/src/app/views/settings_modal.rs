use crate::app::platform::*;
use crate::app::views::quick_search::matches_quick_query;
use crate::app::*;

pub(crate) fn settings_card_matches(search_q: Option<&str>, fields: &[&str]) -> bool {
    let Some(q) = search_q else {
        return true;
    };
    matches_quick_query(q, fields)
}

pub(crate) fn render_settings_search_badge(ui: &mut egui::Ui, p: ThemePalette, category: &str) {
    ui.label(
        RichText::new(category.to_ascii_uppercase())
            .strong()
            .monospace()
            .size(10.0)
            .color(p.accent),
    );
}

impl McwApp {
    /// Horizon-style in-app centered Launcher Settings modal overlay
    /// (left category list + cross-category right-pane search + scrollable body).
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
        let mut open_java_manager = false;
        let mut check_latest_java = false;
        let mut discord_changed = false;
        let mut open_first_run = false;
        let trigger_storage_scan = (self.launcher_settings_tab == LauncherSettingsTab::Storage
            || !self.settings_search.trim().is_empty())
            && self.storage_summary.is_none()
            && !self.storage_scanning;
        let trigger_latest_java_check = (self.launcher_settings_tab
            == LauncherSettingsTab::Runtime
            || !self.settings_search.trim().is_empty())
            && self.latest_java_major.is_none()
            && !self.latest_java_checking
            && self.latest_java_error.is_none();
        let mut install_java_major: Option<u32> = None;
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
        let modal_notice = self.modal_notice_for("mcw_modal_launcher_settings");
        let mut dismiss_notice = false;

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
                if let Some((ref msg, is_err)) = modal_notice {
                    if super::dialogs::render_notice_banner_box(ui, p, msg, is_err) {
                        dismiss_notice = true;
                    }
                    ui.add_space(6.0);
                }
                let body_h = ui.available_height();
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(190.0, body_h),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.horizontal(|ui| {
                                let w = if self.settings_search.is_empty() {
                                    ui.available_width()
                                } else {
                                    (ui.available_width() - 28.0).max(80.0)
                                };
                                ui.add(
                                    TextEdit::singleline(&mut self.settings_search)
                                        .hint_text(search_hint.as_str())
                                        .desired_width(w),
                                );
                                if !self.settings_search.is_empty()
                                    && ui.small_button("×").on_hover_text("Clear search").clicked()
                                {
                                    self.settings_search.clear();
                                }
                            });
                            ui.add_space(8.0);
                            for (tab, label) in &nav_tabs {
                                let active =
                                    settings_q.is_empty() && self.launcher_settings_tab == *tab;
                                let btn = Button::new(
                                    RichText::new(label).strong().size(13.0).color(if active {
                                        p.text
                                    } else {
                                        p.text_muted
                                    }),
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
                                    self.settings_search.clear();
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
                                    if settings_q.is_empty() {
                                        match self.launcher_settings_tab {
                                            LauncherSettingsTab::General => {
                                                self.render_settings_general_tab(
                                                    ui,
                                                    None,
                                                    &mut prefs_dirty,
                                                    &mut reload_langs,
                                                    &mut discord_changed,
                                                    &mut open_first_run,
                                                    &mut reset_defaults,
                                                );
                                            }
                                            LauncherSettingsTab::Downloads => {
                                                self.render_settings_downloads_tab(
                                                    ui,
                                                    None,
                                                    &mut prefs_dirty,
                                                );
                                            }
                                            LauncherSettingsTab::Runtime => {
                                                self.render_settings_runtime_tab(
                                                    ui,
                                                    None,
                                                    &mut prefs_dirty,
                                                    &mut scan_java,
                                                    &mut open_java_manager,
                                                    &mut check_latest_java,
                                                    &mut install_java_major,
                                                );
                                            }
                                            LauncherSettingsTab::Appearance => {
                                                self.render_settings_appearance_tab(
                                                    ui,
                                                    vp_ctx,
                                                    &mut prefs_dirty,
                                                    None,
                                                );
                                            }
                                            LauncherSettingsTab::Storage => {
                                                self.render_settings_storage_tab(
                                                    ui,
                                                    vp_ctx,
                                                    &mut prefs_dirty,
                                                    None,
                                                );
                                            }
                                            LauncherSettingsTab::Updates => {
                                                self.render_settings_updates_tab(
                                                    ui,
                                                    vp_ctx,
                                                    &mut prefs_dirty,
                                                    None,
                                                );
                                            }
                                        }
                                    } else {
                                        let q = Some(settings_q.as_str());
                                        let mut matched = 0usize;
                                        matched += self.render_settings_general_tab(
                                            ui,
                                            q,
                                            &mut prefs_dirty,
                                            &mut reload_langs,
                                            &mut discord_changed,
                                            &mut open_first_run,
                                            &mut reset_defaults,
                                        );
                                        matched += self.render_settings_downloads_tab(
                                            ui,
                                            q,
                                            &mut prefs_dirty,
                                        );
                                        matched += self.render_settings_runtime_tab(
                                            ui,
                                            q,
                                            &mut prefs_dirty,
                                            &mut scan_java,
                                            &mut open_java_manager,
                                            &mut check_latest_java,
                                            &mut install_java_major,
                                        );
                                        matched += self.render_settings_appearance_tab(
                                            ui,
                                            vp_ctx,
                                            &mut prefs_dirty,
                                            q,
                                        );
                                        matched += self.render_settings_storage_tab(
                                            ui,
                                            vp_ctx,
                                            &mut prefs_dirty,
                                            q,
                                        );
                                        matched += self.render_settings_updates_tab(
                                            ui,
                                            vp_ctx,
                                            &mut prefs_dirty,
                                            q,
                                        );
                                        if matched == 0 {
                                            ui.add_space(24.0);
                                            ui.vertical_centered(|ui| {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "No settings matched '{}'.",
                                                        self.settings_search.trim()
                                                    ))
                                                    .color(p.text_muted),
                                                );
                                                ui.add_space(6.0);
                                                if ui.button("Clear search").clicked() {
                                                    self.settings_search.clear();
                                                }
                                            });
                                        }
                                    }
                                });
                        },
                    );
                });
            });

        self.show_launcher_settings = open;
        if dismiss_notice {
            self.notice_banner = None;
        }
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
        if open_java_manager {
            self.open_java_manager_window(ctx);
        }
        if check_latest_java || trigger_latest_java_check {
            self.start_check_latest_java_release(ctx);
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
            self.pending_ui_scale_pct = 100.0;
            self.custom_accent_input = self.prefs.custom_accent_hex.clone();
            self.apply_current_theme(ctx);
            self.apply_typography_and_scale(ctx);
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

    #[allow(clippy::too_many_arguments)]
    fn render_settings_general_tab(
        &mut self,
        ui: &mut egui::Ui,
        search_q: Option<&str>,
        prefs_dirty: &mut bool,
        reload_langs: &mut bool,
        discord_changed: &mut bool,
        open_first_run: &mut bool,
        reset_defaults: &mut bool,
    ) -> usize {
        let p = self.palette;
        let cat_title = self.t("settings.section.general").to_string();
        let cat_detail = self.t("settings.section.general_detail").to_string();
        if search_q.is_none() {
            ui.label(RichText::new(&cat_title).strong().size(15.0).color(p.text));
            ui.label(RichText::new(&cat_detail).size(11.5).color(p.text_muted));
            ui.add_space(10.0);
        }
        let mut matched = 0usize;

        // Card 1: Startup and behavior
        let startup_title = self.t("Startup and behavior").to_string();
        let snap_lbl = self.t("Show non-release versions by default").to_string();
        let dbg_lbl = self.t("Enable debug launch information").to_string();
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                &startup_title,
                &snap_lbl,
                &dbg_lbl,
                "minimize launcher window remember size dedicated gpu notification auto-dismiss timeout seconds",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&startup_title).strong());
                    ui.add_space(6.0);
                    if ui.checkbox(&mut self.prefs.show_snapshots, &snap_lbl).changed() {
                        self.create_show_snapshots = self.prefs.show_snapshots;
                        *prefs_dirty = true;
                    }
                    if ui.checkbox(&mut self.prefs.debug_mode, &dbg_lbl).changed() {
                        *prefs_dirty = true;
                    }
                    if ui
                        .checkbox(
                            &mut self.prefs.minimize_on_launch,
                            "Minimize launcher window when Minecraft launches",
                        )
                        .changed()
                    {
                        *prefs_dirty = true;
                    }
                    if ui
                        .checkbox(
                            &mut self.prefs.remember_window_size,
                            "Remember launcher window size across sessions",
                        )
                        .changed()
                    {
                        *prefs_dirty = true;
                    }
                    if ui
                        .checkbox(
                            &mut self.prefs.prefer_dedicated_gpu,
                            "Prefer dedicated GPU for Java / Minecraft launches",
                        )
                        .changed()
                    {
                        *prefs_dirty = true;
                    }
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Notification auto-dismiss (seconds, 0 = disabled):");
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.prefs.notification_timeout_secs)
                                    .range(0..=60)
                                    .suffix("s"),
                            )
                            .changed()
                        {
                            *prefs_dirty = true;
                        }
                    });
                });
            ui.add_space(8.0);
        }

        // Card 2: Language (official Python LanguageManager - Dropdown)
        let lang_title = self.t("launcher_settings.language.title").to_string();
        let lang_detail = self.t("launcher_settings.language.detail").to_string();
        let reload_lbl = self.t("launcher_settings.language.reload").to_string();
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                &lang_title,
                &lang_detail,
                &reload_lbl,
                "language locale english vietnamese reload language packs",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&lang_title).strong());
                    ui.label(RichText::new(&lang_detail).size(11.5).color(p.text_muted));
                    ui.add_space(8.0);
                    let available: Vec<(String, String)> = self
                        .lang
                        .available_languages()
                        .into_iter()
                        .map(|(loc, name)| (loc.to_string(), name.to_string()))
                        .collect();
                    let current_loc = self.lang.current_locale().to_string();
                    let fmt_lang_opt = |loc: &str, name: &str| {
                        let (_, _, pct) = self.lang.translation_completion(loc);
                        if pct < 100 {
                            format!("{name} ({loc}) - {pct}%")
                        } else {
                            format!("{name} ({loc})")
                        }
                    };
                    let current_lang_label = available
                        .iter()
                        .find(|(loc, _)| loc == &current_loc)
                        .map(|(loc, name)| fmt_lang_opt(loc, name))
                        .unwrap_or_else(|| current_loc.clone());
                    let mut picked_locale: Option<String> = None;
                    ui.horizontal_wrapped(|ui| {
                        egui::ComboBox::from_id_salt("launcher_settings_lang_combo")
                            .selected_text(current_lang_label)
                            .width(260.0)
                            .show_ui(ui, |ui| {
                                for (loc, name) in &available {
                                    let selected = loc == &current_loc;
                                    let label = fmt_lang_opt(loc, name);
                                    if ui.selectable_label(selected, label).clicked() && !selected {
                                        picked_locale = Some(loc.clone());
                                    }
                                }
                            });
                        if ui.button(&reload_lbl).clicked() {
                            *reload_langs = true;
                        }
                    });
                    if let Some(loc) = picked_locale {
                        self.lang.set_language(&loc);
                        *prefs_dirty = true;
                    }
                    let active_loc = self.lang.current_locale().to_string();
                    let (tr_cnt, tot_cnt, pct) = self.lang.translation_completion(&active_loc);
                    if pct < 100 {
                        ui.add_space(6.0);
                        let notice_key = if active_loc.eq_ignore_ascii_case("vi-VN") {
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
                        ui.add(
                            egui::Label::new(RichText::new(msg).size(11.0).color(p.warning)).wrap(),
                        );
                    }
                });
            ui.add_space(8.0);
        }

        // Card 3: Discord Rich Presence
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                "discord rich presence enable application id client status",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Discord Rich Presence").strong());
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
                        *prefs_dirty = true;
                        *discord_changed = true;
                    }
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Application ID (optional):");
                        ui.add(
                            TextEdit::singleline(&mut self.prefs.discord_client_id)
                                .hint_text(DEFAULT_DISCORD_CLIENT_ID)
                                .desired_width(210.0),
                        );
                        if ui.button("Apply").clicked() {
                            *prefs_dirty = true;
                            *discord_changed = true;
                        }
                    });
                });
            ui.add_space(8.0);
        }

        // Card 4: First-Run Setup Wizard
        let fr_title = self.t("launcher_settings.first_run.title").to_string();
        let fr_detail = self.t("launcher_settings.first_run.detail").to_string();
        let fr_btn = self.t("launcher_settings.first_run.button").to_string();
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                &fr_title,
                &fr_detail,
                &fr_btn,
                "first run setup wizard initial recommendations",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&fr_title).strong());
                    ui.label(RichText::new(&fr_detail).size(11.5).color(p.text_muted));
                    ui.add_space(6.0);
                    if ui.button(&fr_btn).clicked() {
                        *open_first_run = true;
                    }
                });
            ui.add_space(8.0);
        }

        // Card 5: Reset to defaults
        let rst_title = self.t("launcher_settings.reset_defaults.title").to_string();
        let rst_detail = self
            .t("launcher_settings.reset_defaults.detail")
            .to_string();
        let rst_lbl = self.t("launcher_settings.reset").to_string();
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                &rst_title,
                &rst_detail,
                &rst_lbl,
                "reset defaults restore factory settings",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&rst_title).strong());
                    ui.label(RichText::new(&rst_detail).size(11.5).color(p.text_muted));
                    ui.add_space(6.0);
                    let btn = Button::new(RichText::new(&rst_lbl).color(p.danger))
                        .stroke(Stroke::new(1.0_f32, p.danger));
                    if ui.add(btn).clicked() {
                        *reset_defaults = true;
                    }
                });
            ui.add_space(8.0);
        }

        matched
    }

    fn render_settings_downloads_tab(
        &mut self,
        ui: &mut egui::Ui,
        search_q: Option<&str>,
        prefs_dirty: &mut bool,
    ) -> usize {
        let p = self.palette;
        let cat_title = self.t("settings.section.downloads").to_string();
        let cat_detail = self.t("settings.section.downloads_detail").to_string();
        if search_q.is_none() {
            ui.label(RichText::new(&cat_title).strong().size(15.0).color(p.text));
            ui.label(RichText::new(&cat_detail).size(11.5).color(p.text_muted));
            ui.add_space(10.0);
        }
        let mut matched = 0usize;

        // Card 1: Download concurrency
        let bw_title = self.t("Download bandwidth").to_string();
        let conc_label = self.t("network.concurrency.label").to_string();
        let auto_label = self.t("network.concurrency.auto").to_string();
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                &bw_title,
                &conc_label,
                &auto_label,
                "download concurrency speed limit mbps bandwidth network unlimited",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&bw_title).strong());
                    ui.add_space(6.0);
                    let cur_text = if self.prefs.download_concurrency == 0 {
                        auto_label.clone()
                    } else {
                        format!("{} concurrent", self.prefs.download_concurrency)
                    };
                    egui::ComboBox::from_label(&conc_label)
                        .selected_text(cur_text)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(
                                    &mut self.prefs.download_concurrency,
                                    0,
                                    &auto_label,
                                )
                                .changed()
                            {
                                *prefs_dirty = true;
                            }
                            for v in [2u32, 4, 6, 8, 12, 16] {
                                if ui
                                    .selectable_value(
                                        &mut self.prefs.download_concurrency,
                                        v,
                                        format!("{v} concurrent"),
                                    )
                                    .changed()
                                {
                                    *prefs_dirty = true;
                                }
                            }
                        });
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Speed limit (MB/s, 0 = Unlimited):");
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.prefs.download_limit_mbps)
                                    .range(0.0..=1024.0)
                                    .speed(0.5),
                            )
                            .changed()
                        {
                            let normalized = mcw_net::set_global_bandwidth_limit_mbps(
                                self.prefs.download_limit_mbps,
                            );
                            self.prefs.download_limit_mbps = normalized;
                            *prefs_dirty = true;
                        }
                    });
                    ui.add_space(6.0);
                    let seg_cur = match self.prefs.download_segments_per_file {
                        1 => "Off (1 stream)".to_string(),
                        4 => "4 streams".to_string(),
                        8 => "8 streams".to_string(),
                        _ => "6 streams (Default)".to_string(),
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Multi-segment Range streams (files >= 4 MB):");
                        egui::ComboBox::from_id_salt("launcher_settings_download_segments_combo")
                            .selected_text(seg_cur)
                            .width(190.0)
                            .show_ui(ui, |ui| {
                                for (val, label) in [
                                    (1usize, "Off (1 stream)"),
                                    (4usize, "4 streams"),
                                    (6usize, "6 streams (Default)"),
                                    (8usize, "8 streams"),
                                ] {
                                    if ui
                                        .selectable_value(
                                            &mut self.prefs.download_segments_per_file,
                                            val,
                                            label,
                                        )
                                        .changed()
                                    {
                                        mcw_net::set_global_segments_per_file(val);
                                        *prefs_dirty = true;
                                    }
                                }
                            });
                    });
                });
            ui.add_space(8.0);
        }

        // Card 1b: Custom CurseForge API Key
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                "curseforge api key optional override bundled key config save reveal",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("CurseForge API Key (Optional Override)").strong());
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
                            TextEdit::singleline(&mut self.cf_api_key_input)
                                .password(!self.cf_api_key_revealed)
                                .hint_text("$2a$10$...")
                                .desired_width(240.0),
                        );
                        let reveal_lbl = if self.cf_api_key_revealed {
                            "Hide"
                        } else {
                            "Reveal"
                        };
                        if ui.button(reveal_lbl).clicked() {
                            self.cf_api_key_revealed = !self.cf_api_key_revealed;
                        }
                        if ui.button("Save Key").clicked() {
                            match save_custom_curseforge_api_key(
                                &self.paths.config_root,
                                &self.cf_api_key_input,
                            ) {
                                Ok(()) => {
                                    self.notice_banner = Some((
                                        "Saved custom CurseForge API key.".to_string(),
                                        false,
                                    ));
                                }
                                Err(e) => {
                                    self.notice_banner =
                                        Some((format!("Failed to save key: {e}"), true));
                                }
                            }
                        }
                        if !self.cf_api_key_input.trim().is_empty()
                            && ui.button("Use Bundled Key").clicked()
                        {
                            self.cf_api_key_input.clear();
                            let _ = save_custom_curseforge_api_key(&self.paths.config_root, "");
                            self.notice_banner = Some((
                                "Cleared custom CurseForge API key; reverted to default."
                                    .to_string(),
                                false,
                            ));
                        }
                    });
                });
            ui.add_space(8.0);
        }

        // Card 2: Content browser
        let cb_title = self.t("content.settings.title").to_string();
        let cb_detail = self.t("content.settings.detail").to_string();
        let desc_lbl = self.t("content.settings.show_descriptions").to_string();
        let beta_lbl = self.t("Include Beta mod and modpack versions").to_string();
        let alpha_lbl = self.t("Include Alpha mod and modpack versions").to_string();
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                &cb_title,
                &cb_detail,
                &desc_lbl,
                &beta_lbl,
                &alpha_lbl,
                "content browser modrinth curseforge beta alpha descriptions mods modpacks",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&cb_title).strong());
                    ui.label(RichText::new(&cb_detail).size(11.5).color(p.text_muted));
                    ui.add_space(6.0);
                    if ui
                        .checkbox(&mut self.prefs.show_content_descriptions, &desc_lbl)
                        .changed()
                    {
                        *prefs_dirty = true;
                    }
                    if ui
                        .checkbox(&mut self.prefs.modrinth_include_beta, &beta_lbl)
                        .changed()
                    {
                        *prefs_dirty = true;
                    }
                    if ui
                        .checkbox(&mut self.prefs.modrinth_include_alpha, &alpha_lbl)
                        .changed()
                    {
                        *prefs_dirty = true;
                    }
                });
            ui.add_space(8.0);
        }

        // Card 3: Managed Content Failure Policies
        let mc_title = self.t("managed_content.launcher.title").to_string();
        let mc_detail = self.t("managed_content.launcher.detail").to_string();
        let mr_lbl = self.t("managed_content.modrinth.label").to_string();
        let cf_lbl = self.t("managed_content.curseforge.label").to_string();
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                &mc_title,
                &mc_detail,
                &mr_lbl,
                &cf_lbl,
                "managed content failure policies modrinth curseforge block allow warn",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&mc_title).strong());
                    ui.label(RichText::new(&mc_detail).size(11.5).color(p.text_muted));
                    ui.add_space(6.0);
                    let block_txt = self.t("managed_content.policy.block").to_string();
                    let allow_txt = self.t("managed_content.policy.allow").to_string();
                    let warn_txt = "Warn in logs & continue".to_string();
                    let policy_label = |val: &str| match val.trim().to_ascii_lowercase().as_str() {
                        "warn" | "ask" => warn_txt.as_str(),
                        "allow" => allow_txt.as_str(),
                        _ => block_txt.as_str(),
                    };

                    let cur_mr = policy_label(&self.prefs.modrinth_failure_policy);
                    let mr_w = auto_combo_width(ui, cur_mr, 180.0, 280.0);
                    egui::ComboBox::from_label(&mr_lbl)
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
                                        &mut self.prefs.modrinth_failure_policy,
                                        v.to_string(),
                                        lbl,
                                    )
                                    .changed()
                                {
                                    *prefs_dirty = true;
                                }
                            }
                        });

                    ui.add_space(4.0);
                    let cur_cf = policy_label(&self.prefs.curseforge_failure_policy);
                    let cf_w = auto_combo_width(ui, cur_cf, 180.0, 280.0);
                    egui::ComboBox::from_label(&cf_lbl)
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
                                        &mut self.prefs.curseforge_failure_policy,
                                        v.to_string(),
                                        lbl,
                                    )
                                    .changed()
                                {
                                    *prefs_dirty = true;
                                }
                            }
                        });
                });
            ui.add_space(8.0);
        }

        matched
    }

    #[allow(clippy::too_many_arguments)]
    fn render_settings_runtime_tab(
        &mut self,
        ui: &mut egui::Ui,
        search_q: Option<&str>,
        prefs_dirty: &mut bool,
        scan_java: &mut bool,
        open_java_manager: &mut bool,
        check_latest_java: &mut bool,
        install_java_major: &mut Option<u32>,
    ) -> usize {
        let p = self.palette;
        let cat_title = self.t("settings.section.runtime").to_string();
        let cat_detail = self.t("settings.section.runtime_detail").to_string();
        if search_q.is_none() {
            ui.label(RichText::new(&cat_title).strong().size(15.0).color(p.text));
            ui.label(RichText::new(&cat_detail).size(11.5).color(p.text_muted));
            ui.add_space(10.0);
        }
        let mut matched = 0usize;

        // Card 1: Compact Java Runtimes Summary + Java Runtimes OS Window + Latest JDK
        let java_title = self.t("launcher_settings.java.title").to_string();
        let java_desc = self.t("launcher_settings.java.description").to_string();
        let scan_lbl = self.t("Scan Java installations").to_string();
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                &java_title,
                &java_desc,
                &scan_lbl,
                "java runtimes manage scan temurin jdk latest 8 17 21 25 uninstall",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&java_title).strong());
                    ui.label(RichText::new(&java_desc).size(11.5).color(p.text_muted));
                    ui.add_space(6.0);

                    if self.java_scanning {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(self.t("java.scan.progress")).color(p.accent));
                        });
                    } else {
                        let total_detected = if !self.java_diagnostics.is_empty() {
                            self.java_diagnostics.len()
                        } else {
                            self.java_installations.len()
                        };
                        let managed_count = if !self.java_diagnostics.is_empty() {
                            self.java_diagnostics.iter().filter(|d| d.is_managed).count()
                        } else {
                            self.java_installations
                                .iter()
                                .filter(|j| j.path.starts_with(&self.paths.runtimes_root))
                                .count()
                        };
                        let mut majors = std::collections::BTreeSet::new();
                        if !self.java_diagnostics.is_empty() {
                            for d in &self.java_diagnostics {
                                if d.major_version > 0 {
                                    majors.insert(d.major_version);
                                }
                            }
                        } else {
                            for j in &self.java_installations {
                                if j.major > 0 {
                                    majors.insert(j.major);
                                }
                            }
                        }

                        if total_detected == 0 {
                            ui.label(
                                RichText::new(self.t("launcher_settings.java.none"))
                                    .color(p.warning),
                            );
                        } else {
                            let majors_str = if majors.is_empty() {
                                "unknown".to_string()
                            } else {
                                majors
                                    .iter()
                                    .map(|m| format!("Java {m}"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            };
                            ui.label(
                                RichText::new(format!(
                                    "Detected {total_detected} runtime(s) ({managed_count} managed) - Available: {majors_str}"
                                ))
                                .monospace()
                                .size(11.5)
                                .color(p.text_muted),
                            );
                        }
                    }

                    if self.latest_java_checking {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                RichText::new(self.t("launcher_settings.java.latest_checking"))
                                    .size(11.5)
                                    .color(p.accent),
                            );
                        });
                    } else if let Some(latest_major) = self.latest_java_major {
                        let txt = self
                            .lang
                            .tr_fmt(
                                "launcher_settings.java.latest_version",
                                &[("major", &latest_major.to_string())],
                            )
                            .replace(['\u{2014}', '\u{2013}'], "-");
                        ui.label(RichText::new(txt).size(11.5).color(p.text_muted));
                    } else if let Some(ref err) = self.latest_java_error {
                        ui.label(
                            RichText::new(format!(
                                "{} ({err})",
                                self.t("launcher_settings.java.latest_unavailable")
                            ))
                            .size(11.0)
                            .color(p.warning),
                        );
                    }

                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        let mgr_btn = Button::new(
                            RichText::new("Manage Java Runtimes...")
                                .color(p.accent_text)
                                .strong(),
                        )
                        .fill(p.accent);
                        if ui.add(mgr_btn).clicked() {
                            *open_java_manager = true;
                        }

                        if let Some(latest_major) = self.latest_java_major {
                            let busy = self.installing_java_majors.contains(&latest_major);
                            let managed_exists = self
                                .java_diagnostics
                                .iter()
                                .any(|d| d.is_managed && d.major_version == latest_major)
                                || self
                                    .java_installations
                                    .iter()
                                    .any(|j| {
                                        j.major == latest_major
                                            && j.path.starts_with(&self.paths.runtimes_root)
                                    });
                            let lbl = if busy {
                                format!("Installing Java {latest_major}...")
                            } else if managed_exists {
                                self.lang.tr_fmt(
                                    "launcher_settings.java.latest_reinstall",
                                    &[("major", &latest_major.to_string())],
                                )
                            } else {
                                self.lang.tr_fmt(
                                    "launcher_settings.java.latest_install",
                                    &[("major", &latest_major.to_string())],
                                )
                            };
                            if ui.add_enabled(!busy, Button::new(lbl)).clicked() {
                                *install_java_major = Some(latest_major);
                            }
                        } else {
                            let checking = self.latest_java_checking;
                            let lbl = if checking {
                                "Checking latest JDK...".to_string()
                            } else {
                                "Check Latest JDK".to_string()
                            };
                            if ui.add_enabled(!checking, Button::new(lbl)).clicked() {
                                *check_latest_java = true;
                            }
                        }

                        if ui.button(&scan_lbl).clicked() {
                            *scan_java = true;
                        }
                    });
                });
            ui.add_space(8.0);
        }

        // Card 2: Default settings for new instances
        let def_title = self.t("instance_defaults.launcher.title").to_string();
        let def_desc = self.t("instance_defaults.launcher.description").to_string();
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                &def_title,
                &def_desc,
                "default memory min ram max ram width height fullscreen new instances",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&def_title).strong());
                    ui.label(RichText::new(&def_desc).size(11.5).color(p.text_muted));
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Min RAM (MB):");
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.prefs.default_min_memory)
                                    .range(256..=32768),
                            )
                            .changed()
                        {
                            *prefs_dirty = true;
                        }
                        ui.label("Max RAM (MB):");
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.prefs.default_max_memory)
                                    .range(512..=65536),
                            )
                            .changed()
                        {
                            *prefs_dirty = true;
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Width:");
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.prefs.default_width)
                                    .range(320..=7680),
                            )
                            .changed()
                        {
                            *prefs_dirty = true;
                        }
                        ui.label("Height:");
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.prefs.default_height)
                                    .range(240..=4320),
                            )
                            .changed()
                        {
                            *prefs_dirty = true;
                        }
                        if ui
                            .checkbox(&mut self.prefs.default_fullscreen, "Fullscreen")
                            .changed()
                        {
                            *prefs_dirty = true;
                        }
                    });
                });
            ui.add_space(8.0);
        }

        // Card 3: Forge/NeoForge Preflight Failure Policy
        let fp_title = self.t("forge_preflight.launcher.title").to_string();
        let fp_detail = self.t("forge_preflight.launcher.detail").to_string();
        let fp_label = self.t("instance_defaults.policy.forge").to_string();
        if settings_card_matches(
            search_q,
            &[
                &cat_title,
                &fp_title,
                &fp_detail,
                &fp_label,
                "forge neoforge preflight failure policy block allow warn",
            ],
        ) {
            matched += 1;
            if search_q.is_some() {
                render_settings_search_badge(ui, p, &cat_title);
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&fp_title).strong());
                    ui.label(RichText::new(&fp_detail).size(11.5).color(p.text_muted));
                    ui.add_space(6.0);
                    let blk_lbl = self.t("forge_preflight.policy.block").to_string();
                    let alw_lbl = self.t("forge_preflight.policy.allow").to_string();
                    let wrn_lbl = "Warn in logs & continue".to_string();
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
                    let fp_w = auto_combo_width(ui, cur_fp, 210.0, 320.0);
                    egui::ComboBox::from_label(&fp_label)
                        .selected_text(cur_fp)
                        .width(fp_w)
                        .truncate()
                        .show_ui(ui, |ui| {
                            for (v, lbl) in
                                [("warn", &wrn_lbl), ("block", &blk_lbl), ("allow", &alw_lbl)]
                            {
                                if ui
                                    .selectable_value(
                                        &mut self.prefs.forge_preflight_failure_policy,
                                        v.to_string(),
                                        lbl,
                                    )
                                    .changed()
                                {
                                    *prefs_dirty = true;
                                }
                            }
                        });
                });
            ui.add_space(8.0);
        }

        matched
    }
}
