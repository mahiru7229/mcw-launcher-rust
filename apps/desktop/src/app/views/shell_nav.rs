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
    Servers,
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
        if let Some(err) = self.background.ensure(
            ctx,
            self.prefs.background_enabled,
            &self.prefs.background_image_path,
        ) {
            self.notice_banner = Some((err, true));
        }
        if self.background.is_active() {
            self.background
                .paint(ctx, self.prefs.background_dim, self.palette.canvas);
        } else {
            ctx.layer_painter(egui::LayerId::background()).rect_filled(
                ctx.screen_rect(),
                0.0,
                self.palette.canvas,
            );
        }
    }

    pub(crate) fn open_create_instance_dialog(&mut self) {
        self.create_tab = CreateInstanceTab::Custom;
        self.create_name.clear();
        self.create_error = None;
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
        let mods_lbl = self.t("navigation.mods").to_string();
        let logs_lbl = self.t("navigation.logs").to_string();
        let items = [
            (NavTarget::Home, home_lbl),
            (NavTarget::Mods, mods_lbl),
            (NavTarget::Servers, self.t("Servers").to_string()),
            (NavTarget::Versions, self.t("Versions").to_string()),
            (NavTarget::Logs, logs_lbl),
        ];
        let mut clicked: Option<NavTarget> = None;
        let mut toggle_flyout = false;

        egui::TopBottomPanel::top("mcw_top_bar")
            .exact_height(TOP_BAR_H)
            .frame(Frame::new().inner_margin(Margin::symmetric(10, 9)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Hamburger: opens the instance flyout.
                    let (resp, rect) = self.round_button(ui, 38.0, self.show_instance_flyout);
                    for dy in [-6.0_f32, 0.0, 6.0] {
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.center().x - 8.0, rect.center().y + dy),
                                egui::pos2(rect.center().x + 8.0, rect.center().y + dy),
                            ],
                            Stroke::new(2.0_f32, p.text),
                        );
                    }
                    self.flyout_toggle_rect = Some(rect);
                    if resp.on_hover_text(self.t("Instances")).clicked() {
                        toggle_flyout = true;
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
                                        NavTarget::Home => {
                                            self.active_tab == DetailTab::Overview
                                                && !self.show_global_logs_window
                                        }
                                        NavTarget::Mods => self.active_tab == DetailTab::Mods,
                                        NavTarget::Servers => {
                                            self.active_tab == DetailTab::Settings
                                        }
                                        NavTarget::Versions => {
                                            self.show_launcher_settings
                                                && self.launcher_settings_tab
                                                    == LauncherSettingsTab::Storage
                                        }
                                        NavTarget::Logs => self.show_global_logs_window,
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
                                self.show_account_modal = true;
                            }
                        } else if ui
                            .add(
                                Button::new(self.t("Sign in / Add Account"))
                                    .corner_radius(CornerRadius::same(18))
                                    .min_size(Vec2::new(0.0, 34.0)),
                            )
                            .clicked()
                        {
                            self.show_account_modal = true;
                        }
                        if ui
                            .selectable_label(self.show_about_window, self.t("navigation.about"))
                            .clicked()
                        {
                            self.show_about_window = !self.show_about_window;
                        }
                    });
                });
            });

        if toggle_flyout {
            self.show_instance_flyout = !self.show_instance_flyout;
        }
        if let Some(target) = clicked {
            match target {
                NavTarget::Home => {
                    self.active_tab = DetailTab::Overview;
                    self.show_global_logs_window = false;
                }
                NavTarget::Mods => self.active_tab = DetailTab::Mods,
                NavTarget::Servers => self.active_tab = DetailTab::Settings,
                NavTarget::Versions => {
                    self.launcher_settings_tab = LauncherSettingsTab::Storage;
                    self.show_launcher_settings = true;
                }
                NavTarget::Logs => {
                    self.show_global_logs_window = !self.show_global_logs_window;
                }
            }
            if matches!(
                target,
                NavTarget::Home | NavTarget::Mods | NavTarget::Servers
            ) {
                self.ensure_active_tab_loaded(ctx);
            }
        }
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
            self.open_create_instance_dialog();
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
            self.selected_instance_id = Some(id);
            self.sidebar_context_menu = None;
            self.sync_selected_from_cache();
            self.ensure_active_tab_loaded(ctx);
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
            "{}\n{} · {}",
            inst.name, inst.version_id, inst.mod_loader.name
        );
        if resp.on_hover_text(tip).clicked() {
            *select = Some(inst.instance_id.clone());
        } else if ui.rect_contains_pointer(rect) && ui.input(|i| i.pointer.secondary_clicked()) {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                *open_menu = Some((inst.instance_id.clone(), pos));
            }
        }
    }

    /// Dropdown flyout with the full instance list (search, favorites, groups, tags).
    pub(crate) fn render_instance_flyout(&mut self, ctx: &egui::Context) {
        if !self.show_instance_flyout {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_instance_flyout = false;
            return;
        }
        let p = self.palette;
        let screen = ctx.screen_rect();
        let max_h = (screen.height() - TOP_BAR_H - 120.0).max(160.0);
        let area = egui::Area::new(Id::new("mcw_instance_flyout"))
            .order(Order::Foreground)
            .fixed_pos(egui::pos2(10.0, TOP_BAR_H - 2.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(with_alpha(p.canvas, 246))
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(18))
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(300.0);
                        self.render_instance_list(ui, ctx, max_h);
                    });
            });

        if ctx.input(|i| i.pointer.primary_clicked()) {
            let inside_toggle = self
                .flyout_toggle_rect
                .zip(ctx.input(|i| i.pointer.interact_pos()))
                .map(|(r, pos)| r.contains(pos))
                .unwrap_or(false);
            let inside_flyout = ctx
                .input(|i| i.pointer.interact_pos())
                .map(|pos| area.response.rect.contains(pos))
                .unwrap_or(false);
            if !inside_toggle && !inside_flyout {
                self.show_instance_flyout = false;
            }
        }
    }
}
