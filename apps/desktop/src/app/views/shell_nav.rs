//! v0.11 shell: background painting, floating top pill navigation, instance icon rail and
//! the instance flyout. Visual rules: DESIGN.md v2.0 (translucent rounded panels, simple
//! animations only, `MotionMode` respected).

use crate::app::platform::*;
use crate::app::*;
use eframe::egui::{Id, Margin, Order, Rect, Sense, UiBuilder};

pub(crate) const TOP_BAR_H: f32 = 56.0;
pub(crate) const RAIL_W: f32 = 70.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavTarget {
    Home,
    Mods,
    Versions,
    Logs,
}

fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

impl McwApp {
    /// Animation length for simple UI transitions, honoring Launcher Settings → Motion.
    pub(crate) fn motion_secs(&self, base: f32) -> f32 {
        match self.motion_mode {
            MotionMode::Full => base,
            MotionMode::Reduced => base * 0.35,
            MotionMode::Off => 0.0,
        }
    }

    pub(crate) fn paint_background(&mut self, ctx: &egui::Context) {
        if let Some((key, path)) = self.background.ensure_or_request(
            ctx,
            self.prefs.background_enabled,
            &self.prefs.background_image_path,
        ) {
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            self.runtime.spawn_blocking(move || {
                let result = decode_user_image(&path);
                let _ = tx.send(AppEvent::BackgroundImageLoaded { key, result });
                ctx.request_repaint();
            });
        }
        if self.background.is_active() {
            self.background
                .paint(ctx, self.prefs.background_dim, Color32::BLACK);
        } else {
            let bg_col = if self.theme_mode == ThemeMode::Dark {
                Color32::BLACK
            } else {
                self.palette.canvas
            };
            ctx.layer_painter(egui::LayerId::background()).rect_filled(
                ctx.screen_rect(),
                0.0,
                bg_col,
            );
        }
    }

    pub(crate) fn open_create_instance_dialog(&mut self, ctx: &egui::Context) {
        if self.show_create_modal {
            focus_viewport(
                ctx,
                egui::ViewportId::from_hash_of("mcw_vp_create_instance"),
                Some("Create Instance - MCW Launcher"),
            );
            return;
        }
        self.create_tab = CreateInstanceTab::Custom;
        self.create_name.clear();
        self.create_error = None;
        self.create_enable_lan_agent = false;
        self.archive_enable_lan_agent = false;
        self.modpack_manual_downloads.clear();
        let latest = self.latest_mc_version(self.create_show_snapshots);
        if !latest.is_empty() {
            self.create_version = latest;
        }
        self.show_create_modal = true;
    }

