use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_repair_center_window(&mut self, ctx: &egui::Context) {
        if !self.show_repair_center_window {
            return;
        }
        let Some(inst) = self.selected_instance().cloned() else {
            self.show_repair_center_window = false;
            return;
        };
        let p = self.palette;
        let mut open = self.show_repair_center_window;
        let mut rescan_clicked = false;
        let mut repair_clicked = false;
        let busy = self.is_instance_busy(&inst.instance_id);

        let repair_lbl = self.t("workspace.action.repair").to_string();
        let close_lbl = self.t("common.close").to_string();
        let notice = self.modal_notice_for("mcw_modal_repair_center");

        let (keep_open, dismiss_notice) = super::dialogs::render_in_app_modal(
            ctx,
            "mcw_modal_repair_center",
            &format!("Repair Center & Health - {}", inst.name),
            [680.0, 520.0],
            [480.0, 360.0],
            p,
            notice.as_ref(),
            |ui| {
                egui::TopBottomPanel::bottom("repair_center_footer")
                    .frame(
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .inner_margin(egui::Margin::same(10)),
                    )
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            let repair_btn = Button::new(
                                RichText::new(&repair_lbl).color(p.accent_text).strong(),
                            )
                            .fill(p.accent);
                            if ui.add_enabled(!busy, repair_btn).clicked() {
                                repair_clicked = true;
                            }
                            if ui.button(self.t("common.refresh")).clicked() {
                                rescan_clicked = true;
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button(&close_lbl).clicked() {
                                    open = false;
                                }
                            });
                        });
                    });

                ScrollArea::vertical()
                    .id_salt("repair_center_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            if let Some(ref rep) = self.repair_center_report {
                                let state_col = match rep.state {
                                    InstanceHealthState::Healthy => p.accent,
                                    InstanceHealthState::NeedsAttention => p.warning,
                                    _ => p.danger,
                                };
                                Frame::new()
                                    .fill(p.surface)
                                    .stroke(Stroke::new(1.0_f32, state_col))
                                    .corner_radius(CornerRadius::same(12))
                                    .inner_margin(egui::Margin::same(12))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(
                                                RichText::new("Instance Health:")
                                                    .strong()
                                                    .size(14.0),
                                            );
                                            ui.label(
                                                RichText::new(rep.state.label())
                                                    .strong()
                                                    .size(14.0)
                                                    .color(state_col),
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(format!(
                                                "version.json: {}  ·  client JAR: {}  ·  Issues: {}",
                                                if rep.version_json_present { "OK" } else { "Missing" },
                                                if rep.client_jar_present { "OK" } else { "Missing" },
                                                rep.issues.len()
                                            ))
                                            .monospace()
                                            .size(11.5)
                                            .color(p.text_muted),
                                        );
                                    });

                                ui.add_space(8.0);
                                ui.label(RichText::new("Instance Health Checks").strong());
                                ui.add_space(4.0);
                                if rep.issues.is_empty() {
                                    ui.label(
                                        RichText::new(
                                            "No structural or runtime issues detected for this instance.",
                                        )
                                        .color(p.accent),
                                    );
                                } else {
                                    for issue in &rep.issues {
                                        let col = match issue.severity {
                                            HealthSeverity::Error => p.danger,
                                            HealthSeverity::Warning => p.warning,
                                            HealthSeverity::Info => p.text_muted,
                                        };
                                        Frame::new()
                                            .fill(p.elevated)
                                            .stroke(Stroke::new(1.0_f32, col))
                                            .corner_radius(CornerRadius::same(10))
                                            .inner_margin(egui::Margin::symmetric(10, 7))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(format!(
                                                        "[{}] {}",
                                                        issue.code, issue.message
                                                    ))
                                                    .size(11.5)
                                                    .color(col),
                                                );
                                            });
                                        ui.add_space(4.0);
                                    }
                                }
                            }

                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(8.0);
                            ui.label(RichText::new("Mod Compatibility Check").strong());
                            ui.add_space(4.0);
                            if let Some(ref compat) = self.mod_compat_report {
                                ui.label(
                                    RichText::new(format!(
                                        "Enabled mods: {} · Disabled: {} · Errors: {} · Warnings: {}",
                                        compat.enabled_mods,
                                        compat.disabled_mods,
                                        compat.error_count,
                                        compat.warning_count
                                    ))
                                    .monospace()
                                    .size(11.5)
                                    .color(p.text_muted),
                                );
                                ui.add_space(4.0);
                                if compat.issues.is_empty() {
                                    ui.label(
                                        RichText::new("No mod compatibility or dependency issues detected.")
                                            .color(p.accent),
                                    );
                                } else {
                                    for issue in &compat.issues {
                                        let col = match issue.severity {
                                            ModIssueSeverity::Error => p.danger,
                                            ModIssueSeverity::Warning => p.warning,
                                            ModIssueSeverity::Info => p.text_muted,
                                        };
                                        Frame::new()
                                            .fill(p.elevated)
                                            .stroke(Stroke::new(1.0_f32, col))
                                            .corner_radius(CornerRadius::same(10))
                                            .inner_margin(egui::Margin::symmetric(10, 7))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(format!(
                                                        "[{}] {}",
                                                        issue.code, issue.message
                                                    ))
                                                    .size(11.5)
                                                    .color(col),
                                                );
                                            });
                                        ui.add_space(4.0);
                                    }
                                }
                            } else {
                                ui.label(
                                    RichText::new(
                                        "Open the Mods tab or click 'Re-scan Health' to scan installed mods.",
                                    )
                                    .size(11.5)
                                    .color(p.text_muted),
                                );
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

        self.show_repair_center_window = open;
        if rescan_clicked {
            self.open_repair_center(&inst.instance_id, ctx);
        }
        if repair_clicked {
            self.start_repair(ctx);
        }
    }

    pub(crate) fn render_global_logs_window(&mut self, ctx: &egui::Context) {
        if !self.show_global_logs_window {
            return;
        }
        let p = self.palette;
        let mut open = self.show_global_logs_window;
        let mut clear_clicked = false;
        let mut copy_clicked = false;
        let mut export_clicked = false;

        let copy_lbl = self.t("logs.copy_all").to_string();
        let clear_lbl = self.t("common.clear").to_string();
        let close_lbl = self.t("common.close").to_string();
        let title_lbl = self.t("navigation.logs").to_string();

        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_global_logs");
        let builder = centered_viewport_builder(
            ctx,
            format!("{title_lbl} - MCW Launcher"),
            [700.0, 480.0],
            [500.0, 340.0],
        );

        let lines_snapshot: Vec<String> = self.global_logs.iter().cloned().collect();
        let needle = self.global_logs_filter.trim().to_ascii_lowercase();

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
            {
                open = false;
            }
            egui::TopBottomPanel::top("global_logs_toolbar")
                .frame(
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(vp_ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&title_lbl).strong());
                        ui.add(
                            TextEdit::singleline(&mut self.global_logs_filter)
                                .hint_text("Filter logs...")
                                .desired_width(200.0),
                        );
                        if ui.button(&copy_lbl).clicked() {
                            copy_clicked = true;
                        }
                        if ui.button("Export Log...").clicked() {
                            export_clicked = true;
                        }
                        if ui.button(&clear_lbl).clicked() {
                            clear_clicked = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(&close_lbl).clicked() {
                                open = false;
                            }
                        });
                    });
                });

            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(12)),
                )
                .show(vp_ctx, |ui| {
                    ScrollArea::vertical()
                        .id_salt("global_logs_scroll")
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let mut visible_count = 0usize;
                            for line in &lines_snapshot {
                                if !needle.is_empty()
                                    && !line.to_ascii_lowercase().contains(&needle)
                                {
                                    continue;
                                }
                                visible_count += 1;
                                let col = if line.contains("[WARN]") || line.contains("[ERROR]") {
                                    p.warning
                                } else {
                                    p.text
                                };
                                ui.label(RichText::new(line).monospace().size(11.5).color(col));
                            }
                            if visible_count == 0 {
                                ui.label(
                                    RichText::new("No global launcher events recorded yet.")
                                        .color(p.text_muted),
                                );
                            }
                        });
                });
        });

        self.show_global_logs_window = open;
        if clear_clicked {
            self.global_logs.clear();
        }
        if copy_clicked {
            ctx.copy_text(lines_snapshot.join("\n"));
            self.notice_banner = Some((
                "Copied global launcher logs to clipboard.".to_string(),
                false,
            ));
        }
        if export_clicked {
            if let Some(dest) = save_file_dialog(
                "Export Global Launcher Log",
                "Text Log (*.log;*.txt)|*.log;*.txt|All Files (*.*)|*.*",
                "mcw-launcher-global.log",
            ) {
                match std::fs::write(&dest, lines_snapshot.join("\n")) {
                    Ok(()) => {
                        self.notice_banner =
                            Some((format!("Exported global log to {}", dest.display()), false));
                    }
                    Err(e) => {
                        self.notice_banner = Some((format!("Failed to export log: {e}"), true));
                    }
                }
            }
        }
    }

    pub(crate) fn render_about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about_window {
            return;
        }
        let p = self.palette;
        let mut open = self.show_about_window;
        let repo_url = format!("https://github.com/{}", mcw_models::GITHUB_REPOSITORY);
        let integrity_text = if self.account_integrity_ok {
            "Verified (HMAC-SHA256 + OS DPAPI)"
        } else {
            "Attention: account store signature mismatch detected"
        };
        let integrity_col = if self.account_integrity_ok {
            p.accent
        } else {
            p.warning
        };

        let notice = self.modal_notice_for("mcw_modal_about");
        let (keep_open, dismiss_notice) = super::dialogs::render_in_app_modal(
            ctx,
            "mcw_modal_about",
            "About - MCW Launcher: Rust Edition",
            [520.0, 390.0],
            [400.0, 300.0],
            p,
            notice.as_ref(),
            |ui| {
                ui.label(
                    RichText::new("MCW Launcher: Rust Edition")
                        .strong()
                        .size(18.0)
                        .color(p.text),
                );
                ui.label(
                    RichText::new(format!("Version v{}", mcw_models::APP_VERSION))
                        .monospace()
                        .size(12.5)
                        .color(p.accent),
                );
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Native Rust Minecraft launcher engineered for high speed, low memory usage, and clean architecture.",
                        )
                        .size(12.0)
                        .color(p.text_muted),
                    )
                    .wrap(),
                );
                ui.add_space(10.0);

                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::Grid::new("about_info_grid")
                            .num_columns(2)
                            .spacing([16.0, 5.0])
                            .show(ui, |ui| {
                                ui.label(RichText::new("Developer:").color(p.text_muted));
                                ui.label(mcw_models::DEVELOPER_NAME);
                                ui.end_row();
                                ui.label(RichText::new("System RAM:").color(p.text_muted));
                                ui.label(format!("{} MB", self.pc_ram_mb));
                                ui.end_row();
                                ui.label(RichText::new("Detected Java:").color(p.text_muted));
                                ui.label(format!("{} runtime(s)", self.java_installations.len()));
                                ui.end_row();
                                ui.label(RichText::new("Account Security:").color(p.text_muted));
                                ui.label(RichText::new(integrity_text).color(integrity_col));
                                ui.end_row();
                                ui.label(RichText::new("Updater Status:").color(p.text_muted));
                                ui.label(if mcw_updater::UPDATES_ENABLED {
                                    "Enabled (Cloudflare Edge CDN + GitHub)"
                                } else {
                                    "Disabled"
                                });
                                ui.end_row();
                                ui.label(RichText::new("License:").color(p.text_muted));
                                ui.label("MIT License");
                                ui.end_row();
                                ui.label(RichText::new("Fonts:").color(p.text_muted));
                                ui.label("JetBrains Mono (SIL OFL-1.1)");
                                ui.end_row();
                            });
                    });

                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    let gh_btn = Button::new(
                        RichText::new("Open GitHub Repository")
                            .color(p.accent_text)
                            .strong(),
                    )
                    .fill(p.accent);
                    if ui.add(gh_btn).clicked() {
                        open_external(&repo_url);
                    }
                    if ui.button("Open Data Folder").clicked() {
                        open_external(&self.paths.project_root.display().to_string());
                    }
                    let mut report_clicked = false;
                    if ui.button("Report Issue...").clicked() {
                        report_clicked = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(self.t("common.close")).clicked() {
                            open = false;
                        }
                    });
                    if report_clicked {
                        open = false;
                        self.open_issue_report_window(ctx);
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

        self.show_about_window = open;
    }
}
