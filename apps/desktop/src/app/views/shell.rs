use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_sidebar_context_menu(&mut self, ctx: &egui::Context) {
        let Some((menu_inst_id, pos)) = self.sidebar_context_menu.clone() else {
            return;
        };
        let Some(inst) = self
            .instances
            .iter()
            .find(|i| i.instance_id == menu_inst_id)
            .cloned()
        else {
            self.sidebar_context_menu = None;
            return;
        };

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.sidebar_context_menu = None;
            return;
        }

        let p = self.palette;
        let busy = self.is_instance_busy(&inst.instance_id);
        let is_running = matches!(
            self.launch_states.get(&inst.instance_id),
            Some(LaunchState::Running { .. })
        );

        let launch_lbl = if is_running {
            self.t("Stop Game").to_string()
        } else {
            self.t("workspace.action.launch").to_string()
        };
        let open_folder_lbl = self.t("workspace.action.open_folder").to_string();
        let repair_lbl = self.t("workspace.action.repair").to_string();
        let chg_icon_lbl = self.t("workspace.action.change_icon").to_string();
        let clone_lbl = self.t("workspace.action.clone").to_string();
        let rename_lbl = self.t("instance.rename").to_string();
        let del_lbl = self.t("workspace.action.delete").to_string();

        let mut close_menu = false;
        let mut launch_or_stop = false;
        let mut open_folder = false;
        let mut open_repair = false;
        let mut open_tags = false;
        let mut change_icon = false;
        let mut open_clone = false;
        let mut open_rename = false;
        let mut open_delete = false;

        let area_resp = egui::Area::new(egui::Id::new("mcw_sidebar_context_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.accent))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_min_width(210.0);
                        ui.label(
                            RichText::new(&inst.name)
                                .strong()
                                .size(11.5)
                                .color(p.accent),
                        );
                        ui.separator();

                        if ui
                            .add_enabled(
                                is_running || !busy,
                                Button::new(RichText::new(launch_lbl).strong()),
                            )
                            .clicked()
                        {
                            launch_or_stop = true;
                            close_menu = true;
                        }
                        if ui.button(open_folder_lbl).clicked() {
                            open_folder = true;
                            close_menu = true;
                        }
                        if ui
                            .button(format!("{repair_lbl} / Health Center..."))
                            .clicked()
                        {
                            open_repair = true;
                            close_menu = true;
                        }
                        ui.separator();
                        if ui.button("Group & Tags...").clicked() {
                            open_tags = true;
                            close_menu = true;
                        }
                        if ui.button(chg_icon_lbl).clicked() {
                            change_icon = true;
                            close_menu = true;
                        }
                        if ui.button(clone_lbl).clicked() {
                            open_clone = true;
                            close_menu = true;
                        }
                        if ui.add_enabled(!busy, Button::new(rename_lbl)).clicked() {
                            open_rename = true;
                            close_menu = true;
                        }
                        ui.separator();
                        if ui
                            .add_enabled(!busy, Button::new(RichText::new(del_lbl).color(p.danger)))
                            .clicked()
                        {
                            open_delete = true;
                            close_menu = true;
                        }
                    });
            });

        // Close if user clicks anywhere outside the menu popup
        if !close_menu
            && ctx.input(|i| i.pointer.primary_clicked())
            && !area_resp.response.hovered()
        {
            if let Some(p_pos) = ctx.input(|i| i.pointer.interact_pos()) {
                if !area_resp.response.rect.contains(p_pos) {
                    close_menu = true;
                }
            } else {
                close_menu = true;
            }
        }

        if close_menu {
            self.sidebar_context_menu = None;
        }
        if launch_or_stop {
            if let Some(LaunchState::Running { control, .. }) =
                self.launch_states.get(&inst.instance_id)
            {
                control.request_stop();
            } else {
                self.start_launch(ctx);
            }
        }
        if open_folder {
            let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
            let _ = std::fs::create_dir_all(&mc_dir);
            open_external(&mc_dir.display().to_string());
        }
        if open_repair {
            self.open_repair_center(&inst.instance_id, ctx);
        }
        if open_tags && !self.show_group_tags_modal {
            self.edit_group_input = inst.group.clone();
            self.edit_tags_input = inst.tags.join(", ");
            self.show_group_tags_modal = true;
        }
        if change_icon {
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
        if open_clone && !self.show_clone_modal {
            self.clone_name_input = format!("{} (Copy)", inst.name);
            self.clone_include_saves = false;
            self.show_clone_modal = true;
        }
        if open_rename && !self.show_rename_modal {
            self.rename_input = inst.name.clone();
            self.show_rename_modal = true;
        }
        if open_delete {
            self.show_delete_modal = true;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_single_task_card(
        ui: &mut egui::Ui,
        p: ThemePalette,
        task: &ActiveTask,
        speed_text: &str,
        pause_lbl: &str,
        resume_lbl: &str,
        cancel_lbl: &str,
        paused_badge: &str,
        cancelling_badge: &str,
        motion_secs: f32,
    ) {
        let is_paused = task.control.is_paused();
        let is_cancelled = task.control.is_cancelled();
        let border_col = if is_cancelled {
            p.danger
        } else if is_paused {
            p.warning
        } else {
            p.border
        };
        Frame::new()
            .fill(p.elevated)
            .stroke(Stroke::new(1.0_f32, border_col))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&task.label).strong().size(12.0).color(p.text));
                    if is_cancelled {
                        ui.label(
                            RichText::new(cancelling_badge)
                                .strong()
                                .monospace()
                                .size(10.0)
                                .color(p.danger),
                        );
                    } else if is_paused {
                        ui.label(
                            RichText::new(paused_badge)
                                .strong()
                                .monospace()
                                .size(10.0)
                                .color(p.warning),
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let cancel_btn =
                            Button::new(RichText::new(cancel_lbl).size(10.5).color(p.danger))
                                .small();
                        if ui.add_enabled(!is_cancelled, cancel_btn).clicked() {
                            task.control.cancel();
                        }
                        let pause_text = if is_paused { resume_lbl } else { pause_lbl };
                        let pause_btn =
                            Button::new(RichText::new(pause_text).size(10.5).color(if is_paused {
                                p.accent
                            } else {
                                p.text
                            }))
                            .small();
                        if ui.add_enabled(!is_cancelled, pause_btn).clicked() {
                            task.control.toggle_pause();
                        }

                        let right_text = if task.total > 0 {
                            let pct = ((task.current.min(task.total) as f64 / task.total as f64)
                                * 100.0)
                                .round() as u64;
                            if speed_text.is_empty() || is_paused {
                                format!("{}/{} ({pct}%)", task.current, task.total)
                            } else {
                                format!("{}/{} ({pct}%) · {speed_text}", task.current, task.total)
                            }
                        } else if speed_text.is_empty() || is_paused {
                            if is_paused {
                                paused_badge.to_string()
                            } else {
                                "Working...".to_string()
                            }
                        } else {
                            speed_text.to_string()
                        };
                        ui.label(
                            RichText::new(right_text)
                                .monospace()
                                .size(11.0)
                                .color(if is_paused { p.warning } else { p.accent }),
                        );
                    });
                });
                if !task.message.is_empty() {
                    ui.label(RichText::new(&task.message).size(11.0).color(p.text_muted));
                }
                ui.add_space(3.0);
                let target_fraction = if task.total > 0 {
                    (task.current as f32 / task.total as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let animated_fraction = if task.total > 0 && motion_secs > 0.0 {
                    ui.ctx().animate_value_with_time(
                        egui::Id::new(("mcw_task_prog", task.id)),
                        target_fraction,
                        motion_secs,
                    )
                } else {
                    target_fraction
                };
                let bar = egui::ProgressBar::new(animated_fraction)
                    .desired_height(7.0)
                    .fill(if is_paused { p.warning } else { p.accent })
                    .corner_radius(CornerRadius::same(4))
                    .animate(task.total == 0 && !is_paused && !is_cancelled && motion_secs > 0.0);
                ui.add(bar);
            });
    }

    pub(crate) fn render_tasks_bar(&mut self, ctx: &egui::Context) {
        let total_tasks = self.active_tasks.len();
        if total_tasks <= 2 {
            self.show_all_tasks_modal = false;
        }
        if total_tasks == 0 {
            return;
        }
        let p = self.palette;
        let pause_lbl = self.t("launch.pause_button").to_string();
        let resume_lbl = self.t("launch.resume_button").to_string();
        let cancel_lbl = self.t("launch.cancel_button").to_string();
        let paused_badge = self.t("launch.paused_badge").to_string();
        let cancelling_badge = self.t("launch.cancelling_badge").to_string();
        let motion_secs = self.motion_secs(0.15);
        let speed_text = if self.current_download_bps >= 1.0 {
            format!("↓ {}", format_speed(self.current_download_bps))
        } else {
            String::new()
        };

        egui::TopBottomPanel::bottom("mcw_tasks_bar")
            .frame(
                Frame::new()
                    .fill(p.glass(self.prefs.panel_opacity.max(0.88)).surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("ACTIVE TASKS ({total_tasks})"))
                            .strong()
                            .size(11.0)
                            .color(p.accent),
                    );
                    if !speed_text.is_empty() {
                        ui.label(
                            RichText::new(&speed_text)
                                .monospace()
                                .size(11.0)
                                .color(p.text_muted),
                        );
                    }
                    if total_tasks > 2 {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let hidden = total_tasks - 2;
                            let btn_label = format!("View All ({total_tasks}, +{hidden} more)");
                            if ui.button(btn_label).clicked() {
                                self.show_all_tasks_modal = true;
                            }
                        });
                    }
                });
                ui.add_space(6.0);

                // Show at most 2 representative tasks in the bottom bar; responsive 2-column when wide
                let visible = &self.active_tasks[..total_tasks.min(2)];
                if visible.len() == 2 && ui.available_width() >= 760.0 {
                    ui.columns(2, |cols| {
                        Self::render_single_task_card(
                            &mut cols[0],
                            p,
                            &visible[0],
                            &speed_text,
                            &pause_lbl,
                            &resume_lbl,
                            &cancel_lbl,
                            &paused_badge,
                            &cancelling_badge,
                            motion_secs,
                        );
                        Self::render_single_task_card(
                            &mut cols[1],
                            p,
                            &visible[1],
                            &speed_text,
                            &pause_lbl,
                            &resume_lbl,
                            &cancel_lbl,
                            &paused_badge,
                            &cancelling_badge,
                            motion_secs,
                        );
                    });
                } else {
                    for (idx, task) in visible.iter().enumerate() {
                        Self::render_single_task_card(
                            ui,
                            p,
                            task,
                            &speed_text,
                            &pause_lbl,
                            &resume_lbl,
                            &cancel_lbl,
                            &paused_badge,
                            &cancelling_badge,
                            motion_secs,
                        );
                        if idx + 1 < visible.len() {
                            ui.add_space(4.0);
                        }
                    }
                }
            });

        // In-app centered modal when > 2 tasks are running and user clicked "View All"
        if self.show_all_tasks_modal && total_tasks > 2 {
            let mut open = self.show_all_tasks_modal;
            let tasks = self.active_tasks.clone();
            let notice = self.notice_banner.clone();
            let (keep_open, dismiss_notice) = super::dialogs::render_in_app_modal(
                ctx,
                "mcw_all_tasks_modal",
                &format!("All Active Tasks ({total_tasks})"),
                [560.0, 440.0],
                [360.0, 260.0],
                p,
                notice.as_ref(),
                |ui| {
                    if !speed_text.is_empty() {
                        ui.label(
                            RichText::new(format!("Network Speed: {speed_text}"))
                                .monospace()
                                .size(11.5)
                                .color(p.accent),
                        );
                        ui.add_space(6.0);
                    }
                    ScrollArea::vertical()
                        .id_salt("all_active_tasks_modal_scroll")
                        .show(ui, |ui| {
                            for task in &tasks {
                                Self::render_single_task_card(
                                    ui,
                                    p,
                                    task,
                                    &speed_text,
                                    &pause_lbl,
                                    &resume_lbl,
                                    &cancel_lbl,
                                    &paused_badge,
                                    &cancelling_badge,
                                    motion_secs,
                                );
                                ui.add_space(4.0);
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
            self.show_all_tasks_modal = open;
        }
    }

    pub(crate) fn ensure_instance_icon(&mut self, inst: &Instance, ctx: &egui::Context) {
        let key = format!("{}|{}", inst.instance_id, inst.icon);
        if self.instance_icon_textures.contains_key(&key) {
            return;
        }
        let store = InstanceStore::new(&self.paths);
        let tex = store.resolve_instance_icon_path(inst).and_then(|path| {
            let bytes = std::fs::read(&path).ok()?;
            let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            let color_img = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            Some(ctx.load_texture(
                format!("inst_icon:{key}"),
                color_img,
                egui::TextureOptions::LINEAR,
            ))
        });
        self.instance_icon_textures.insert(key, tex);
    }

    pub(crate) fn render_instance_icon(&self, ui: &mut egui::Ui, inst: &Instance, size: f32) {
        let p = self.palette;
        let key = format!("{}|{}", inst.instance_id, inst.icon);
        if let Some(Some(tex)) = self.instance_icon_textures.get(&key) {
            ui.add(
                egui::Image::new((tex.id(), Vec2::splat(size)))
                    .corner_radius(CornerRadius::same(8)),
            );
            return;
        }
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
        ui.painter().rect(
            rect,
            CornerRadius::same(8),
            p.elevated,
            Stroke::new(1.0_f32, p.border),
            egui::StrokeKind::Inside,
        );
        let initial = inst
            .name
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase().to_string())
            .unwrap_or_else(|| "M".to_string());
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            initial,
            egui::FontId::proportional((size * 0.48).max(11.0)),
            p.accent,
        );
    }
}