    /// Round icon button used by the top bar and rail. Returns the response and its rect.
    fn round_button(&self, ui: &mut egui::Ui, size: f32, active: bool) -> (egui::Response, Rect) {
        let p = self.palette;
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
        let t = ui.ctx().animate_bool_with_time(
            resp.id.with("hover"),
            resp.hovered(),
            self.motion_secs(0.12),
        );
        let fill = if active {
            with_alpha(p.accent, 70)
        } else {
            lerp_color(p.surface, p.elevated, t)
        };
        let stroke = if active || resp.hovered() {
            Stroke::new(1.0_f32, p.accent)
        } else {
            Stroke::new(1.0_f32, p.border)
        };
        ui.painter().rect(
            rect,
            CornerRadius::same((size * 0.34) as u8),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        (resp, rect)
    }

    fn nav_pill(&self, ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
        let p = self.palette;
        let font = egui::FontId::proportional(13.5);
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_string(), font.clone(), p.text);
        let size = Vec2::new(galley.size().x + 26.0, 30.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let t = ui.ctx().animate_bool_with_time(
            resp.id.with("hover"),
            resp.hovered(),
            self.motion_secs(0.12),
        );
        let fill = if active {
            with_alpha(p.accent, 60)
        } else {
            lerp_color(with_alpha(p.elevated, 0), with_alpha(p.elevated, 200), t)
        };
        ui.painter().rect_filled(rect, CornerRadius::same(15), fill);
        if active {
            let y = rect.bottom() - 3.0;
            ui.painter().line_segment(
                [
                    egui::pos2(rect.left() + 14.0, y),
                    egui::pos2(rect.right() - 14.0, y),
                ],
                Stroke::new(2.0_f32, p.accent),
            );
        }
        let text_col = if active {
            p.text
        } else {
            lerp_color(p.text_muted, p.text, t)
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font,
            text_col,
        );
        resp
    }

    pub(crate) fn render_top_bar(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let home_lbl = self.t("navigation.home").to_string();
        let mods_lbl = "Mod & Modpacks".to_string();
        let logs_lbl = self.t("navigation.logs").to_string();
        let items = [
            (NavTarget::Home, home_lbl),
            (NavTarget::Mods, mods_lbl),
            (NavTarget::Versions, self.t("Versions").to_string()),
            (NavTarget::Logs, logs_lbl),
        ];
        let mut clicked: Option<NavTarget> = None;
        let mut go_back = false;
        let mut go_forward = false;
        let mut open_accounts = false;
        let mut open_about = false;
        let mut check_connectivity = false;

        egui::TopBottomPanel::top("mcw_top_bar")
            .exact_height(TOP_BAR_H)
            .frame(Frame::new().inner_margin(Margin::symmetric(10, 9)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Clickable MCW Launcher logo -> Home
                    let (logo_rect, logo_resp) =
                        ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
                    let logo_hover = ui.ctx().animate_bool_with_time(
                        logo_resp.id.with("hover"),
                        logo_resp.hovered(),
                        self.motion_secs(0.12),
                    );
                    ui.painter().rect(
                        logo_rect,
                        CornerRadius::same(10),
                        lerp_color(p.surface, p.elevated, logo_hover),
                        Stroke::new(
                            1.0_f32,
                            if logo_resp.hovered() {
                                p.accent
                            } else {
                                p.border
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    if let Some(ref tex) = self.app_logo_texture {
                        let inner = logo_rect.shrink(4.0);
                        ui.painter().image(
                            tex.id(),
                            inner,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        ui.painter().text(
                            logo_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "M",
                            egui::FontId::proportional(16.0),
                            p.accent,
                        );
                    }
                    if logo_resp.on_hover_text("MCW Launcher").clicked() {
                        clicked = Some(NavTarget::Home);
                    }

                    ui.add_space(4.0);
                    let can_back = self.can_go_back();
                    let back_resp = self.nav_arrow_button(ui, can_back, false);
                    if can_back && back_resp.clicked() {
                        go_back = true;
                    }
                    let can_fwd = self.can_go_forward();
                    let fwd_resp = self.nav_arrow_button(ui, can_fwd, true);
                    if can_fwd && fwd_resp.clicked() {
                        go_forward = true;
                    }

                    ui.add_space(4.0);
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(20))
                        .inner_margin(Margin::symmetric(5, 3))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                            ui.horizontal(|ui| {
                                for (target, label) in &items {
                                    let active = match target {
                                        NavTarget::Home => self.active_page == AppPage::Home,
                                        NavTarget::Mods => {
                                            self.active_page == AppPage::Modpacks
                                                || self.show_content_browser
                                        }
                                        NavTarget::Versions => {
                                            self.active_page == AppPage::Versions
                                        }
                                        NavTarget::Logs => self.active_page == AppPage::Logs,
                                    };
                                    if self.nav_pill(ui, label, active).clicked() {
                                        clicked = Some(*target);
                                    }
                                }
                            });
                        });

                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("v{}", mcw_models::APP_VERSION))
                            .monospace()
                            .size(11.0)
                            .color(p.text_muted),
                    );
                    if self.connectivity_status == ConnectivityStatus::Offline {
                        ui.add_space(6.0);
                        let offline_btn = Button::new(
                            RichText::new(if self.connectivity_checking {
                                "Checking..."
                            } else {
                                "Offline"
                            })
                            .size(10.5)
                            .strong()
                            .color(Color32::BLACK),
                        )
                        .fill(p.warning)
                        .corner_radius(CornerRadius::same(10));
                        if ui
                            .add_enabled(!self.connectivity_checking, offline_btn)
                            .on_hover_text("No internet reachability detected. Click to re-check.")
                            .clicked()
                        {
                            check_connectivity = true;
                        }
                    }
                    if mcw_updater::UPDATES_ENABLED {
                        if let Some(ref upd) = self.last_update_check {
                            if upd.update_available {
                                if let Some(ref latest) = upd.latest {
                                    let badge_btn = Button::new(
                                        RichText::new(format!("Update v{}", latest.version))
                                            .size(11.0)
                                            .strong()
                                            .color(p.accent_text),
                                    )
                                    .fill(p.accent)
                                    .corner_radius(CornerRadius::same(10));
                                    if ui.add(badge_btn).clicked() {
                                        self.show_update_dialog = true;
                                    }
                                }
                            }
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Account chip with crisp 2D skin face avatar
                        if let Some(acc) = self.active_account().cloned() {
                            let kind = match acc.account_type {
                                AccountSource::Microsoft => "MSA",
                                AccountSource::Offline => "Offline",
                            };
                            let chip = Button::new(format!("{} ({kind})", acc.username))
                                .corner_radius(CornerRadius::same(18))
                                .min_size(Vec2::new(0.0, 34.0));
                            let clicked_label = ui.add(chip).clicked();
                            self.render_account_avatar(ui, &acc.uuid, 26.0);
                            if clicked_label {
                                open_accounts = true;
                            }
                        } else if ui
                            .add(
                                Button::new(self.t("Sign in / Add Account"))
                                    .corner_radius(CornerRadius::same(18))
                                    .min_size(Vec2::new(0.0, 34.0)),
                            )
                            .clicked()
                        {
                            open_accounts = true;
                        }
                        ui.add_space(6.0);

                        // Compact Horizon-style Quick Search (Ctrl+K) trigger pill
                        let search_lbl = self.t("common.search").to_string();
                        let (qs_rect, qs_resp) =
                            ui.allocate_exact_size(Vec2::new(168.0, 34.0), egui::Sense::click());
                        if ui.is_rect_visible(qs_rect) {
                            let painter = ui.painter();
                            let border_col = if self.show_quick_search || qs_resp.hovered() {
                                p.accent
                            } else {
                                p.border
                            };
                            painter.rect(
                                qs_rect,
                                CornerRadius::same(17),
                                p.surface,
                                Stroke::new(1.0_f32, border_col),
                                egui::StrokeKind::Inside,
                            );
                            // Vector magnifying glass icon
                            let icon_c = egui::pos2(qs_rect.min.x + 17.0, qs_rect.center().y - 1.0);
                            let icon_col = if qs_resp.hovered() {
                                p.text
                            } else {
                                p.text_muted
                            };
                            painter.circle_stroke(icon_c, 4.6, Stroke::new(1.5_f32, icon_col));
                            painter.line_segment(
                                [icon_c + Vec2::new(3.2, 3.2), icon_c + Vec2::new(6.8, 6.8)],
                                Stroke::new(1.6_f32, icon_col),
                            );
                            painter.text(
                                egui::pos2(qs_rect.min.x + 30.0, qs_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!("{search_lbl}..."),
                                egui::FontId::proportional(12.0),
                                p.text_muted,
                            );
                            let kbd_rect = egui::Rect::from_min_max(
                                egui::pos2(qs_rect.max.x - 52.0, qs_rect.center().y - 10.0),
                                egui::pos2(qs_rect.max.x - 8.0, qs_rect.center().y + 10.0),
                            );
                            painter.rect(
                                kbd_rect,
                                CornerRadius::same(6),
                                p.elevated,
                                Stroke::new(1.0_f32, p.border),
                                egui::StrokeKind::Inside,
                            );
                            painter.text(
                                kbd_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "Ctrl K",
                                egui::FontId::monospace(10.0),
                                p.text_muted,
                            );
                        }
                        if qs_resp.clicked() {
                            self.show_quick_search = !self.show_quick_search;
                            if self.show_quick_search {
                                self.quick_search_query.clear();
                                self.quick_search_open_since = Some(Instant::now());
                                self.quick_search_focus_pending = true;
                            }
                        }

                        ui.add_space(6.0);
                        if ui
                            .selectable_label(self.show_about_window, self.t("navigation.about"))
                            .clicked()
                        {
                            open_about = true;
                        }
                    });
                });
            });

