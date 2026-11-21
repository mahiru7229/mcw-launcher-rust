use crate::app::platform::*;
use crate::app::*;

enum PreflightChoice {
    Once,
    AlwaysAllow,
    Cancel,
}

enum UnsavedChoice {
    SaveAndContinue,
    DiscardAndContinue,
    Cancel,
}

impl McwApp {
    pub(crate) fn render_stage6_modals(&mut self, ctx: &egui::Context) {
        self.render_content_library_window(ctx);
        self.render_modpack_update_window(ctx);
        self.render_preflight_confirm_window(ctx);
        self.render_issue_report_window(ctx);
        self.render_theme_issues_window(ctx);
        self.render_unsaved_changes_window(ctx);
    }

    pub(crate) fn render_content_library_window(&mut self, ctx: &egui::Context) {
        if !self.show_content_library_window {
            return;
        }
        let p = self.palette;
        let mut open = self.show_content_library_window;
        let mut refresh_clicked = false;
        let mut toggle_pin: Option<(String, String, bool)> = None;

        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_content_library");
        let builder = centered_viewport_builder(
            ctx,
            "Installed Content Library - MCW Launcher",
            [860.0, 560.0],
            [640.0, 420.0],
        );

        let instances_snapshot = self.instances.clone();
        let items_snapshot = self.content_library_items.clone();

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx
                .input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
            {
                open = false;
            }
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Installed Content Library")
                                    .size(18.0)
                                    .strong()
                                    .color(p.text),
                            );
                            ui.label(
                                RichText::new("Inspect, filter, and pin installed mods, resource packs, and shaders across all instances.")
                                    .size(11.5)
                                    .color(p.text_muted),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(self.t("common.close")).clicked() {
                                open = false;
                            }
                            if ui
                                .add_enabled(!self.content_library_loading, Button::new("Refresh"))
                                .clicked()
                            {
                                refresh_clicked = true;
                            }
                        });
                    });

                    ui.add_space(8.0);
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.add(
                                    TextEdit::singleline(&mut self.content_library_search)
                                        .hint_text("Filter by name, filename, or provider...")
                                        .desired_width(220.0),
                                );

                                let kind_disp = match self.content_library_kind_filter.as_str() {
                                    "mod" => "Mods",
                                    "resourcepack" => "Resource packs",
                                    "shaderpack" => "Shader packs",
                                    "modpack" => "Modpacks",
                                    _ => "All Kinds",
                                };
                                egui::ComboBox::from_id_salt("cl_kind_filter_combo")
                                    .selected_text(kind_disp)
                                    .width(auto_combo_width(ui, kind_disp, 110.0, 165.0))
                                    .show_ui(ui, |ui| {
                                        for (val, lbl) in [
                                            ("all", "All Kinds"),
                                            ("mod", "Mods"),
                                            ("resourcepack", "Resource packs"),
                                            ("shaderpack", "Shader packs"),
                                            ("modpack", "Modpacks"),
                                        ] {
                                            ui.selectable_value(
                                                &mut self.content_library_kind_filter,
                                                val.to_string(),
                                                lbl,
                                            );
                                        }
                                    });

                                let inst_disp = if self.content_library_inst_filter == "all" {
                                    "All Instances".to_string()
                                } else {
                                    instances_snapshot
                                        .iter()
                                        .find(|i| i.instance_id == self.content_library_inst_filter)
                                        .map(|i| i.name.clone())
                                        .unwrap_or_else(|| self.content_library_inst_filter.clone())
                                };
                                egui::ComboBox::from_id_salt("cl_inst_filter_combo")
                                    .selected_text(&inst_disp)
                                    .width(auto_combo_width(ui, &inst_disp, 130.0, 210.0))
                                    .truncate()
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut self.content_library_inst_filter,
                                            "all".to_string(),
                                            "All Instances",
                                        );
                                        for inst in &instances_snapshot {
                                            ui.selectable_value(
                                                &mut self.content_library_inst_filter,
                                                inst.instance_id.clone(),
                                                &inst.name,
                                            );
                                        }
                                    });

                                ui.checkbox(&mut self.content_library_pinned_only, "Pinned only");
                            });
                        });

                    ui.add_space(8.0);

                    if self.content_library_loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Scanning installed content...").color(p.accent));
                        });
                        return;
                    }

                    let q = self.content_library_search.trim().to_ascii_lowercase();
                    let filtered: Vec<&(String, String, InstalledContentItem)> = items_snapshot
                        .iter()
                        .filter(|(inst_id, inst_name, item)| {
                            if self.content_library_pinned_only && !item.pinned {
                                return false;
                            }
                            if self.content_library_kind_filter != "all"
                                && item.kind.as_str() != self.content_library_kind_filter
                            {
                                return false;
                            }
                            if self.content_library_inst_filter != "all"
                                && inst_id != &self.content_library_inst_filter
                            {
                                return false;
                            }
                            if q.is_empty() {
                                return true;
                            }
                            let hay = format!(
                                "{} {} {} {} {}",
                                item.name, item.file_name, item.provider, inst_name, item.version
                            )
                            .to_ascii_lowercase();
                            hay.contains(&q)
                        })
                        .collect();

                    ui.label(
                        RichText::new(format!("{} item(s)", filtered.len()))
                            .monospace()
                            .size(11.0)
                            .color(p.text_muted),
                    );
                    ui.add_space(4.0);

                    ScrollArea::vertical()
                        .id_salt("mcw_content_library_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let row_w = (ui.available_width() - 12.0).max(240.0);
                            if filtered.is_empty() {
                                ui.add_space(20.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new("No matching content items found.")
                                            .color(p.text_muted),
                                    );
                                });
                            } else {
                                for (inst_id, inst_name, item) in filtered {
                                    Frame::new()
                                        .fill(p.surface)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .corner_radius(CornerRadius::same(10))
                                        .inner_margin(egui::Margin::symmetric(10, 8))
                                        .show(ui, |ui| {
                                            ui.set_width(row_w);
                                            ui.horizontal(|ui| {
                                                let max_left = (row_w - 190.0).max(160.0);
                                                ui.vertical(|ui| {
                                                    ui.set_max_width(max_left);
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "[{}]",
                                                                item.kind.as_str().to_uppercase()
                                                            ))
                                                            .monospace()
                                                            .size(10.5)
                                                            .color(p.accent),
                                                        );
                                                        ui.label(RichText::new(&item.name).strong());
                                                        if !item.version.is_empty() {
                                                            ui.label(
                                                                RichText::new(&item.version)
                                                                    .monospace()
                                                                    .size(11.0)
                                                                    .color(p.text_muted),
                                                            );
                                                        }
                                                        if item.pinned {
                                                            ui.label(
                                                                RichText::new("[Pinned]")
                                                                    .monospace()
                                                                    .size(10.5)
                                                                    .color(p.warning),
                                                            );
                                                        }
                                                    });
                                                    let sub = format!(
                                                        "{inst_name} - {} - {} - {} - {}",
                                                        item.file_name,
                                                        item.provider,
                                                        item.status,
                                                        format_bytes(item.size_bytes)
                                                    );
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(sub)
                                                                .size(11.0)
                                                                .color(p.text_muted),
                                                        )
                                                        .truncate(),
                                                    );
                                                });
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if ui.button("Open Folder").clicked() {
                                                            let full = self
                                                                .paths
                                                                .minecraft_dir(inst_id)
                                                                .join(&item.target_path);
                                                            let folder = full
                                                                .parent()
                                                                .unwrap_or(&full)
                                                                .to_path_buf();
                                                            let _ = std::fs::create_dir_all(&folder);
                                                            open_external(
                                                                &folder.display().to_string(),
                                                            );
                                                        }
                                                        let pin_lbl = if item.pinned {
                                                            "Unpin"
                                                        } else {
                                                            "Pin"
                                                        };
                                                        if ui.button(pin_lbl).clicked() {
                                                            toggle_pin = Some((
                                                                inst_id.clone(),
                                                                item.item_id.clone(),
                                                                !item.pinned,
                                                            ));
                                                        }
                                                    },
                                                );
                                            });
                                        });
                                    ui.add_space(4.0);
                                }
                            }
                        });
                });
        });

        self.show_content_library_window = open;
        if let Some((inst_id, item_id, new_pin)) = toggle_pin {
            let inst_dir = self.paths.instance_dir(&inst_id);
            let _ = InstalledContentLibrary::set_flags(&inst_dir, &item_id, Some(new_pin), None);
            for (row_inst, _, row_item) in &mut self.content_library_items {
                if row_inst == &inst_id && row_item.item_id == item_id {
                    row_item.pinned = new_pin;
                }
            }
        }
        if refresh_clicked {
            self.start_content_library_scan(ctx);
        }
    }

    pub(crate) fn render_modpack_update_window(&mut self, ctx: &egui::Context) {
        if !self.show_modpack_update_window {
            return;
        }
        let p = self.palette;
        let inst_id = self.modpack_update_instance_id.clone();
        let mut open = self.show_modpack_update_window;
        let mut recheck = false;
        let mut apply_candidate: Option<mcw_providers::ModpackUpdateCandidate> = None;

        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_modpack_update");
        let builder = centered_viewport_builder(
            ctx,
            format!("Modpack Update - {inst_id}"),
            [640.0, 440.0],
            [500.0, 340.0],
        );

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx
                .input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
            {
                open = false;
            }
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Update Modpack In Place")
                                    .size(18.0)
                                    .strong()
                                    .color(p.text),
                            );
                            ui.label(
                                RichText::new("Upgrades managed pack files while preserving saves, options.txt, servers.dat, screenshots, and user config.")
                                    .size(11.5)
                                    .color(p.text_muted),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(self.t("common.close")).clicked() {
                                open = false;
                            }
                        });
                    });

                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .checkbox(&mut self.modpack_update_include_beta, "Include Beta")
                            .changed()
                        {
                            recheck = true;
                        }
                        if ui
                            .checkbox(&mut self.modpack_update_include_alpha, "Include Alpha")
                            .changed()
                        {
                            recheck = true;
                        }
                        if ui
                            .add_enabled(!self.modpack_update_loading, Button::new("Refresh"))
                            .clicked()
                        {
                            recheck = true;
                        }
                    });
                    ui.add_space(8.0);

                    if self.modpack_update_loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Checking Modrinth pack versions...").color(p.accent));
                        });
                        return;
                    }
                    if let Some(ref err) = self.modpack_update_error {
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, p.warning))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(egui::Margin::same(12))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(err).color(p.warning));
                            });
                        return;
                    }
                    if let Some(ref check) = self.modpack_update_check {
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .corner_radius(CornerRadius::same(12))
                            .inner_margin(egui::Margin::same(12))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                let cur_ver = if check.current_version_number.is_empty() {
                                    "unknown".to_string()
                                } else {
                                    check.current_version_number.clone()
                                };
                                ui.label(
                                    RichText::new(format!(
                                        "{} (Current version: {cur_ver})",
                                        check.pack_name
                                    ))
                                    .strong(),
                                );
                                ui.add_space(8.0);
                                if check.versions.is_empty() {
                                    ui.label(
                                        RichText::new("No matching versions found for current filters.")
                                            .color(p.warning),
                                    );
                                    return;
                                }
                                let sel_cand = check
                                    .versions
                                    .iter()
                                    .find(|v| v.version_id == self.modpack_update_selected_version)
                                    .or_else(|| check.versions.first());
                                let sel_lbl = sel_cand
                                    .map(|v| {
                                        let gvs = v
                                            .game_versions
                                            .iter()
                                            .take(3)
                                            .cloned()
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        format!(
                                            "[{}] {} - MC {} ({})",
                                            v.version_type.to_uppercase(),
                                            v.version_number,
                                            gvs,
                                            v.loaders.join(", ")
                                        )
                                    })
                                    .unwrap_or_else(|| "Select version...".to_string());
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Target Version:");
                                    egui::ComboBox::from_id_salt("mpu_target_ver_combo")
                                        .selected_text(&sel_lbl)
                                        .width(auto_combo_width(ui, &sel_lbl, 260.0, 440.0))
                                        .truncate()
                                        .show_ui(ui, |ui| {
                                            for v in &check.versions {
                                                let gvs = v
                                                    .game_versions
                                                    .iter()
                                                    .take(3)
                                                    .cloned()
                                                    .collect::<Vec<_>>()
                                                    .join(", ");
                                                let lbl = format!(
                                                    "[{}] {} - MC {} ({})",
                                                    v.version_type.to_uppercase(),
                                                    v.version_number,
                                                    gvs,
                                                    v.loaders.join(", ")
                                                );
                                                ui.selectable_value(
                                                    &mut self.modpack_update_selected_version,
                                                    v.version_id.clone(),
                                                    lbl,
                                                );
                                            }
                                        });
                                });

                                if let Some(cand) = sel_cand {
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(format!(
                                            "File: {} ({})",
                                            cand.file_name,
                                            format_bytes(cand.file_size)
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(p.text_muted),
                                    );
                                    ui.add_space(10.0);
                                    let btn = Button::new(
                                        RichText::new("Update Modpack")
                                            .color(p.accent_text)
                                            .strong(),
                                    )
                                    .fill(p.accent);
                                    if ui.add(btn).clicked() {
                                        apply_candidate = Some(cand.clone());
                                    }
                                }
                            });
                    }
                });
        });

        self.show_modpack_update_window = open;
        if recheck {
            self.start_modpack_update_check(inst_id, ctx);
        } else if let Some(cand) = apply_candidate {
            self.show_modpack_update_window = false;
            self.start_modpack_update_apply(inst_id, cand, ctx);
        }
    }

    pub(crate) fn render_preflight_confirm_window(&mut self, ctx: &egui::Context) {
        let Some(inst_id) = self.preflight_confirm_instance_id.clone() else {
            return;
        };
        let p = self.palette;
        let mut choice: Option<PreflightChoice> = None;
        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_preflight_confirm");
        let title = format!("{} - {inst_id}", self.t("compatibility.confirmation.title"));
        let builder = centered_viewport_builder(ctx, title, [620.0, 420.0], [480.0, 320.0]);

        let msg_head = self.lang.tr_fmt(
            "compatibility.confirmation.message",
            &[("count", &self.preflight_confirm_issues.len().to_string())],
        );
        let once_lbl = self.t("compatibility.confirmation.launch_once").to_string();
        let always_lbl = self
            .t("compatibility.confirmation.always_allow")
            .to_string();
        let hint_lbl = self
            .t("compatibility.confirmation.settings_hint")
            .to_string();
        let cancel_lbl = self.t("common.cancel").to_string();

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
            {
                choice = Some(PreflightChoice::Cancel);
            }
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    ui.label(
                        RichText::new(self.t("compatibility.confirmation.title"))
                            .size(17.0)
                            .strong()
                            .color(p.warning),
                    );
                    ui.label(RichText::new(&msg_head).size(12.0).color(p.text));
                    ui.add_space(8.0);

                    egui::TopBottomPanel::bottom("preflight_confirm_footer")
                        .frame(
                            Frame::new()
                                .fill(p.canvas)
                                .inner_margin(egui::Margin::symmetric(0, 6)),
                        )
                        .show_inside(ui, |ui| {
                            ui.label(RichText::new(&hint_lbl).size(11.0).color(p.text_muted));
                            ui.add_space(6.0);
                            ui.horizontal_wrapped(|ui| {
                                let once_btn = Button::new(
                                    RichText::new(&once_lbl).color(p.accent_text).strong(),
                                )
                                .fill(p.accent);
                                if ui.add(once_btn).clicked() {
                                    choice = Some(PreflightChoice::Once);
                                }
                                if ui.button(&always_lbl).clicked() {
                                    choice = Some(PreflightChoice::AlwaysAllow);
                                }
                                if ui.button(&cancel_lbl).clicked() {
                                    choice = Some(PreflightChoice::Cancel);
                                }
                            });
                        });

                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ScrollArea::vertical()
                                .id_salt("preflight_confirm_issues_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for line in &self.preflight_confirm_issues {
                                        let col = if line.starts_with("[ERROR]") {
                                            p.danger
                                        } else if line.starts_with("[WARNING]") {
                                            p.warning
                                        } else {
                                            p.text_muted
                                        };
                                        ui.label(
                                            RichText::new(line).monospace().size(11.5).color(col),
                                        );
                                        ui.add_space(4.0);
                                    }
                                });
                        });
                });
        });

        if let Some(chosen) = choice {
            self.preflight_confirm_instance_id = None;
            let world = self.preflight_confirm_world.take();
            let server = self.preflight_confirm_server.take();
            self.preflight_confirm_issues.clear();
            match chosen {
                PreflightChoice::Once => {
                    self.launch_with_preflight_bypass(inst_id, world, server, ctx);
                }
                PreflightChoice::AlwaysAllow => {
                    let store = InstanceStore::new(&self.paths);
                    if let Ok(mut settings) = store.load_settings(&inst_id) {
                        settings.forge_preflight_always_allow = true;
                        let _ = store.save_settings(&inst_id, &settings);
                        self.settings_cache
                            .insert(inst_id.clone(), settings.clone());
                        if self.selected_instance_id.as_deref() == Some(inst_id.as_str()) {
                            self.selected_settings = Some(settings);
                        }
                    }
                    self.launch_with_preflight_bypass(inst_id, world, server, ctx);
                }
                PreflightChoice::Cancel => {
                    self.notice_banner = Some((
                        self.t("compatibility.confirmation.cancelled").to_string(),
                        false,
                    ));
                }
            }
        }
    }

    pub(crate) fn render_issue_report_window(&mut self, ctx: &egui::Context) {
        if !self.show_issue_report_window {
            return;
        }
        let p = self.palette;
        let mut open = self.show_issue_report_window;
        let mut copy_md = false;
        let mut export_diag = false;
        let mut open_github = false;

        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_issue_report");
        let builder = centered_viewport_builder(
            ctx,
            "Report an Issue - MCW Launcher",
            [720.0, 540.0],
            [540.0, 400.0],
        );

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx
                .input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
            {
                open = false;
            }
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Report an Issue")
                                    .size(18.0)
                                    .strong()
                                    .color(p.text),
                            );
                            ui.label(
                                RichText::new("Sanitizes usernames, tokens, and file paths before generating a Markdown report or GitHub issue.")
                                    .size(11.5)
                                    .color(p.text_muted),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(self.t("common.close")).clicked() {
                                open = false;
                            }
                        });
                    });

                    ui.add_space(8.0);
                    ui.label(RichText::new("Issue Summary / Steps to Reproduce:").strong());
                    let resp = ui.add(
                        TextEdit::multiline(&mut self.issue_report_summary)
                            .desired_rows(3)
                            .desired_width(ui.available_width())
                            .hint_text("Describe what happened and how to reproduce it..."),
                    );
                    if resp.changed() {
                        self.rebuild_issue_report_markdown();
                    }

                    ui.add_space(8.0);
                    ui.label(RichText::new("Sanitized Markdown Preview:").strong());
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_height((ui.available_height() - 48.0).max(150.0));
                            ScrollArea::vertical()
                                .id_salt("issue_report_preview_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.add(
                                        TextEdit::multiline(&mut self.issue_report_markdown.as_str())
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(ui.available_width()),
                                    );
                                });
                        });

                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        let gh_btn = Button::new(
                            RichText::new("Open GitHub Issue")
                                .color(p.accent_text)
                                .strong(),
                        )
                        .fill(p.accent);
                        if ui.add(gh_btn).clicked() {
                            open_github = true;
                        }
                        if ui.button("Copy Markdown").clicked() {
                            copy_md = true;
                        }
                        if ui.button("Export Diagnostics .zip").clicked() {
                            export_diag = true;
                        }
                    });
                });
        });

        self.show_issue_report_window = open;
        if copy_md {
            ctx.copy_text(self.issue_report_markdown.clone());
            self.notice_banner =
                Some(("Copied sanitized issue report Markdown.".to_string(), false));
        }
        if open_github {
            let input = self.current_issue_report_input();
            let url =
                IssueReportBuilder::build_github_issue_url(&input, "mahiru7229/mcw-launcher-rust");
            open_external(&url);
        }
        if export_diag {
            if let Some(inst) = self.selected_instance().cloned() {
                if let Some(dest) = save_file_dialog(
                    "Export Diagnostics Bundle",
                    "ZIP archive (*.zip)|*.zip|All Files (*.*)|*.*",
                    &format!("{}-diagnostics.zip", inst.instance_id),
                ) {
                    let live: Vec<String> = self
                        .instance_logs
                        .get(&inst.instance_id)
                        .map(|v| v.iter().map(|l| l.text.clone()).collect())
                        .unwrap_or_default();
                    let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
                    match DiagnosticsExporter::export_bundle(
                        &mc_dir,
                        &inst.name,
                        &inst.version_id,
                        &inst.mod_loader.name,
                        self.pc_ram_mb,
                        &self.java_installations,
                        &live,
                        &dest,
                    ) {
                        Ok(path) => {
                            self.notice_banner = Some((
                                format!("Exported diagnostics bundle to {}", path.display()),
                                false,
                            ));
                        }
                        Err(e) => {
                            self.notice_banner =
                                Some((format!("Failed to export diagnostics: {e}"), true));
                        }
                    }
                }
            } else {
                self.notice_banner = Some((
                    "Select an instance first to export an instance diagnostics .zip.".to_string(),
                    true,
                ));
            }
        }
    }

    pub(crate) fn render_theme_issues_window(&mut self, ctx: &egui::Context) {
        if !self.show_theme_issues_window {
            return;
        }
        let p = self.palette;
        let mut open = self.show_theme_issues_window;
        let mut reload_clicked = false;
        let issues = self.theme_catalog.issues.clone();

        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_theme_issues");
        let builder = centered_viewport_builder(
            ctx,
            "Theme Validation Issues - MCW Launcher",
            [640.0, 420.0],
            [480.0, 320.0],
        );

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
            {
                open = false;
            }
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("External Theme Issues")
                                .size(18.0)
                                .strong()
                                .color(p.text),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(self.t("common.close")).clicked() {
                                open = false;
                            }
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
                    });
                    ui.add_space(8.0);
                    ScrollArea::vertical()
                        .id_salt("theme_issues_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if issues.is_empty() {
                                ui.label(
                                    RichText::new("All external theme packs loaded cleanly.")
                                        .color(p.accent),
                                );
                            } else {
                                for iss in &issues {
                                    Frame::new()
                                        .fill(p.surface)
                                        .stroke(Stroke::new(1.0_f32, p.warning))
                                        .corner_radius(CornerRadius::same(10))
                                        .inner_margin(egui::Margin::same(10))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.label(
                                                RichText::new(&iss.file_name)
                                                    .strong()
                                                    .color(p.text),
                                            );
                                            ui.label(
                                                RichText::new(&iss.message)
                                                    .size(11.5)
                                                    .color(p.warning),
                                            );
                                            ui.label(
                                                RichText::new(iss.path.display().to_string())
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(p.text_muted),
                                            );
                                        });
                                    ui.add_space(6.0);
                                }
                            }
                        });
                });
        });

        self.show_theme_issues_window = open;
        if reload_clicked {
            self.theme_catalog = ThemeCatalog::load(&self.paths.theme_root);
            self.apply_current_theme(ctx);
        }
    }

    pub(crate) fn render_unsaved_changes_window(&mut self, ctx: &egui::Context) {
        if !self.show_unsaved_changes_window {
            return;
        }
        let p = self.palette;
        let mut choice: Option<UnsavedChoice> = None;
        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_unsaved_changes");
        let builder = centered_viewport_builder(
            ctx,
            "Unsaved Changes - MCW Launcher",
            [460.0, 195.0],
            [380.0, 175.0],
        );

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx
                .input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
            {
                choice = Some(UnsavedChoice::Cancel);
            }
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(16)),
                )
                .show(vp_ctx, |ui| {
                    ui.label(
                        RichText::new("Unsaved Instance Settings")
                            .size(16.0)
                            .strong()
                            .color(p.text),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("You have unsaved changes in Instance Settings. Save them before switching?")
                            .size(12.0)
                            .color(p.text_muted),
                    );
                    ui.add_space(16.0);
                    ui.horizontal_wrapped(|ui| {
                        let save_btn = Button::new(
                            RichText::new("Save & Continue")
                                .color(p.accent_text)
                                .strong(),
                        )
                        .fill(p.accent);
                        if ui.add(save_btn).clicked() {
                            choice = Some(UnsavedChoice::SaveAndContinue);
                        }
                        if ui.button("Discard Changes").clicked() {
                            choice = Some(UnsavedChoice::DiscardAndContinue);
                        }
                        if ui.button(self.t("common.cancel")).clicked() {
                            choice = Some(UnsavedChoice::Cancel);
                        }
                    });
                });
        });

        if let Some(c) = choice {
            self.show_unsaved_changes_window = false;
            match c {
                UnsavedChoice::SaveAndContinue => {
                    if self.save_selected_instance_settings() {
                        self.apply_pending_unsaved_navigation(ctx);
                    }
                }
                UnsavedChoice::DiscardAndContinue => {
                    self.discard_selected_instance_settings();
                    self.apply_pending_unsaved_navigation(ctx);
                }
                UnsavedChoice::Cancel => {
                    self.pending_tab_switch = None;
                    self.pending_instance_switch = None;
                }
            }
        }
    }
}
