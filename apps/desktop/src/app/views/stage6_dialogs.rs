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
        self.render_python_import_window(ctx);
    }

    pub(crate) fn render_content_library_window(&mut self, ctx: &egui::Context) {
        if !self.show_content_library_window {
            return;
        }
        let p = self.palette;
        let mut open = self.show_content_library_window;
        let mut refresh_clicked = false;
        let mut toggle_pin: Option<(String, String, bool)> = None;

        let cl_title = self.t("content_library.title").to_string();
        let cl_subtitle = self.t("content_library.subtitle").to_string();
        let cl_refresh = self.t("content_library.refresh").to_string();
        let cl_filter_hint = self.t("content_library.search_hint").to_string();
        let cl_pinned_only = self.t("content_library.pinned_only").to_string();
        let cl_scanning = self.t("content_library.scanning").to_string();
        let cl_empty = self.t("content_library.empty").to_string();
        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_content_library");
        let builder = centered_viewport_builder(
            ctx,
            format!("{cl_title} - MCW Launcher"),
            [860.0, 560.0],
            [640.0, 420.0],
        );

        let instances_snapshot = self.instances.clone();
        let items_snapshot = self.content_library_items.clone();

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
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&cl_title).size(18.0).strong().color(p.text));
                            ui.label(RichText::new(&cl_subtitle).size(11.5).color(p.text_muted));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(self.t("common.close")).clicked() {
                                open = false;
                            }
                            if ui
                                .add_enabled(
                                    !self.content_library_loading,
                                    Button::new(&cl_refresh),
                                )
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
                                        .hint_text(&cl_filter_hint)
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

                                ui.checkbox(&mut self.content_library_pinned_only, &cl_pinned_only);
                            });
                        });

                    ui.add_space(8.0);

                    if self.content_library_loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(&cl_scanning).color(p.accent));
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
                                    ui.label(RichText::new(&cl_empty).color(p.text_muted));
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
                                                        ui.label(
                                                            RichText::new(&item.name).strong(),
                                                        );
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
                                                            let _ =
                                                                std::fs::create_dir_all(&folder);
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

        let mpu_title = self.t("modpack_update.title").to_string();
        let mpu_subtitle = self.t("modpack_update.subtitle").to_string();
        let mpu_checking = self.t("modpack_update.checking").to_string();
        let mpu_apply = self.t("modpack_update.apply_button").to_string();
        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_modpack_update");
        let builder = centered_viewport_builder(
            ctx,
            format!("{mpu_title} - {inst_id}"),
            [640.0, 440.0],
            [500.0, 340.0],
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
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&mpu_title).size(18.0).strong().color(p.text));
                            ui.label(RichText::new(&mpu_subtitle).size(11.5).color(p.text_muted));
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
                            ui.label(RichText::new(&mpu_checking).color(p.accent));
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
                                        RichText::new(
                                            "No matching versions found for current filters.",
                                        )
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
                                        RichText::new(&mpu_apply).color(p.accent_text).strong(),
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

        let ir_title = self.t("issue_report.title").to_string();
        let ir_subtitle = self.t("issue_report.subtitle").to_string();
        let ir_summary_lbl = self.t("issue_report.summary_label").to_string();
        let ir_summary_hint = self.t("issue_report.summary_hint").to_string();
        let ir_preview_lbl = self.t("issue_report.preview_label").to_string();
        let ir_github_btn = self.t("issue_report.open_github").to_string();
        let ir_copy_btn = self.t("issue_report.copy_markdown").to_string();
        let ir_export_btn = self.t("issue_report.export_zip").to_string();
        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_issue_report");
        let builder = centered_viewport_builder(
            ctx,
            format!("{ir_title} - MCW Launcher"),
            [720.0, 540.0],
            [540.0, 400.0],
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
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&ir_title).size(18.0).strong().color(p.text));
                            ui.label(RichText::new(&ir_subtitle).size(11.5).color(p.text_muted));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(self.t("common.close")).clicked() {
                                open = false;
                            }
                        });
                    });

                    ui.add_space(8.0);
                    ui.label(RichText::new(&ir_summary_lbl).strong());
                    let resp = ui.add(
                        TextEdit::multiline(&mut self.issue_report_summary)
                            .desired_rows(3)
                            .desired_width(ui.available_width())
                            .hint_text(&ir_summary_hint),
                    );
                    if resp.changed() {
                        self.rebuild_issue_report_markdown();
                    }

                    ui.add_space(8.0);
                    ui.label(RichText::new(&ir_preview_lbl).strong());
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
                                        TextEdit::multiline(
                                            &mut self.issue_report_markdown.as_str(),
                                        )
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(ui.available_width()),
                                    );
                                });
                        });

                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        let gh_btn = Button::new(
                            RichText::new(&ir_github_btn).color(p.accent_text).strong(),
                        )
                        .fill(p.accent);
                        if ui.add(gh_btn).clicked() {
                            open_github = true;
                        }
                        if ui.button(&ir_copy_btn).clicked() {
                            copy_md = true;
                        }
                        if ui.button(&ir_export_btn).clicked() {
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

        let ti_title = self.t("theme_issues.title").to_string();
        let ti_open_folder = self.t("theme_issues.open_folder").to_string();
        let ti_all_clean = self.t("theme_issues.all_clean").to_string();
        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_theme_issues");
        let builder = centered_viewport_builder(
            ctx,
            format!("{ti_title} - MCW Launcher"),
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
                        ui.label(RichText::new(&ti_title).size(18.0).strong().color(p.text));
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
                            if ui.button(&ti_open_folder).clicked() {
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
                                ui.label(RichText::new(&ti_all_clean).color(p.accent));
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
        let uc_title = self.t("unsaved_changes.title").to_string();
        let uc_prompt = self.t("unsaved_changes.prompt").to_string();
        let uc_save = self.t("unsaved_changes.save_continue").to_string();
        let uc_discard = self.t("unsaved_changes.discard_continue").to_string();
        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_unsaved_changes");
        let builder = centered_viewport_builder(
            ctx,
            format!("{uc_title} - MCW Launcher"),
            [460.0, 195.0],
            [380.0, 175.0],
        );

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
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
                    ui.label(RichText::new(&uc_title).size(16.0).strong().color(p.text));
                    ui.add_space(6.0);
                    ui.label(RichText::new(&uc_prompt).size(12.0).color(p.text_muted));
                    ui.add_space(16.0);
                    ui.horizontal_wrapped(|ui| {
                        let save_btn =
                            Button::new(RichText::new(&uc_save).color(p.accent_text).strong())
                                .fill(p.accent);
                        if ui.add(save_btn).clicked() {
                            choice = Some(UnsavedChoice::SaveAndContinue);
                        }
                        if ui.button(&uc_discard).clicked() {
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

    pub(crate) fn render_python_import_window(&mut self, ctx: &egui::Context) {
        if !self.show_python_import_window {
            return;
        }
        let p = self.palette;
        let mut open = self.show_python_import_window;
        let mut browse_clicked = false;
        let mut inspect_clicked = false;
        let mut import_clicked = false;

        let win_title = self.t("python_import.title").to_string();
        let subtitle = self.t("python_import.subtitle").to_string();
        let safety_badge = self.t("python_import.safety_badge").to_string();
        let folder_lbl = self.t("python_import.source_folder").to_string();
        let browse_lbl = self.t("instance_defaults.java.browse").to_string();
        let inspect_lbl = self.t("python_import.inspect_button").to_string();
        let instances_hdr = self.t("python_import.instances_header").to_string();
        let conflict_lbl = self.t("python_import.conflict_label").to_string();
        let conflict_skip_lbl = self.t("python_import.conflict_skip").to_string();
        let conflict_rename_lbl = self.t("python_import.conflict_rename").to_string();
        let opt_settings_lbl = self.t("python_import.opt_global_settings").to_string();
        let opt_runtimes_lbl = self.t("python_import.opt_runtimes").to_string();
        let opt_themes_lbl = self.t("python_import.opt_themes").to_string();
        let start_btn_lbl = self.t("python_import.start_button").to_string();
        let importing_lbl = self.t("python_import.importing").to_string();
        let close_lbl = self.t("common.close").to_string();

        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_python_import");
        let builder = centered_viewport_builder(
            ctx,
            format!("{win_title} - MCW Launcher"),
            [760.0, 580.0],
            [560.0, 440.0],
        );

        let inspection_snap = self.python_import_inspection.clone();
        let result_snap = self.python_import_result.clone();
        let selected_count = self.python_import_selected.len();
        let can_start = inspection_snap.is_some()
            && !self.python_import_running
            && !self.python_import_inspecting
            && (selected_count > 0
                || self.python_import_global_settings
                || self.python_import_runtimes
                || self.python_import_themes);

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx
                .input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
            {
                open = false;
            }

            egui::TopBottomPanel::bottom("mcw_python_import_footer")
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::symmetric(14, 10)),
                )
                .show(vp_ctx, |ui| {
                    ui.horizontal(|ui| {
                        let primary_lbl = if self.python_import_running {
                            importing_lbl.as_str()
                        } else {
                            start_btn_lbl.as_str()
                        };
                        let btn = Button::new(
                            RichText::new(primary_lbl)
                                .color(p.accent_text)
                                .strong(),
                        )
                        .fill(p.accent);
                        if ui.add_enabled(can_start, btn).clicked() {
                            import_clicked = true;
                        }
                        if ui.button(&close_lbl).clicked() {
                            open = false;
                        }
                    });
                });

            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    ui.label(RichText::new(&win_title).size(18.0).strong().color(p.text));
                    ui.add(
                        egui::Label::new(
                            RichText::new(&subtitle).size(11.5).color(p.text_muted),
                        )
                        .wrap(),
                    );
                    ui.add_space(6.0);

                    Frame::new()
                        .fill(p.elevated)
                        .stroke(Stroke::new(1.0_f32, p.accent))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&safety_badge)
                                        .size(11.5)
                                        .color(p.accent),
                                )
                                .wrap(),
                            );
                        });

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(&folder_lbl);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let insp_btn = Button::new(
                                RichText::new(&inspect_lbl)
                                    .color(p.accent_text)
                                    .strong(),
                            )
                            .fill(p.accent);
                            if ui
                                .add_enabled(
                                    !self.python_import_inspecting && !self.python_import_running,
                                    insp_btn,
                                )
                                .clicked()
                            {
                                inspect_clicked = true;
                            }
                            if ui
                                .add_enabled(
                                    !self.python_import_folder_picking
                                        && !self.python_import_running,
                                    Button::new(&browse_lbl),
                                )
                                .clicked()
                            {
                                browse_clicked = true;
                            }
                            let edit_w = ui.available_width().max(150.0);
                            ui.add(
                                TextEdit::singleline(&mut self.python_import_path_input)
                                    .hint_text(r"D:\MINECRAFT\mcw-launcher")
                                    .desired_width(edit_w),
                            );
                        });
                    });

                    if let Some(ref err) = self.python_import_error {
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(RichText::new(err).color(p.danger)).wrap(),
                        );
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    if self.python_import_inspecting {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                RichText::new("Scanning Python launcher folder...")
                                    .color(p.accent),
                            );
                        });
                        return;
                    }

                    if let Some(ref res) = result_snap {
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, p.accent))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    RichText::new(format!(
                                        "Import complete: {} instance(s) copied, {} skipped, {} Java runtime(s), {} theme(s), {} error(s).",
                                        res.imported_instances.len(),
                                        res.skipped_instances.len(),
                                        res.imported_runtimes.len(),
                                        res.imported_themes.len(),
                                        res.errors.len()
                                    ))
                                    .strong()
                                    .color(if res.errors.is_empty() { p.accent } else { p.warning }),
                                );
                                for e in res.errors.iter().take(5) {
                                    ui.label(RichText::new(e).size(11.0).color(p.danger));
                                }
                            });
                        ui.add_space(8.0);
                    }

                    let Some(ref insp) = inspection_snap else {
                        ui.label(
                            RichText::new("Select your MCW Launcher Python root folder (containing instances/, config/, runtimes/) and click 'Inspect Folder'.")
                                .color(p.text_muted),
                        );
                        return;
                    };

                    if !insp.warnings.is_empty() {
                        Frame::new()
                            .fill(p.elevated)
                            .stroke(Stroke::new(1.0_f32, p.warning))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(egui::Margin::symmetric(10, 7))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                for w in &insp.warnings {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(w).size(11.0).color(p.warning),
                                        )
                                        .wrap(),
                                    );
                                }
                            });
                        ui.add_space(8.0);
                    }

                    // Options card (Conflict Strategy + Global Settings + Runtimes + Themes)
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.label(&conflict_lbl);
                                let cur_c = match self.python_import_conflict {
                                    ConflictStrategy::Skip => &conflict_skip_lbl,
                                    ConflictStrategy::RenameImported => &conflict_rename_lbl,
                                };
                                egui::ComboBox::from_id_salt("py_import_conflict_combo")
                                    .selected_text(cur_c)
                                    .width(auto_combo_width(ui, cur_c, 180.0, 260.0))
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut self.python_import_conflict,
                                            ConflictStrategy::Skip,
                                            &conflict_skip_lbl,
                                        );
                                        ui.selectable_value(
                                            &mut self.python_import_conflict,
                                            ConflictStrategy::RenameImported,
                                            &conflict_rename_lbl,
                                        );
                                    });
                            });
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.add_enabled(
                                    insp.has_launcher_settings,
                                    egui::Checkbox::new(
                                        &mut self.python_import_global_settings,
                                        &opt_settings_lbl,
                                    ),
                                );
                                let rt_txt = format!(
                                    "{opt_runtimes_lbl} ({})",
                                    insp.managed_runtimes.len()
                                );
                                ui.add_enabled(
                                    !insp.managed_runtimes.is_empty(),
                                    egui::Checkbox::new(&mut self.python_import_runtimes, rt_txt),
                                );
                                let th_txt = format!(
                                    "{opt_themes_lbl} ({})",
                                    insp.custom_themes.len()
                                );
                                ui.add_enabled(
                                    !insp.custom_themes.is_empty(),
                                    egui::Checkbox::new(&mut self.python_import_themes, th_txt),
                                );
                            });
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{instances_hdr} ({}/{})",
                                self.python_import_selected.len(),
                                insp.instances.len()
                            ))
                            .strong(),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Select None").clicked() {
                                self.python_import_selected.clear();
                            }
                            if ui.small_button("Select All").clicked() {
                                self.python_import_selected = insp
                                    .instances
                                    .iter()
                                    .map(|i| i.folder_name.clone())
                                    .collect();
                            }
                        });
                    });
                    ui.add_space(4.0);

                    ScrollArea::vertical()
                        .id_salt("mcw_python_import_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let row_w = (ui.available_width() - 4.0).max(220.0);
                            for inst in &insp.instances {
                                let mut checked =
                                    self.python_import_selected.contains(&inst.folder_name);
                                Frame::new()
                                    .fill(p.surface)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .corner_radius(CornerRadius::same(10))
                                    .inner_margin(egui::Margin::symmetric(10, 7))
                                    .show(ui, |ui| {
                                        ui.set_width(row_w);
                                        ui.horizontal(|ui| {
                                            if ui.checkbox(&mut checked, "").changed() {
                                                if checked {
                                                    self.python_import_selected
                                                        .insert(inst.folder_name.clone());
                                                } else {
                                                    self.python_import_selected
                                                        .remove(&inst.folder_name);
                                                }
                                            }
                                            ui.vertical(|ui| {
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label(RichText::new(&inst.name).strong());
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "MC {} - {}",
                                                            inst.version_id,
                                                            display_loader_name(&inst.loader_name)
                                                        ))
                                                        .monospace()
                                                        .size(11.0)
                                                        .color(p.accent),
                                                    );
                                                    if inst.already_exists_in_dest {
                                                        ui.label(
                                                            RichText::new("[Exists in Rust]")
                                                                .monospace()
                                                                .size(10.5)
                                                                .color(p.warning),
                                                        );
                                                    }
                                                });
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{} - {} - {}",
                                                        inst.layout.label(),
                                                        format_bytes(inst.estimated_bytes),
                                                        if inst.has_mcw_meta {
                                                            ".mcw metadata"
                                                        } else {
                                                            "vanilla meta"
                                                        }
                                                    ))
                                                    .size(11.0)
                                                    .color(p.text_muted),
                                                );
                                            });
                                        });
                                    });
                                ui.add_space(4.0);
                            }
                        });
                });
        });

        self.show_python_import_window = open;
        if browse_clicked {
            self.python_import_folder_picking = true;
            let tx = self.tx.clone();
            let ctx_c = ctx.clone();
            pick_folder_dialog_async(folder_lbl, move |picked| {
                let _ = tx.send(AppEvent::PythonFolderPicked(picked));
                ctx_c.request_repaint();
            });
        } else if inspect_clicked {
            self.start_python_inspection(ctx);
        } else if import_clicked {
            self.start_python_migration(ctx);
        }
    }
}
