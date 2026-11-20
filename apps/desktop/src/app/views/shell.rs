use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    /// Instance list (search, favorites, group/tag chips) shown inside the instance flyout.
    pub(crate) fn render_instance_list(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        max_h: f32,
    ) {
        let p = self.palette;
        ui.label(
            RichText::new(format!("INSTANCES ({})", self.instances.len()))
                .strong()
                .size(11.5)
                .color(p.text_muted),
        );
        ui.add_space(4.0);
        let new_inst_lbl = format!("+ {}", self.t("workspace.create.title"));
        let new_btn = Button::new(RichText::new(new_inst_lbl).color(p.accent_text).strong())
            .fill(p.accent)
            .min_size(Vec2::new(ui.available_width(), 28.0))
            .corner_radius(CornerRadius::same(3));
        if ui.add(new_btn).clicked() {
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

        ui.add_space(6.0);
        let search_hint = self.t("workspace.library.search_placeholder").to_string();
        ui.add(
            TextEdit::singleline(&mut self.sidebar_search_query)
                .hint_text(search_hint)
                .desired_width(ui.available_width()),
        );
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        if self.instances.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(
                    RichText::new("No instances yet")
                        .strong()
                        .color(p.text_muted),
                );
                ui.label(
                    RichText::new("Create an instance to get started.")
                        .size(12.0)
                        .color(p.text_muted),
                );
            });
            return;
        }

        // Ensure local icons are loaded for all instances so they display in sidebar & header
        let inst_snapshots: Vec<Instance> = self.instances.clone();
        for inst_snap in &inst_snapshots {
            self.ensure_instance_icon(inst_snap, ctx);
        }

        let q = self.sidebar_search_query.trim().to_ascii_lowercase();
        let mut newly_selected: Option<String> = None;
        let mut toggle_fav: Option<(String, bool)> = None;
        ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
            for inst in &self.instances {
                if !q.is_empty() {
                    let match_name = inst.name.to_ascii_lowercase().contains(&q);
                    let match_ver = inst.version_id.to_ascii_lowercase().contains(&q);
                    let match_grp = inst.group.to_ascii_lowercase().contains(&q);
                    let match_tag = inst
                        .tags
                        .iter()
                        .any(|t| t.to_ascii_lowercase().contains(&q));
                    if !(match_name || match_ver || match_grp || match_tag) {
                        continue;
                    }
                }
                let is_selected =
                    self.selected_instance_id.as_deref() == Some(inst.instance_id.as_str());
                let missing_manual = self
                    .pending_manual_counts
                    .get(&inst.instance_id)
                    .copied()
                    .unwrap_or(0);
                let (status_text, status_color, is_running) = match self
                    .launch_states
                    .get(&inst.instance_id)
                {
                    Some(LaunchState::Running { .. }) => ("Running".to_string(), p.accent, true),
                    Some(LaunchState::Preparing(_)) => ("Preparing".to_string(), p.warning, true),
                    _ if missing_manual > 0 => {
                        (format!("Incomplete ({missing_manual})"), p.warning, false)
                    }
                    _ => match inst.last_launch_state.as_str() {
                        "crashed" => ("Crashed".to_string(), p.danger, false),
                        "finished" => ("Finished".to_string(), p.text_muted, false),
                        _ => ("Ready".to_string(), p.text_muted, false),
                    },
                };

                let bg = if is_selected { p.elevated } else { p.surface };
                let border_col = if is_selected {
                    p.accent
                } else if missing_manual > 0 {
                    p.warning
                } else {
                    p.border
                };

                let mut star_clicked = false;
                let frame_resp = Frame::new()
                    .fill(bg)
                    .stroke(Stroke::new(1.0_f32, border_col))
                    .corner_radius(CornerRadius::same(3))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            // Signature Moment #1: Left vertical accent bar (Emerald or Warning if incomplete)
                            if is_selected || is_running || missing_manual > 0 {
                                let bar_col = if is_running {
                                    p.accent
                                } else if missing_manual > 0 && !is_selected {
                                    p.warning
                                } else {
                                    p.accent
                                };
                                let (rect, _) = ui.allocate_exact_size(
                                    Vec2::new(3.0, 32.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .rect_filled(rect, CornerRadius::same(1), bar_col);
                            }
                            self.render_instance_icon(ui, inst, 28.0);
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());
                                // Row 1: Star + Instance Name (left) + Status (right)
                                ui.horizontal(|ui| {
                                    if star_icon_button(ui, p, inst.favorite)
                                        .on_hover_text("Toggle favorite")
                                        .clicked()
                                    {
                                        star_clicked = true;
                                        toggle_fav =
                                            Some((inst.instance_id.clone(), !inst.favorite));
                                    }
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&inst.name)
                                                .strong()
                                                .size(13.5)
                                                .color(p.text),
                                        )
                                        .truncate(),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(status_text)
                                                .size(10.5)
                                                .color(status_color),
                                        );
                                    });
                                });
                                // Row 2: Minecraft version · Loader
                                ui.label(
                                    RichText::new(format!(
                                        "{} · {}",
                                        inst.version_id, inst.mod_loader.name
                                    ))
                                    .monospace()
                                    .size(11.0)
                                    .color(p.text_muted),
                                );
                                // Row 3: Responsive auto-sized Group & Tag pill badges (wraps automatically)
                                let has_group = !inst.group.trim().is_empty();
                                let has_tags = inst.tags.iter().any(|t| !t.trim().is_empty());
                                if has_group || has_tags {
                                    ui.add_space(2.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
                                        if has_group {
                                            render_badge_pill(ui, &inst.group, true, true, p);
                                        }
                                        for tag in &inst.tags {
                                            render_badge_pill(ui, tag, false, true, p);
                                        }
                                    });
                                }
                            });
                        });
                    });

                let card_hovered = ui.rect_contains_pointer(frame_resp.response.rect);
                let card_clicked =
                    !star_clicked && card_hovered && ui.input(|i| i.pointer.primary_clicked());
                if card_clicked {
                    newly_selected = Some(inst.instance_id.clone());
                    self.sidebar_context_menu = None;
                }
                let right_clicked =
                    !star_clicked && card_hovered && ui.input(|i| i.pointer.secondary_clicked());
                if right_clicked {
                    if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                        newly_selected = Some(inst.instance_id.clone());
                        self.sidebar_context_menu = Some((inst.instance_id.clone(), pos));
                    }
                }
                ui.add_space(4.0);
            }
        });

        if let Some((fav_id, new_fav)) = toggle_fav {
            let store = InstanceStore::new(&self.paths);
            if store
                .set_library_metadata(&fav_id, Some(new_fav), None, None)
                .is_ok()
            {
                self.reload_instances();
            }
        }
        if let Some(id) = newly_selected {
            self.selected_instance_id = Some(id);
            if self.active_page == AppPage::Versions {
                self.active_page = AppPage::Home;
            }
            self.sync_selected_from_cache();
            self.ensure_active_tab_loaded(ctx);
        }
    }

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
        let has_account = self.active_account().is_some();

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
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(egui::Margin::same(6))
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
                                is_running || (!busy && has_account),
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
        if open_tags {
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
        if open_clone {
            self.clone_name_input = format!("{} (Copy)", inst.name);
            self.clone_include_saves = false;
            self.show_clone_modal = true;
        }
        if open_rename {
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
            .corner_radius(CornerRadius::same(3))
            .inner_margin(egui::Margin::symmetric(8, 6))
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
                let fraction = if task.total > 0 {
                    (task.current as f32 / task.total as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let bar = egui::ProgressBar::new(fraction)
                    .desired_height(6.0)
                    .fill(if is_paused { p.warning } else { p.accent })
                    .corner_radius(CornerRadius::same(2))
                    .animate(task.total == 0 && !is_paused && !is_cancelled);
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
        let speed_text = if self.current_download_bps >= 1.0 {
            format!("↓ {}", format_speed(self.current_download_bps))
        } else {
            String::new()
        };

        egui::TopBottomPanel::bottom("mcw_tasks_bar")
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .inner_margin(egui::Margin::symmetric(12, 8)),
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
                                self.show_all_tasks_modal = !self.show_all_tasks_modal;
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
                        );
                        if idx + 1 < visible.len() {
                            ui.add_space(4.0);
                        }
                    }
                }
            });

        // Overflow isolated OS window when > 2 tasks are running and user clicked "View All"
        if self.show_all_tasks_modal && total_tasks > 2 {
            let mut open = self.show_all_tasks_modal;
            let vp_id = egui::ViewportId::from_hash_of("mcw_vp_all_tasks");
            let builder = centered_viewport_builder(
                ctx,
                format!("All Active Tasks ({total_tasks}) — MCW Launcher"),
                [560.0, 420.0],
                [380.0, 260.0],
            );
            ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                if vp_ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
                egui::CentralPanel::default()
                    .frame(
                        Frame::new()
                            .fill(p.canvas)
                            .inner_margin(egui::Margin::same(12)),
                    )
                    .show(vp_ctx, |ui| {
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
                                for task in &self.active_tasks {
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
                                    );
                                    ui.add_space(4.0);
                                }
                            });
                    });
            });
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
                    .corner_radius(CornerRadius::same(4)),
            );
            return;
        }
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
        ui.painter().rect(
            rect,
            CornerRadius::same(4),
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
