use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_logs_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.palette;

        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(self.t("navigation.logs"))
                        .size(28.0)
                        .strong()
                        .color(p.text),
                );
                ui.label(
                    RichText::new(
                        "Inspect global launcher events or live stdout/stderr game logs.",
                    )
                    .size(12.5)
                    .color(p.text_muted),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Pop Out Global Window").clicked() {
                    self.open_global_logs_window(ctx);
                }
            });
        });

        ui.add_space(12.0);

        let is_global = self.logs_page_source == "__global__"
            || !self
                .instances
                .iter()
                .any(|i| i.instance_id == self.logs_page_source);
        let selected_source_label = if is_global {
            self.logs_page_source = "__global__".to_string();
            "Global Launcher Events".to_string()
        } else {
            self.instances
                .iter()
                .find(|i| i.instance_id == self.logs_page_source)
                .map(|i| format!("{} ({})", i.name, i.version_id))
                .unwrap_or_else(|| "Global Launcher Events".to_string())
        };

        let copy_lbl = self.t("logs.copy_all").to_string();
        let clear_lbl = self.t("common.clear").to_string();

        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(ui.available_height());

                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Source:").strong());
                    let src_w = auto_combo_width(ui, &selected_source_label, 180.0, 290.0);
                    egui::ComboBox::from_id_salt("logs_page_source_combo")
                        .selected_text(&selected_source_label)
                        .width(src_w)
                        .truncate()
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.logs_page_source,
                                "__global__".to_string(),
                                "Global Launcher Events",
                            );
                            for inst in &self.instances {
                                let lbl = format!("{} ({})", inst.name, inst.version_id);
                                ui.selectable_value(
                                    &mut self.logs_page_source,
                                    inst.instance_id.clone(),
                                    lbl,
                                );
                            }
                        });

                    ui.add(
                        TextEdit::singleline(&mut self.logs_page_filter)
                            .hint_text("Filter log lines...")
                            .desired_width(210.0),
                    );

                    if self.logs_page_source == "__global__" {
                        let lines_snapshot: Vec<String> =
                            self.global_logs.iter().cloned().collect();
                        if ui.button(&copy_lbl).clicked() {
                            ctx.copy_text(lines_snapshot.join("\n"));
                            self.notice_banner = Some((
                                "Copied global launcher logs to clipboard.".to_string(),
                                false,
                            ));
                        }
                        if ui.button("Export Log...").clicked() {
                            if let Some(dest) = save_file_dialog(
                                "Export Global Launcher Log",
                                "Text Log (*.log;*.txt)|*.log;*.txt|All Files (*.*)|*.*",
                                "mcw-launcher-global.log",
                            ) {
                                match std::fs::write(&dest, lines_snapshot.join("\n")) {
                                    Ok(()) => {
                                        self.notice_banner = Some((
                                            format!("Exported global log to {}", dest.display()),
                                            false,
                                        ));
                                    }
                                    Err(e) => {
                                        self.notice_banner =
                                            Some((format!("Failed to export log: {e}"), true));
                                    }
                                }
                            }
                        }
                        if ui.button(&clear_lbl).clicked() {
                            self.global_logs.clear();
                        }
                    } else if let Some(inst) = self
                        .instances
                        .iter()
                        .find(|i| i.instance_id == self.logs_page_source)
                        .cloned()
                    {
                        ui.checkbox(&mut self.logs_errors_only, "Errors only");
                        if ui.button(&copy_lbl).clicked() {
                            let text = self
                                .instance_logs
                                .get(&inst.instance_id)
                                .map(|v| {
                                    v.iter()
                                        .map(|l| l.text.as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .unwrap_or_default();
                            ctx.copy_text(text);
                            self.notice_banner =
                                Some(("Copied instance logs to clipboard.".to_string(), false));
                        }
                        if ui.button(&clear_lbl).clicked() {
                            self.instance_logs
                                .insert(inst.instance_id.clone(), Vec::new());
                        }
                        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
                        let log_file = self
                            .paths
                            .logs_root
                            .join(format!("{}-latest.log", inst.instance_id));
                        if log_file.is_file() && ui.button("Open Log File").clicked() {
                            open_external(&log_file.display().to_string());
                        }
                        if let Some(crash) = find_latest_crash_report(&mc_dir) {
                            if ui
                                .button(
                                    RichText::new("Latest Crash Report")
                                        .color(p.warning)
                                        .strong(),
                                )
                                .clicked()
                            {
                                open_external(&crash.display().to_string());
                            }
                        }
                        if ui
                            .add_enabled(
                                !self.uploading_mclogs,
                                Button::new(if self.uploading_mclogs {
                                    "Uploading..."
                                } else {
                                    "mclo.gs"
                                }),
                            )
                            .clicked()
                        {
                            let mut text = self
                                .instance_logs
                                .get(&inst.instance_id)
                                .map(|v| {
                                    v.iter()
                                        .map(|l| l.text.as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .unwrap_or_default();
                            if text.trim().is_empty() && log_file.is_file() {
                                text = std::fs::read_to_string(&log_file).unwrap_or_default();
                            }
                            if text.trim().is_empty() {
                                self.notice_banner = Some((
                                    "No instance log lines available to upload.".to_string(),
                                    true,
                                ));
                            } else {
                                self.start_mclogs_upload(text, ctx);
                            }
                        }
                        if ui.button("Export Diagnostics (.zip)").clicked() {
                            let epoch = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0);
                            let default_name =
                                format!("mcw-diagnostics-{}-{epoch}.zip", inst.instance_id);
                            if let Some(out_zip) = save_file_dialog(
                                "Export Diagnostic Bundle (.zip)",
                                "Zip Archive (*.zip)|*.zip|All Files (*.*)|*.*",
                                &default_name,
                            ) {
                                let live_lines: Vec<String> = self
                                    .instance_logs
                                    .get(&inst.instance_id)
                                    .map(|v| v.iter().map(|l| l.text.clone()).collect())
                                    .unwrap_or_default();
                                match DiagnosticsExporter::export_bundle(
                                    &mc_dir,
                                    &inst.name,
                                    &inst.version_id,
                                    &inst.mod_loader.name,
                                    self.pc_ram_mb,
                                    &self.java_installations,
                                    &live_lines,
                                    &out_zip,
                                ) {
                                    Ok(path) => {
                                        self.notice_banner = Some((
                                            format!(
                                                "Exported diagnostic bundle to {}",
                                                path.display()
                                            ),
                                            false,
                                        ));
                                    }
                                    Err(e) => {
                                        self.notice_banner = Some((
                                            format!("Failed to export diagnostics: {e}"),
                                            true,
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    if ui.button("Report Issue...").clicked() {
                        self.open_issue_report_window(ctx);
                    }
                });

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                let needle = self.logs_page_filter.trim().to_ascii_lowercase();
                ScrollArea::vertical()
                    .id_salt("logs_page_scroll")
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let mut shown = 0usize;
                        if self.logs_page_source == "__global__" {
                            for line in &self.global_logs {
                                if !needle.is_empty()
                                    && !line.to_ascii_lowercase().contains(&needle)
                                {
                                    continue;
                                }
                                shown += 1;
                                let col = if line.contains("[WARN]") || line.contains("[ERROR]") {
                                    p.warning
                                } else {
                                    p.text
                                };
                                ui.label(RichText::new(line).monospace().size(11.5).color(col));
                            }
                        } else if let Some(lines) = self.instance_logs.get(&self.logs_page_source) {
                            for line in lines {
                                let is_err = matches!(line.stream, LogStream::Stderr)
                                    || line.text.contains("ERROR")
                                    || line.text.contains("Exception");
                                if self.logs_errors_only && !is_err {
                                    continue;
                                }
                                if !needle.is_empty()
                                    && !line.text.to_ascii_lowercase().contains(&needle)
                                {
                                    continue;
                                }
                                shown += 1;
                                let col = if is_err { p.danger } else { p.text };
                                ui.label(
                                    RichText::new(&line.text).monospace().size(11.5).color(col),
                                );
                            }
                        }
                        if shown == 0 {
                            ui.label(
                                RichText::new("No matching log lines yet.").color(p.text_muted),
                            );
                        }
                    });
            });
    }
}
