use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    /// Dedicated centered OS window for viewing and exporting `mcw-lan-agent.log`
    /// (port of Python `LanAgentLogDialog` in `src/gui/dialogs/lan_agent_log_dialog.py`).
    pub(crate) fn render_lan_agent_log_window(&mut self, ctx: &egui::Context) {
        if !self.show_lan_agent_log_window {
            return;
        }
        let p = self.palette;
        let inst_id = self.lan_agent_log_instance_id.clone();
        if inst_id.is_empty() {
            self.show_lan_agent_log_window = false;
            return;
        }
        let inst_dir = self.paths.instance_dir(&inst_id);
        let log_path = LanAgentManager::log_path(&inst_dir);
        let exists = log_path.is_file();
        let mut open = self.show_lan_agent_log_window;
        let mut refresh_clicked = false;
        let mut copy_clicked = false;
        let mut export_clicked = false;
        let mut open_file_clicked = false;
        let mut open_folder_clicked = false;

        let title = self.t("lan.agent.log.title").to_string();
        let desc = self.t("lan.agent.log.description").to_string();
        let refresh_lbl = self.t("common.refresh").to_string();
        let copy_lbl = self.t("logs.copy_all").to_string();
        let open_file_lbl = self.t("lan.agent.log.open_file").to_string();
        let open_folder_lbl = self.t("lan.agent.log.open_folder").to_string();
        let close_lbl = self.t("common.close").to_string();
        let path_line = self.lang.tr_fmt(
            "lan.agent.log.path",
            &[("path", &log_path.display().to_string())],
        );

        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_lan_agent_log");
        let builder = centered_viewport_builder(ctx, title, [760.0, 520.0], [540.0, 360.0]);

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
            {
                open = false;
            }
            egui::TopBottomPanel::bottom("lan_agent_log_footer")
                .frame(
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(vp_ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(&refresh_lbl).clicked() {
                            refresh_clicked = true;
                        }
                        if ui.button(&copy_lbl).clicked() {
                            copy_clicked = true;
                        }
                        if ui.button("Export Log...").clicked() {
                            export_clicked = true;
                        }
                        if ui
                            .add_enabled(exists, Button::new(&open_file_lbl))
                            .clicked()
                        {
                            open_file_clicked = true;
                        }
                        if ui.button(&open_folder_lbl).clicked() {
                            open_folder_clicked = true;
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
                        .inner_margin(egui::Margin::same(16)),
                )
                .show(vp_ctx, |ui| {
                    ui.label(RichText::new(&desc).size(12.5).color(p.text));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(&path_line)
                            .monospace()
                            .size(11.0)
                            .color(p.text_muted),
                    );
                    ui.add_space(8.0);
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ScrollArea::vertical()
                                .id_salt("lan_agent_log_scroll")
                                .auto_shrink([false, false])
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let body = if self.lan_agent_log_text.is_empty() {
                                        self.t("lan.agent.log.not_found").to_string()
                                    } else {
                                        self.lan_agent_log_text.clone()
                                    };
                                    ui.add(
                                        egui::TextEdit::multiline(&mut body.as_str())
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(ui.available_width()),
                                    );
                                });
                        });
                });
        });

        self.show_lan_agent_log_window = open;
        if refresh_clicked {
            self.refresh_lan_agent_log();
        }
        if copy_clicked {
            ctx.copy_text(self.lan_agent_log_text.clone());
            self.notice_banner = Some((self.t("Copied to clipboard.").to_string(), false));
        }
        if open_file_clicked && exists {
            open_external(&log_path.display().to_string());
        }
        if open_folder_clicked {
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
                open_external(&parent.display().to_string());
            }
        }
        if export_clicked {
            if let Some(dest) = save_file_dialog(
                "Export MCW LAN Agent Log",
                "Text Log (*.log;*.txt)|*.log;*.txt|All Files (*.*)|*.*",
                &format!("{inst_id}-mcw-lan-agent.log"),
            ) {
                match std::fs::write(&dest, &self.lan_agent_log_text) {
                    Ok(()) => {
                        self.notice_banner = Some((
                            format!("Exported LAN Agent log to {}", dest.display()),
                            false,
                        ));
                    }
                    Err(e) => {
                        self.notice_banner =
                            Some((format!("Failed to export LAN Agent log: {e}"), true));
                    }
                }
            }
        }
    }
}
