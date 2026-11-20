use crate::app::platform::*;
use crate::app::*;
use eframe::egui::{Margin, Sense, UiBuilder};

fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn quick_icon_button(
    ui: &mut egui::Ui,
    p: ThemePalette,
    motion_secs: f32,
    tooltip: &str,
    paint_icon: impl FnOnce(&egui::Painter, egui::Rect, Color32),
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::click());
    let t = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hover"), resp.hovered(), motion_secs);
    let fill = if t > 0.01 { p.elevated } else { p.surface };
    let stroke = if resp.hovered() {
        Stroke::new(1.0_f32, p.accent)
    } else {
        Stroke::new(1.0_f32, p.border)
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(10),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    paint_icon(ui.painter(), rect, p.text);
    resp.on_hover_text(tooltip)
}

impl McwApp {
    pub(crate) fn render_active_page(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        if self.fade_page != self.active_page {
            self.fade_page = self.active_page;
            self.fade_since = Instant::now();
        }
        let fade_secs = self.motion_secs(0.15);
        let fade = if fade_secs <= 0.0 {
            1.0
        } else {
            (self.fade_since.elapsed().as_secs_f32() / fade_secs).clamp(0.0, 1.0)
        };
        if fade < 1.0 {
            ctx.request_repaint();
        }

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(Margin {
                        left: 16,
                        right: 16,
                        top: 8,
                        bottom: 14,
                    }),
            )
            .show(ctx, |ui| {
                ui.set_opacity(0.35 + 0.65 * fade);

                if let Some((msg, is_err)) = self.notice_banner.clone() {
                    let banner_stroke = if is_err { p.danger } else { p.accent };
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, banner_stroke))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::symmetric(12, 8))
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