        if open_accounts {
            self.open_accounts_window(ctx);
        }
        if open_about {
            self.open_about_window(ctx);
        }
        if check_connectivity {
            self.start_connectivity_check(ctx);
        }
        if go_back {
            self.navigate_back(ctx);
        } else if go_forward {
            self.navigate_forward(ctx);
        }
        if let Some(target) = clicked {
            match target {
                NavTarget::Home => {
                    self.navigate_to_page(AppPage::Home, ctx);
                }
                NavTarget::Mods => {
                    self.open_content_browser_mode(BrowserCategory::Mods, None, true, ctx);
                }
                NavTarget::Versions => {
                    self.navigate_to_page(AppPage::Versions, ctx);
                }
                NavTarget::Logs => {
                    self.navigate_to_page(AppPage::Logs, ctx);
                }
            }
        }
    }

    fn nav_arrow_button(&self, ui: &mut egui::Ui, enabled: bool, forward: bool) -> egui::Response {
        let p = self.palette;
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), sense);
        let t = ui.ctx().animate_bool_with_time(
            resp.id.with("hover"),
            enabled && resp.hovered(),
            self.motion_secs(0.12),
        );
        let fill = if enabled {
            lerp_color(p.surface, p.elevated, t)
        } else {
            with_alpha(p.surface, 130)
        };
        let stroke_col = if enabled && resp.hovered() {
            p.accent
        } else if enabled {
            p.border
        } else {
            with_alpha(p.border, 100)
        };
        let radius = rect.width() * 0.5;
        ui.painter().circle(
            rect.center(),
            radius,
            fill,
            Stroke::new(1.0_f32, stroke_col),
        );
        let arrow_col = if enabled {
            p.text
        } else {
            with_alpha(p.text_muted, 110)
        };
        let cx = rect.center().x;
        let cy = rect.center().y;
        let dir = if forward { 1.0_f32 } else { -1.0_f32 };
        let pts = [
            egui::pos2(cx - 2.5 * dir, cy - 4.5),
            egui::pos2(cx + 2.5 * dir, cy),
            egui::pos2(cx - 2.5 * dir, cy + 4.5),
        ];
        ui.painter()
            .line_segment([pts[0], pts[1]], Stroke::new(1.8_f32, arrow_col));
        ui.painter()
            .line_segment([pts[1], pts[2]], Stroke::new(1.8_f32, arrow_col));
        let tip = if forward {
            self.t("nav.forward")
        } else {
            self.t("nav.back")
        };
        resp.on_hover_text(tip)
    }

    /// Slim floating icon rail: add instance, instance icons, Settings and version.
    pub(crate) fn render_instance_rail(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let snapshots: Vec<Instance> = self.instances.clone();
        for inst in &snapshots {
            self.ensure_instance_icon(inst, ctx);
        }
        let mut open_create = false;
        let mut select: Option<String> = None;
        let mut open_menu: Option<(String, egui::Pos2)> = None;
        let mut toggle_settings = false;

        egui::SidePanel::left("mcw_instance_rail")
            .exact_width(RAIL_W)
            .resizable(false)
            .frame(Frame::new().inner_margin(Margin {
                left: 10,
                right: 0,
                top: 0,
                bottom: 10,
            }))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(20))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_min_height(ui.available_height());
                        ui.set_width(ui.available_width());
                        ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(mcw_models::APP_VERSION)
                                    .monospace()
                                    .size(8.5)
                                    .color(p.text_muted),
                            );
                            if gear_icon_button(ui, p, self.show_launcher_settings)
                                .on_hover_text(self.t("Launcher Settings"))
                                .clicked()
                            {
                                toggle_settings = true;
                            }
                            ui.add_space(4.0);
                            ui.separator();
                            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                                let (resp, rect) = self.round_button(ui, 44.0, false);
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "+",
                                    egui::FontId::proportional(24.0),
                                    p.accent,
                                );
                                if resp
                                    .on_hover_text(self.t("workspace.create.title"))
                                    .clicked()
                                {
                                    open_create = true;
                                }
                                ui.add_space(4.0);
                                ScrollArea::vertical()
                                    .id_salt("mcw_rail_scroll")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.vertical_centered(|ui| {
                                            for inst in &snapshots {
                                                self.rail_instance_button(
                                                    ui,
                                                    inst,
                                                    &mut select,
                                                    &mut open_menu,
                                                );
                                                ui.add_space(4.0);
                                            }
                                        });
                                    });
                            });
                        });
                    });
            });

        if open_create {
            self.open_create_instance_dialog(ctx);
        }
        if toggle_settings {
            self.show_launcher_settings = !self.show_launcher_settings;
        }
        if let Some((id, pos)) = open_menu {
            self.selected_instance_id = Some(id.clone());
            self.sync_selected_from_cache();
            self.ensure_active_tab_loaded(ctx);
            self.sidebar_context_menu = Some((id, pos));
        } else if let Some(id) = select {
            if self.active_page != AppPage::Home {
                self.active_page = AppPage::Home;
            }
            self.sidebar_context_menu = None;
            self.request_instance_switch(id, ctx);
        }
        self.render_sidebar_context_menu(ctx);
    }

    fn rail_instance_button(
        &self,
        ui: &mut egui::Ui,
        inst: &Instance,
        select: &mut Option<String>,
        open_menu: &mut Option<(String, egui::Pos2)>,
    ) {
        let p = self.palette;
        let selected = self.selected_instance_id.as_deref() == Some(inst.instance_id.as_str());
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(46.0), Sense::click());
        let t = ui.ctx().animate_bool_with_time(
            resp.id.with("hover"),
            resp.hovered(),
            self.motion_secs(0.12),
        );
        let fill = if selected {
            with_alpha(p.accent, 60)
        } else {
            lerp_color(with_alpha(p.elevated, 0), with_alpha(p.elevated, 220), t)
        };
        let stroke = if selected {
            Stroke::new(1.5_f32, p.accent)
        } else {
            Stroke::NONE
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(14),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(32.0));
        ui.scope_builder(UiBuilder::new().max_rect(icon_rect), |ui| {
            self.render_instance_icon(ui, inst, 32.0);
        });

        // Status dot (running / preparing / incomplete / crashed)
        let missing = self
            .pending_manual_counts
            .get(&inst.instance_id)
            .copied()
            .unwrap_or(0);
        let dot = match self.launch_states.get(&inst.instance_id) {
            Some(LaunchState::Running { .. }) => Some(p.accent),
            Some(LaunchState::Preparing(_)) => Some(p.warning),
            _ if missing > 0 => Some(p.warning),
            _ if inst.last_launch_state == "crashed" => Some(p.danger),
            _ => None,
        };
        if let Some(c) = dot {
            let center = egui::pos2(rect.right() - 6.0, rect.bottom() - 6.0);
            ui.painter().circle_filled(center, 5.0, p.canvas);
            ui.painter().circle_filled(center, 3.5, c);
        }

        let tip = format!(
            "{}\n{} - {}",
            inst.name,
            inst.version_id,
            display_loader_name(&inst.mod_loader.name)
        );
        if resp.on_hover_text(tip).clicked() {
            *select = Some(inst.instance_id.clone());
        } else if ui.rect_contains_pointer(rect) && ui.input(|i| i.pointer.secondary_clicked()) {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                *open_menu = Some((inst.instance_id.clone(), pos));
            }
        }
    }

    /// Animated startup splash screen displayed when entering the launcher.
    pub(crate) fn render_startup_splash(&mut self, ctx: &egui::Context) {
        if self.startup_splash_done {
            return;
        }
        let elapsed = self.startup_splash_started_at.elapsed().as_secs_f32();
        const TOTAL_SECS: f32 = 1.35;
        const FADE_SECS: f32 = 0.25;
        if elapsed >= TOTAL_SECS {
            self.startup_splash_done = true;
            return;
        }

        let p = self.palette;
        let raw_t = (elapsed / (TOTAL_SECS - FADE_SECS)).clamp(0.0, 1.0);
        let progress = 1.0 - (1.0 - raw_t).powi(3);
        let alpha = if elapsed > (TOTAL_SECS - FADE_SECS) {
            ((TOTAL_SECS - elapsed) / FADE_SECS).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let intro_t = (elapsed / 0.35).clamp(0.0, 1.0);
        let logo_scale = 0.88 + 0.12 * (1.0 - (1.0 - intro_t).powi(3));

        let fade_u8 = |col: Color32, base_a: u8| -> Color32 {
            let a = ((f32::from(base_a)) * alpha).round().clamp(0.0, 255.0) as u8;
            Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), a)
        };

        let status_key = if progress < 0.40 {
            "startup.stage.init"
        } else if progress < 0.85 {
            "startup.stage.loading"
        } else {
            "startup.stage.ready"
        };
        let status_text = self.t(status_key).to_string();
        let screen = ctx.screen_rect();

        egui::Area::new(Id::new("mcw_startup_splash"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                ui.set_max_size(screen.size());
                let bg_col = fade_u8(Color32::from_rgb(7, 9, 14), 255);
                ui.painter().rect_filled(screen, 0.0, bg_col);

                let center = screen.center();
                let logo_side = 76.0 * logo_scale;
                let logo_rect = egui::Rect::from_center_size(
                    egui::pos2(center.x, center.y - 68.0),
                    Vec2::splat(logo_side),
                );
                ui.painter().rect(
                    logo_rect.expand(6.0),
                    CornerRadius::same(22),
                    fade_u8(p.surface, 235),
                    Stroke::new(1.5_f32, fade_u8(p.accent, 210)),
                    egui::StrokeKind::Outside,
                );
                if let Some(ref tex) = self.app_logo_texture {
                    ui.painter().image(
                        tex.id(),
                        logo_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        fade_u8(Color32::WHITE, 255),
                    );
                }

                ui.painter().text(
                    egui::pos2(center.x, center.y + 4.0),
                    egui::Align2::CENTER_CENTER,
                    "MCW Launcher: Rust Edition",
                    egui::FontId::proportional(24.0),
                    fade_u8(p.text, 255),
                );

                let ver_text = format!("v{}", mcw_models::APP_VERSION);
                let pill_w = (ver_text.len() as f32 * 7.5 + 28.0).max(96.0);
                let pill_rect = egui::Rect::from_center_size(
                    egui::pos2(center.x, center.y + 34.0),
                    Vec2::new(pill_w, 22.0),
                );
                ui.painter().rect(
                    pill_rect,
                    CornerRadius::same(11),
                    fade_u8(p.surface, 220),
                    Stroke::new(1.0_f32, fade_u8(p.accent, 140)),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    pill_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    ver_text,
                    egui::FontId::monospace(11.5),
                    fade_u8(p.accent, 255),
                );

                let bar_w = (screen.width() * 0.34).clamp(240.0, 360.0);
                let bar_rect = egui::Rect::from_center_size(
                    egui::pos2(center.x, center.y + 66.0),
                    Vec2::new(bar_w, 7.0),
                );
                ui.painter()
                    .rect_filled(bar_rect, CornerRadius::same(4), fade_u8(p.surface, 230));
                let fill_w = (bar_rect.width() * progress).max(8.0);
                let fill_rect =
                    egui::Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, bar_rect.height()));
                ui.painter()
                    .rect_filled(fill_rect, CornerRadius::same(4), fade_u8(p.accent, 255));

                ui.painter().text(
                    egui::pos2(center.x, center.y + 88.0),
                    egui::Align2::CENTER_CENTER,
                    status_text,
                    egui::FontId::proportional(12.5),
                    fade_u8(p.text_muted, 255),
                );
            });

        ctx.request_repaint();
    }
}
