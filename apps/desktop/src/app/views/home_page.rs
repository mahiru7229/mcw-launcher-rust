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
    quick_icon_button_active(ui, p, motion_secs, false, tooltip, paint_icon)
}

fn quick_icon_button_active(
    ui: &mut egui::Ui,
    p: ThemePalette,
    motion_secs: f32,
    active: bool,
    tooltip: &str,
    paint_icon: impl FnOnce(&egui::Painter, egui::Rect, Color32),
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::click());
    let t = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hover"), resp.hovered(), motion_secs);
    let fill = if active {
        with_alpha(p.accent, 64)
    } else if t > 0.01 {
        p.elevated
    } else {
        p.surface
    };
    let stroke = if active || resp.hovered() {
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
    let icon_col = if active { p.accent } else { p.text };
    paint_icon(ui.painter(), rect, icon_col);
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

                if self.topmost_notice_modal().is_none() {
                    if let Some((msg, is_err)) = self.notice_banner.clone() {
                        if super::dialogs::render_notice_banner_box(ui, p, &msg, is_err) {
                            self.notice_banner = None;
                        }
                        ui.add_space(8.0);
                    }
                }

                match self.active_page {
                    AppPage::Home => self.render_home_page(ui, ctx),
                    AppPage::Modpacks => self.render_modpacks_page(ui, ctx),
                    AppPage::Versions => self.render_versions_page(ui, ctx),
                    AppPage::Logs => self.render_logs_page(ui, ctx),
                    AppPage::Changelog => self.render_changelog_page(ui, ctx),
                }
            });
    }

    fn render_home_library_grid(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use std::collections::BTreeMap;

        let p = self.palette;
        let q = self.home_grid_search.trim().to_ascii_lowercase();
        let filtered: Vec<Instance> = self
            .instances
            .iter()
            .filter(|inst| {
                if q.is_empty() {
                    return true;
                }
                inst.name.to_ascii_lowercase().contains(&q)
                    || inst.version_id.to_ascii_lowercase().contains(&q)
                    || inst.mod_loader.name.to_ascii_lowercase().contains(&q)
                    || inst.group.to_ascii_lowercase().contains(&q)
                    || inst
                        .tags
                        .iter()
                        .any(|t| t.to_ascii_lowercase().contains(&q))
            })
            .cloned()
            .collect();

        let sorted = sort_instances_for_grid(&filtered, &self.prefs.home_sort_by);
        for inst in &sorted {
            self.ensure_instance_icon(inst, ctx);
        }

        if sorted.is_empty() {
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::same(24))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No instances match your current filter.")
                                .color(p.text_muted),
                        );
                    });
                });
            return;
        }

        // Order named groups alphabetically first, followed by Ungrouped
        let mut grouped: BTreeMap<(u8, String), Vec<Instance>> = BTreeMap::new();
        for inst in sorted {
            let trimmed = inst.group.trim();
            let key = if trimmed.is_empty() {
                (1u8, "Ungrouped".to_string())
            } else {
                (0u8, trimmed.to_string())
            };
            grouped.entry(key).or_default().push(inst);
        }

        let mut launch_instance_id: Option<String> = None;
        let mut stop_instance_id: Option<String> = None;
        let mut open_instance_id: Option<String> = None;
        let mut manual_dl_instance_id: Option<String> = None;

        ScrollArea::vertical()
            .id_salt("home_library_grid_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail_w = ui.available_width().max(240.0);
                let cols = (((avail_w + 10.0) / 260.0).floor() as usize).clamp(1, 5);
                let gap = 10.0;
                let card_w =
                    ((avail_w - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(200.0);

                for ((_, group_name), items) in &grouped {
                    let is_collapsed = self.collapsed_home_groups.contains(group_name);
                    let arrow = if is_collapsed { "▸" } else { "▾" };
                    let hdr_lbl = format!("{arrow} {group_name} ({})", items.len());
                    if ui
                        .add(
                            Button::new(RichText::new(hdr_lbl).strong().size(13.0).color(p.text))
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::NONE),
                        )
                        .clicked()
                    {
                        if is_collapsed {
                            self.collapsed_home_groups.remove(group_name);
                        } else {
                            self.collapsed_home_groups.insert(group_name.clone());
                        }
                    }

                    if is_collapsed {
                        ui.add_space(4.0);
                        continue;
                    }
                    ui.add_space(4.0);

                    for row in items.chunks(cols) {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for inst in row {
                                let is_sel = self.selected_instance_id.as_deref()
                                    == Some(inst.instance_id.as_str());
                                let pending_manual = self
                                    .pending_manual_counts
                                    .get(&inst.instance_id)
                                    .copied()
                                    .unwrap_or(0);
                                let is_running = matches!(
                                    self.launch_states.get(&inst.instance_id),
                                    Some(LaunchState::Running { .. })
                                );
                                let is_preparing = matches!(
                                    self.launch_states.get(&inst.instance_id),
                                    Some(LaunchState::Preparing(_))
                                );
                                let (status_lbl, status_col) = if is_running {
                                    ("Running".to_string(), p.accent)
                                } else if is_preparing {
                                    ("Preparing".to_string(), p.warning)
                                } else if pending_manual > 0 {
                                    (format!("Missing ({pending_manual})"), p.warning)
                                } else if inst.last_launch_state == "crashed" {
                                    ("Crashed".to_string(), p.danger)
                                } else {
                                    ("Ready".to_string(), p.accent)
                                };

                                ui.vertical(|ui| {
                                    Frame::new()
                                        .fill(p.surface)
                                        .stroke(Stroke::new(
                                            if is_sel { 1.5_f32 } else { 1.0_f32 },
                                            if is_sel { p.accent } else { p.border },
                                        ))
                                        .corner_radius(CornerRadius::same(12))
                                        .inner_margin(Margin::same(12))
                                        .show(ui, |ui| {
                                            ui.set_width((card_w - 26.0).max(174.0));
                                            ui.set_min_height(108.0);
                                            ui.horizontal(|ui| {
                                                self.render_instance_icon(ui, inst, 38.0);
                                                ui.add_space(6.0);
                                                ui.vertical(|ui| {
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(&inst.name)
                                                                .strong()
                                                                .size(13.5)
                                                                .color(p.text),
                                                        )
                                                        .truncate(),
                                                    );
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(format!(
                                                                "MC {} · {}",
                                                                inst.version_id,
                                                                display_loader_name(
                                                                    &inst.mod_loader.name
                                                                )
                                                            ))
                                                            .monospace()
                                                            .size(11.0)
                                                            .color(p.text_muted),
                                                        )
                                                        .truncate(),
                                                    );
                                                });
                                            });
                                            ui.add_space(6.0);
                                            ui.horizontal_wrapped(|ui| {
                                                let (dot_r, _) = ui.allocate_exact_size(
                                                    Vec2::splat(8.0),
                                                    Sense::hover(),
                                                );
                                                ui.painter().circle_filled(
                                                    dot_r.center(),
                                                    3.5,
                                                    status_col,
                                                );
                                                ui.label(
                                                    RichText::new(&status_lbl)
                                                        .size(11.0)
                                                        .color(p.text_muted),
                                                );
                                                ui.label(
                                                    RichText::new(format!(
                                                        "· {}",
                                                        format_duration(
                                                            inst.total_playtime_seconds
                                                        )
                                                    ))
                                                    .size(11.0)
                                                    .color(p.text_muted),
                                                );
                                            });
                                            ui.add_space(8.0);
                                            ui.horizontal_wrapped(|ui| {
                                                if is_running {
                                                    if ui
                                                        .add(
                                                            Button::new(
                                                                RichText::new("Stop")
                                                                    .color(Color32::WHITE)
                                                                    .size(11.5)
                                                                    .strong(),
                                                            )
                                                            .fill(p.danger)
                                                            .corner_radius(CornerRadius::same(7)),
                                                        )
                                                        .clicked()
                                                    {
                                                        stop_instance_id =
                                                            Some(inst.instance_id.clone());
                                                    }
                                                } else if is_preparing {
                                                    ui.add_enabled(
                                                        false,
                                                        Button::new(
                                                            RichText::new("Preparing...")
                                                                .size(11.5),
                                                        )
                                                        .corner_radius(CornerRadius::same(7)),
                                                    );
                                                } else if pending_manual > 0 {
                                                    if ui
                                                        .add(
                                                            Button::new(
                                                                RichText::new("Missing Mods")
                                                                    .color(Color32::BLACK)
                                                                    .size(11.5)
                                                                    .strong(),
                                                            )
                                                            .fill(p.warning)
                                                            .corner_radius(CornerRadius::same(7)),
                                                        )
                                                        .clicked()
                                                    {
                                                        manual_dl_instance_id =
                                                            Some(inst.instance_id.clone());
                                                    }
                                                } else if ui
                                                    .add(
                                                        Button::new(
                                                            RichText::new("Play")
                                                                .color(p.accent_text)
                                                                .size(11.5)
                                                                .strong(),
                                                        )
                                                        .fill(p.accent)
                                                        .corner_radius(CornerRadius::same(7)),
                                                    )
                                                    .clicked()
                                                {
                                                    launch_instance_id =
                                                        Some(inst.instance_id.clone());
                                                }
                                                if ui
                                                    .add(
                                                        Button::new(
                                                            RichText::new("Open").size(11.5),
                                                        )
                                                        .corner_radius(CornerRadius::same(7)),
                                                    )
                                                    .clicked()
                                                {
                                                    open_instance_id =
                                                        Some(inst.instance_id.clone());
                                                }
                                            });
                                        });
                                });
                            }
                        });
                        ui.add_space(gap);
                    }
                    ui.add_space(6.0);
                }
            });

        if let Some(id) = stop_instance_id {
            if let Some(LaunchState::Running { control, .. }) = self.launch_states.get(&id) {
                control.request_stop();
            }
        } else if let Some(id) = launch_instance_id {
            if self.selected_instance_id.as_deref() != Some(id.as_str()) {
                self.selected_instance_id = Some(id);
                self.sync_selected_from_cache();
            }
            self.start_launch(ctx);
        } else if let Some(id) = manual_dl_instance_id {
            self.open_manual_downloads_for(&id, ctx);
        } else if let Some(id) = open_instance_id {
            self.prefs.home_view_mode = "workspace".to_string();
            self.save_prefs();
            self.request_instance_switch(id, ctx);
        }
    }

    fn render_home_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.palette;
        if !self.instances.is_empty() {
            let mut prefs_dirty = false;
            let mut open_create_grid = false;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 4.0);
                let is_grid = self.prefs.home_view_mode == "grid";
                let mode_lbl = if is_grid {
                    "View: Library Grid"
                } else {
                    "View: Workspace"
                };
                let mut picked_mode = if is_grid {
                    "grid".to_string()
                } else {
                    "workspace".to_string()
                };
                egui::ComboBox::from_id_salt("cb_home_view_mode")
                    .selected_text(mode_lbl)
                    .width(156.0)
                    .truncate()
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut picked_mode,
                            "workspace".to_string(),
                            "Workspace View",
                        );
                        ui.selectable_value(&mut picked_mode, "grid".to_string(), "Library Grid");
                    });
                if picked_mode != self.prefs.home_view_mode {
                    self.prefs.home_view_mode = picked_mode;
                    prefs_dirty = true;
                }

                if self.prefs.home_view_mode == "grid" {
                    ui.add(
                        TextEdit::singleline(&mut self.home_grid_search)
                            .hint_text("Filter instances...")
                            .desired_width(200.0),
                    );
                    let sort_lbl = match self.prefs.home_sort_by.as_str() {
                        "playtime" => "Sort: Playtime",
                        "name" => "Sort: Name",
                        _ => "Sort: Last Played",
                    };
                    let mut picked_sort = match self.prefs.home_sort_by.as_str() {
                        "playtime" => "playtime".to_string(),
                        "name" => "name".to_string(),
                        _ => "last_played".to_string(),
                    };
                    egui::ComboBox::from_id_salt("cb_home_sort_by")
                        .selected_text(sort_lbl)
                        .width(152.0)
                        .truncate()
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut picked_sort,
                                "last_played".to_string(),
                                "Sort: Last Played",
                            );
                            ui.selectable_value(
                                &mut picked_sort,
                                "playtime".to_string(),
                                "Sort: Playtime",
                            );
                            ui.selectable_value(&mut picked_sort, "name".to_string(), "Sort: Name");
                        });
                    if picked_sort != self.prefs.home_sort_by {
                        self.prefs.home_sort_by = picked_sort;
                        prefs_dirty = true;
                    }
                    if ui
                        .add(
                            Button::new(
                                RichText::new("+ Create Instance")
                                    .color(p.accent_text)
                                    .strong(),
                            )
                            .fill(p.accent)
                            .corner_radius(CornerRadius::same(8)),
                        )
                        .clicked()
                    {
                        open_create_grid = true;
                    }
                }
            });
            if prefs_dirty {
                self.save_prefs();
            }
            if open_create_grid {
                self.open_create_instance_dialog(ctx);
            }
            ui.add_space(6.0);
            if self.prefs.home_view_mode == "grid" {
                self.render_home_library_grid(ui, ctx);
                return;
            }
        }
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
                            self.open_create_instance_dialog(ctx);
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
                let note_preview = self
                    .edit_instance_notes_text
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .map(str::trim)
                    .unwrap_or("");
                if !note_preview.is_empty() {
                    ui.add_space(4.0);
                    let clipped = if note_preview.chars().count() > 80 {
                        let head: String = note_preview.chars().take(77).collect();
                        format!("Note: {head}...")
                    } else {
                        format!("Note: {note_preview}")
                    };
                    ui.label(
                        RichText::new(clipped)
                            .size(11.5)
                            .italics()
                            .color(p.text_muted),
                    );
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
                                "MC {} - {}",
                                inst.version_id,
                                display_loader_name(&inst.mod_loader.name)
                            ))
                            .monospace()
                            .size(12.0)
                            .color(p.text_muted),
                        );
                    });
            });
        });

        // Security assessment pill + clear human-readable description (never bare status alone)
        let sec = self.assess_instance_security(&inst);
        let (sec_bg, sec_fg) = match sec.status {
            SecurityStatus::Protected => (
                Color32::from_rgba_unmultiplied(p.accent.r(), p.accent.g(), p.accent.b(), 26),
                p.accent,
            ),
            SecurityStatus::PartiallyProtected => (
                Color32::from_rgba_unmultiplied(p.warning.r(), p.warning.g(), p.warning.b(), 28),
                p.warning,
            ),
            SecurityStatus::Unprotected => (
                Color32::from_rgba_unmultiplied(p.danger.r(), p.danger.g(), p.danger.b(), 30),
                p.danger,
            ),
            SecurityStatus::Unknown => (p.surface, p.text_muted),
        };
        ui.add_space(6.0);
        let mut sync_verify_clicked = false;
        let syncing_sec = self.security_sync_busy || self.security_policy_refreshing;
        Frame::new()
            .fill(sec_bg)
            .stroke(Stroke::new(1.0_f32, sec_fg.gamma_multiply(0.75)))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        let reserve = if sec.status == SecurityStatus::Protected {
                            0.0
                        } else {
                            132.0
                        };
                        ui.set_max_width((ui.available_width() - reserve).max(160.0));
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(sec.status.display_title())
                                    .strong()
                                    .size(11.5)
                                    .color(sec_fg),
                            );
                            ui.label(
                                RichText::new(format!("— {}", sec.description))
                                    .size(11.5)
                                    .color(p.text),
                            );
                        });
                    });
                    if sec.status != SecurityStatus::Protected {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let btn_lbl = if syncing_sec {
                                "Verifying..."
                            } else {
                                "Sync & Verify"
                            };
                            let btn = Button::new(RichText::new(btn_lbl).size(11.5).strong())
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, sec_fg.gamma_multiply(0.8)))
                                .corner_radius(CornerRadius::same(8));
                            if ui
                                .add_enabled(!syncing_sec, btn)
                                .on_hover_text(
                                    "Refresh Signed Policy, sync official Mojang FreeType checksums, and re-verify this instance.",
                                )
                                .clicked()
                            {
                                sync_verify_clicked = true;
                            }
                        });
                    }
                });
            });
        if sync_verify_clicked {
            self.start_sync_and_verify_security(&inst, ctx);
        }

        ui.add_space(12.0);

        // 2. Hero Action Bar: Play + `...` Actions menu + Quick Icon Buttons
        let mut trigger_repair = false;
        let mut open_repair_center = false;
        let mut open_modpack_update = false;
        let mut open_content_library = false;
        let mut open_export = false;
        let mut create_shortcut = false;
        let mut export_loader_diag = false;
        let mut change_icon_clicked = false;
        let mut reset_icon_clicked = false;
        let mut open_content_cat: Option<BrowserCategory> = None;
        let has_official_modpack = tracked_pack.is_some();

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
                        self.open_manual_downloads_for(&inst.instance_id, ctx);
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
                    let resp = ui.add(launch_btn);
                    {
                        // Draw small crisp play triangle inside the left side of the Play button
                        let r = resp.rect;
                        let cx = r.left() + 20.0;
                        let cy = r.center().y;
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![
                                egui::pos2(cx - 4.0, cy - 5.5),
                                egui::pos2(cx + 5.5, cy),
                                egui::pos2(cx - 4.0, cy + 5.5),
                            ],
                            p.accent_text,
                            Stroke::NONE,
                        ));
                    }
                    if resp.clicked() {
                        self.start_launch(ctx);
                    } else if !has_account {
                        resp.on_hover_text(self.t("accounts.auth_required_notice"));
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
                            if ui.button("Create Desktop Shortcut").clicked() {
                                create_shortcut = true;
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
                            if has_official_modpack
                                && ui
                                    .add_enabled(!busy, Button::new("Check Modpack Update..."))
                                    .on_disabled_hover_text("Stop the instance before updating")
                                    .clicked()
                            {
                                open_modpack_update = true;
                                ui.close_menu();
                            }
                            if ui.button("Installed Content Library...").clicked() {
                                open_content_library = true;
                                ui.close_menu();
                            }
                            if is_diag_loader && ui.button(&export_diag_lbl).clicked() {
                                export_loader_diag = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Edit Group & Tags...").clicked() {
                                if !self.show_group_tags_modal {
                                    self.edit_group_input = inst.group.clone();
                                    self.edit_tags_input = inst.tags.join(", ");
                                    self.show_group_tags_modal = true;
                                }
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
                                if !self.show_rename_modal {
                                    self.rename_input = inst.name.clone();
                                    self.show_rename_modal = true;
                                }
                                ui.close_menu();
                            }
                            if ui.button(&clone_lbl).clicked() {
                                if !self.show_clone_modal {
                                    self.clone_name_input = format!("{} (Copy)", inst.name);
                                    self.clone_include_saves = false;
                                    self.show_clone_modal = true;
                                }
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

            // Quick icon: Instance Settings (8-tooth gear vector)
            let settings_active = self.active_tab == DetailTab::Settings;
            if quick_icon_button_active(
                ui,
                p,
                motion_hover,
                settings_active,
                "Instance Settings",
                |painter, r, col| {
                    let c = r.center();
                    painter.circle_stroke(c, 5.4, Stroke::new(2.0_f32, col));
                    painter.circle_stroke(c, 2.1, Stroke::new(1.5_f32, col));
                    for i in 0..8 {
                        let a = (i as f32) * std::f32::consts::FRAC_PI_4;
                        let p1 = c + Vec2::new(a.cos() * 6.0, a.sin() * 6.0);
                        let p2 = c + Vec2::new(a.cos() * 8.6, a.sin() * 8.6);
                        painter.line_segment([p1, p2], Stroke::new(2.0_f32, col));
                    }
                },
            )
            .clicked()
            {
                if settings_active {
                    let fallback = if inst.mod_loader.is_vanilla() {
                        DetailTab::Worlds
                    } else {
                        DetailTab::Mods
                    };
                    self.request_tab_switch(fallback, ctx);
                } else {
                    self.request_tab_switch(DetailTab::Settings, ctx);
                }
            }
        });

        if trigger_repair {
            self.start_repair(ctx);
        }
        if open_repair_center {
            self.open_repair_center(&inst.instance_id, ctx);
        }
        if open_modpack_update {
            self.open_modpack_update_window(inst.instance_id.clone(), ctx);
        }
        if open_content_library {
            self.open_content_library_window(ctx);
        }
        if open_export {
            self.open_export_modpack_window(ctx);
        }
        if create_shortcut {
            self.handle_create_target_shortcut(&inst, None, None);
        }
        if export_loader_diag {
            self.export_loader_diagnostics(&inst);
        }
        if let Some(cat) = open_content_cat {
            self.open_content_browser_mode(cat, None, false, ctx);
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
                                self.open_manual_downloads_for(&inst.instance_id, ctx);
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
                            RichText::new(format!("PID {pid} - streaming live logs"))
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

                let is_vanilla = inst.mod_loader.is_vanilla();
                if is_vanilla && matches!(self.active_tab, DetailTab::Mods | DetailTab::Shaders) {
                    self.active_tab = DetailTab::Worlds;
                    self.ensure_active_tab_loaded(ctx);
                }
                if self.revealed_tab_counts_instance.as_deref() != Some(inst.instance_id.as_str()) {
                    self.revealed_tab_counts.clear();
                    self.revealed_tab_counts_instance = Some(inst.instance_id.clone());
                }
                match self.active_tab {
                    DetailTab::Mods if !self.loading_mods.contains(&inst.instance_id) => {
                        self.revealed_tab_counts
                            .insert(DetailTab::Mods, self.selected_mods.len());
                    }
                    DetailTab::Worlds if !self.loading_worlds.contains(&inst.instance_id) => {
                        self.revealed_tab_counts
                            .insert(DetailTab::Worlds, self.selected_worlds.len());
                    }
                    DetailTab::ResourcePacks
                        if !self
                            .loading_content_packs
                            .contains(&(inst.instance_id.clone(), ContentKind::ResourcePack)) =>
                    {
                        self.revealed_tab_counts
                            .insert(DetailTab::ResourcePacks, self.selected_content_packs.len());
                    }
                    DetailTab::Shaders
                        if !self
                            .loading_content_packs
                            .contains(&(inst.instance_id.clone(), ContentKind::ShaderPack)) =>
                    {
                        self.revealed_tab_counts
                            .insert(DetailTab::Shaders, self.selected_content_packs.len());
                    }
                    DetailTab::Screenshots
                        if !self.loading_screenshots.contains(&inst.instance_id) =>
                    {
                        self.revealed_tab_counts
                            .insert(DetailTab::Screenshots, self.selected_screenshots.len());
                    }
                    DetailTab::Backups if !self.loading_backups.contains(&inst.instance_id) => {
                        self.revealed_tab_counts
                            .insert(DetailTab::Backups, self.selected_backups.len());
                    }
                    _ => {}
                }

                let mut switched_tab: Option<DetailTab> = None;
                let tab_lbl = |base: String, tab: DetailTab| -> String {
                    if let Some(count) = self.revealed_tab_counts.get(&tab) {
                        format!("{base} ({count})")
                    } else {
                        base
                    }
                };
                let mut tabs: Vec<(DetailTab, String)> = Vec::new();
                if !is_vanilla {
                    tabs.push((
                        DetailTab::Mods,
                        tab_lbl(self.t("Mods").to_string(), DetailTab::Mods),
                    ));
                }
                tabs.push((
                    DetailTab::Worlds,
                    tab_lbl(self.t("Worlds").to_string(), DetailTab::Worlds),
                ));
                tabs.push((
                    DetailTab::ResourcePacks,
                    tab_lbl("Resource packs".to_string(), DetailTab::ResourcePacks),
                ));
                if !is_vanilla {
                    tabs.push((
                        DetailTab::Shaders,
                        tab_lbl("Shaders".to_string(), DetailTab::Shaders),
                    ));
                }
                tabs.push((
                    DetailTab::Screenshots,
                    tab_lbl(self.t("Screenshots").to_string(), DetailTab::Screenshots),
                ));
                tabs.push((
                    DetailTab::Backups,
                    tab_lbl(self.t("Backups").to_string(), DetailTab::Backups),
                ));
                tabs.push((DetailTab::Logs, self.t("navigation.logs").to_string()));
                tabs.push((DetailTab::Servers, "Servers & LAN".to_string()));

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
                    self.request_tab_switch(tab, ctx);
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
                        DetailTab::Servers => self.render_servers_tab(ui, ctx, &inst),
                        DetailTab::Settings => self.render_settings_tab(ui, ctx, &inst),
                    }
                });
            });
    }
}