                match self.active_page {
                    AppPage::Home => self.render_home_page(ui, ctx),
                    AppPage::Servers => self.render_servers_page(ui, ctx),
                    AppPage::Versions => self.render_versions_page(ui, ctx),
                    AppPage::Logs => self.render_logs_page(ui, ctx),
                }
            });
    }

    fn render_home_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.palette;
        let Some(inst) = self.selected_instance().cloned() else {
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(16))
                .inner_margin(Margin::same(28))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.add_space(24.0);
                        ui.label(
                            RichText::new("Select or create a Minecraft instance")
                                .size(18.0)
                                .strong()
                                .color(p.text),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "Use '+' on the left rail or import a Modrinth / CurseForge / .mcwpack archive.",
                            )
                            .color(p.text_muted),
                        );
                        ui.add_space(12.0);
                        if ui
                            .add(
                                Button::new(
                                    RichText::new("+ Create Instance")
                                        .color(p.accent_text)
                                        .strong(),
                                )
                                .fill(p.accent)
                                .min_size(Vec2::new(160.0, 36.0))
                                .corner_radius(CornerRadius::same(10)),
                            )
                            .clicked()
                        {
                            self.open_create_instance_dialog();
                        }
                        ui.add_space(24.0);
                    });
                });
            return;
        };

        self.ensure_instance_icon(&inst, ctx);

        let busy = self.is_instance_busy(&inst.instance_id);
        let has_account = self.active_account().is_some();
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let tracked_pack = detect_installed_pack(&inst_dir);
        let pending_manual_count = self
            .pending_manual_counts
            .get(&inst.instance_id)
            .copied()
            .unwrap_or(0);

        let (status_text, status_color) = match self.launch_states.get(&inst.instance_id) {
            Some(LaunchState::Running { .. }) => ("Running".to_string(), p.accent),
            Some(LaunchState::Preparing(_)) => ("Preparing".to_string(), p.warning),
            _ if pending_manual_count > 0 => {
                (format!("Incomplete ({pending_manual_count})"), p.warning)
            }
            _ => match inst.last_launch_state.as_str() {
                "crashed" => ("Crashed".to_string(), p.danger),
                "finished" => ("Ready".to_string(), p.accent),
                _ => ("Ready".to_string(), p.accent),
            },
        };

        // 1. Hero Header over the background image
        ui.horizontal_top(|ui| {
            self.render_instance_icon(ui, &inst, 52.0);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&inst.name).size(28.0).strong().color(p.text));
                });
                let mut stats = format!(
                    "Playtime: {} · Mods: {} · Worlds: {}",
                    format_duration(inst.total_playtime_seconds),
                    self.selected_mods.len(),
                    self.selected_worlds.len(),
                );
                if let Some(ref pack) = tracked_pack {
                    stats.push_str(&format!(" · Pack: {} ({})", pack.pack_name, pack.provider));
                }
                ui.label(RichText::new(stats).size(12.5).color(p.text_muted));

                let has_group = !inst.group.trim().is_empty();
                let has_tags = inst.tags.iter().any(|t| !t.trim().is_empty());
                if has_group || has_tags {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(5.0, 3.0);
                        if has_group {
                            render_badge_pill(ui, &inst.group, true, false, p);
                        }
                        for tag in &inst.tags {
                            render_badge_pill(ui, tag, false, false, p);
                        }
                    });
                }
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (dot_r, _) =
                                ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                            ui.painter()
                                .circle_filled(dot_r.center(), 4.0, status_color);
                            ui.label(
                                RichText::new(&status_text)
                                    .strong()
                                    .size(12.0)
                                    .color(p.text),
                            );
                        });
                    });
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "MC {} · {}",
                                inst.version_id, inst.mod_loader.name
                            ))
                            .monospace()
                            .size(12.0)
                            .color(p.text_muted),
                        );
                    });
            });
        });

        ui.add_space(12.0);

        // 2. Hero Action Bar: Play + `...` Actions menu + Quick Icon Buttons
        let mut trigger_repair = false;
        let mut open_repair_center = false;
        let mut open_export = false;
        let mut export_loader_diag = false;
        let mut change_icon_clicked = false;
        let mut reset_icon_clicked = false;
        let mut open_content_cat: Option<BrowserCategory> = None;

        let export_label = self.t("modpack_package.export.title").to_string();
        let clone_lbl = self.t("workspace.action.clone").to_string();
        let chg_icon_lbl = self.t("workspace.action.change_icon").to_string();
        let rst_icon_lbl = self.t("workspace.action.reset_icon").to_string();
        let export_diag_lbl = self.t("forge.export_diagnostics").to_string();
        let is_diag_loader = matches!(
            inst.mod_loader.name.to_ascii_lowercase().as_str(),
            "forge" | "neoforge" | "quilt"
        );
        let motion_hover = self.motion_secs(0.12);

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
            match self.launch_states.get(&inst.instance_id) {
                Some(LaunchState::Running { control, .. }) => {
                    let stop_btn = Button::new(
                        RichText::new(self.t("Stop Game"))
                            .color(Color32::WHITE)
                            .size(14.0)
                            .strong(),
                    )
                    .fill(p.danger)
                    .min_size(Vec2::new(140.0, 38.0))
                    .corner_radius(CornerRadius::same(12));
                    if ui.add(stop_btn).clicked() {
                        control.request_stop();
                    }
                }
                Some(LaunchState::Preparing(_)) => {
                    let launch_ctrl = self.launch_task_ids.get(&inst.instance_id).and_then(|tid| {
                        self.active_tasks
                            .iter()
                            .find(|t| t.id == *tid)
                            .map(|t| Arc::clone(&t.control))
                    });
                    let is_paused = launch_ctrl.as_ref().map(|c| c.is_paused()).unwrap_or(false);
                    let is_cancelling = launch_ctrl
                        .as_ref()
                        .map(|c| c.is_cancelled())
                        .unwrap_or(false);
                    let pause_text = if is_paused {
                        self.t("launch.resume_button").to_string()
                    } else {
                        self.t("launch.pause_button").to_string()
                    };
                    if ui
                        .add_enabled(
                            !is_cancelling,
                            Button::new(
                                RichText::new(pause_text)
                                    .color(if is_paused { p.accent_text } else { p.text })
                                    .strong(),
                            )
                            .fill(if is_paused { p.accent } else { p.elevated })
                            .min_size(Vec2::new(100.0, 38.0))
                            .corner_radius(CornerRadius::same(12)),
                        )
                        .clicked()
                    {
                        if let Some(ref ctrl) = launch_ctrl {
                            ctrl.toggle_pause();
                        }
                    }
                    let cancel_text = if is_cancelling {
                        self.t("launch.cancelling_badge").to_string()
                    } else {
                        self.t("launch.cancel_button").to_string()
                    };
                    if ui
                        .add_enabled(
                            !is_cancelling,
                            Button::new(RichText::new(cancel_text).color(Color32::WHITE).strong())
                                .fill(p.danger)
                                .min_size(Vec2::new(100.0, 38.0))
                                .corner_radius(CornerRadius::same(12)),
                        )
                        .clicked()
                    {
                        if let Some(ref ctrl) = launch_ctrl {
                            ctrl.cancel();
                        }
                    }
                }
                _ if pending_manual_count > 0 => {
                    let warn_btn = Button::new(
                        RichText::new(format!("Missing Mods ({pending_manual_count})"))
                            .color(Color32::BLACK)
                            .size(13.5)
                            .strong(),
                    )
                    .fill(p.warning)
                    .min_size(Vec2::new(150.0, 38.0))
                    .corner_radius(CornerRadius::same(12));
                    if ui
                        .add(warn_btn)
                        .on_hover_text(
                            "Complete missing CurseForge manual downloads before launching.",
                        )
                        .clicked()
                    {
                        self.open_manual_downloads_for(&inst.instance_id);
                    }
                }
                _ => {
                    let play_lbl = format!("  {}", self.t("workspace.action.launch"));
                    let launch_btn = Button::new(
                        RichText::new(play_lbl)
                            .color(p.accent_text)
                            .size(14.5)
                            .strong(),
                    )
                    .fill(p.accent)
                    .min_size(Vec2::new(146.0, 38.0))
                    .corner_radius(CornerRadius::same(12));
                    let resp = ui.add_enabled(has_account, launch_btn);
                    {
                        // Draw small crisp play triangle inside the left side of the Play button
                        let r = resp.rect;
                        let cx = r.left() + 20.0;
                        let cy = r.center().y;
                        let col = if has_account {
                            p.accent_text
                        } else {
                            p.text_muted
                        };
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![
                                egui::pos2(cx - 4.0, cy - 5.5),
                                egui::pos2(cx + 5.5, cy),
                                egui::pos2(cx - 4.0, cy + 5.5),
                            ],
                            col,
                            Stroke::NONE,
                        ));
                    }
                    if !has_account {
                        resp.on_disabled_hover_text("Add or sign in to an account first");
                    } else if resp.clicked() {
                        self.start_launch(ctx);
                    }
                }
            }

            // `...` Actions dropdown holding all Instance Actions & Overview tools
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(8, 7))
                .show(ui, |ui| {
                    ui.menu_button(
                        RichText::new("•••").strong().size(13.0).color(p.text),
                        |ui| {
                            ui.set_min_width(220.0);
                            let open_f_lbl = self.t("workspace.action.open_folder").to_string();
                            if ui.button(format!("{open_f_lbl} (.minecraft)")).clicked() {
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
                            let rep_lbl = self.t("workspace.action.repair").to_string();
                            if ui
                                .add_enabled(!busy, Button::new(&rep_lbl))
                                .on_disabled_hover_text("Stop the instance before repairing")
                                .clicked()
                            {
                                trigger_repair = true;
                                ui.close_menu();
                            }
                            if ui.button("Repair Center / Health Check...").clicked() {
                                open_repair_center = true;
                                ui.close_menu();
                            }
                            if is_diag_loader && ui.button(&export_diag_lbl).clicked() {
                                export_loader_diag = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Edit Group & Tags...").clicked() {
                                self.edit_group_input = inst.group.clone();
                                self.edit_tags_input = inst.tags.join(", ");
                                self.show_group_tags_modal = true;
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
                            let ren_lbl = self.t("instance.rename").to_string();
                            if ui
                                .add_enabled(!busy, Button::new(format!("{ren_lbl}...")))
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
                            ui.separator();
                            let del_lbl = self.t("workspace.action.delete").to_string();
                            if ui
                                .add_enabled(
                                    !busy,
                                    Button::new(RichText::new(del_lbl).color(p.danger)),
                                )
                                .on_disabled_hover_text("Stop the instance before deleting")
                                .clicked()
                            {
                                self.show_delete_modal = true;
                                ui.close_menu();
                            }
                        },
                    );
                });

            // Quick icon: Add Mods / Content (puzzle / plus box vector icon)
            if quick_icon_button(
                ui,
                p,
                motion_hover,
                "Browse & Install Content...",
                |painter, r, col| {
                    let st = Stroke::new(1.6_f32, col);
                    let inner = r.shrink(11.0);
                    painter.rect_stroke(inner, CornerRadius::same(3), st, egui::StrokeKind::Inside);
                    painter.line_segment(
                        [
                            egui::pos2(r.center().x - 4.0, r.center().y),
                            egui::pos2(r.center().x + 4.0, r.center().y),
                        ],
                        st,
                    );
                    painter.line_segment(
                        [
                            egui::pos2(r.center().x, r.center().y - 4.0),
                            egui::pos2(r.center().x, r.center().y + 4.0),
                        ],
                        st,
                    );
                },
            )
            .clicked()
            {
                let cat = match self.active_tab {
                    DetailTab::ResourcePacks => BrowserCategory::ResourcePacks,
                    DetailTab::Shaders => BrowserCategory::ShaderPacks,
                    _ if inst.mod_loader.is_vanilla() => BrowserCategory::ResourcePacks,
                    _ => BrowserCategory::Mods,
                };
                open_content_cat = Some(cat);
            }

            // Quick icon: Open .minecraft Folder (clean vector folder outline)
            if quick_icon_button(
                ui,
                p,
                motion_hover,
                "Open .minecraft Folder",
                |painter, r, col| {
                    let st = Stroke::new(1.6_f32, col);
                    let c = r.center();
                    let left = c.x - 8.0;
                    let right = c.x + 8.0;
                    let top = c.y - 6.0;
                    let bot = c.y + 6.0;
                    painter.add(egui::Shape::closed_line(
                        vec![
                            egui::pos2(left, top),
                            egui::pos2(left + 6.0, top),
                            egui::pos2(left + 8.0, top + 2.5),
                            egui::pos2(right, top + 2.5),
                            egui::pos2(right, bot),
                            egui::pos2(left, bot),
                        ],
                        st,
                    ));
                },
            )
            .clicked()
            {
                let _ = std::fs::create_dir_all(&mc_dir);
                open_external(&mc_dir.display().to_string());
            }

            // Quick icon: Instance Settings (edit pencil vector)
            if quick_icon_button(
                ui,
                p,
                motion_hover,
                "Instance Settings",
                |painter, r, col| {
                    let st = Stroke::new(1.6_f32, col);
                    let c = r.center();
                    painter.line_segment(
                        [
                            egui::pos2(c.x - 6.0, c.y + 6.0),
                            egui::pos2(c.x + 6.0, c.y - 6.0),
                        ],
                        Stroke::new(2.0_f32, col),
                    );
                    painter.line_segment(
                        [
                            egui::pos2(c.x + 3.0, c.y - 6.0),
                            egui::pos2(c.x + 6.0, c.y - 3.0),
                        ],
                        st,
                    );
                    painter.line_segment(
                        [
                            egui::pos2(c.x - 7.0, c.y + 7.0),
                            egui::pos2(c.x - 3.5, c.y + 7.0),
                        ],
                        st,
                    );
                },
            )
            .clicked()
            {
                self.active_tab = DetailTab::Settings;
            }
        });

        if trigger_repair {
            self.start_repair(ctx);
        }
        if open_repair_center {
            self.open_repair_center(&inst.instance_id, ctx);
        }
        if open_export {
            self.show_export_modal = true;
        }
        if export_loader_diag {
            self.export_loader_diagnostics(&inst);
        }
        if let Some(cat) = open_content_cat {
            self.open_content_browser(cat, None, ctx);
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

        ui.add_space(10.0);

        if pending_manual_count > 0 {
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.warning))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
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
                            .fill(p.warning)
                            .corner_radius(CornerRadius::same(8));
                            if ui.add(open_btn).clicked() {
                                self.open_manual_downloads_for(&inst.instance_id);
                            }
                            if ui.button("Scan Now").clicked() {
                                self.run_manual_downloads_scan(&inst.instance_id, true);
                            }
                        });
                    });
                });
            ui.add_space(8.0);
        }

        if let Some(LaunchState::Running { pid, .. }) = self.launch_states.get(&inst.instance_id) {
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.accent))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(12, 7))
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

        // 3. Single Large Floating Translucent Tabbed Panel
        let tab_fade_secs = self.motion_secs(0.15);
        if self.fade_tab != self.active_tab {
            self.fade_tab = self.active_tab;
            self.fade_since = Instant::now();
        }
        let tab_fade = if tab_fade_secs <= 0.0 {
            1.0
        } else {
            (self.fade_since.elapsed().as_secs_f32() / tab_fade_secs).clamp(0.0, 1.0)
        };
        if tab_fade < 1.0 {
            ctx.request_repaint();
        }

        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(ui.available_height());

                let mut switched_tab: Option<DetailTab> = None;
                let tabs = [
                    (
                        DetailTab::Mods,
                        format!("{} ({})", self.t("Mods"), self.selected_mods.len()),
                    ),
                    (
                        DetailTab::Worlds,
                        format!("{} ({})", self.t("Worlds"), self.selected_worlds.len()),
                    ),
                    (DetailTab::ResourcePacks, "Resource packs".to_string()),
                    (DetailTab::Shaders, "Shaders".to_string()),
                    (
                        DetailTab::Screenshots,
                        format!(
                            "{} ({})",
                            self.t("Screenshots"),
                            self.selected_screenshots.len()
                        ),
                    ),
                    (
                        DetailTab::Backups,
                        format!("{} ({})", self.t("Backups"), self.selected_backups.len()),
                    ),
                    (DetailTab::Logs, self.t("navigation.logs").to_string()),
                    (
                        DetailTab::Settings,
                        self.t("navigation.instance_settings").to_string(),
                    ),
                ];

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                    for (tab, label) in &tabs {
                        let active = self.active_tab == *tab;
                        let btn = Button::new(
                            RichText::new(label).size(12.5).strong().color(if active {
                                p.text
                            } else {
                                p.text_muted
                            }),
                        )
                        .fill(if active {
                            with_alpha(p.accent, 64)
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(if active {
                            Stroke::new(1.0_f32, p.accent)
                        } else {
                            Stroke::new(1.0_f32, with_alpha(p.border, 120))
                        })
                        .corner_radius(CornerRadius::same(10));
                        if ui.add(btn).clicked() {
                            switched_tab = Some(*tab);
                        }
                    }
                });
                if let Some(tab) = switched_tab {
                    self.active_tab = tab;
                    self.ensure_active_tab_loaded(ctx);
                }

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(8.0);

                ui.scope_builder(UiBuilder::new(), |ui| {
                    ui.set_opacity(0.45 + 0.55 * tab_fade);
                    match self.active_tab {
                        DetailTab::Mods => self.render_mods_tab(ui, ctx, &inst),
                        DetailTab::Worlds => self.render_worlds_tab(ui, ctx, &inst, busy),
                        DetailTab::ResourcePacks => {
                            self.render_content_pack_tab(ui, ctx, &inst, ContentKind::ResourcePack)
                        }
                        DetailTab::Shaders => {
                            self.render_content_pack_tab(ui, ctx, &inst, ContentKind::ShaderPack)
                        }
                        DetailTab::Screenshots => self.render_screenshots_tab(ui, ctx, &inst),
                        DetailTab::Backups => self.render_backups_tab(ui, ctx, &inst, busy),
                        DetailTab::Logs => self.render_logs_tab(ui, ctx, &inst),
                        DetailTab::Settings => self.render_settings_tab(ui, ctx, &inst),
                    }
                });
            });
    }
}
