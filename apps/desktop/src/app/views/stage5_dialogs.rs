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

    /// Dedicated OS window for inspecting, installing (including dynamic Latest JDK),
    /// opening folders, and uninstalling managed Java runtimes without overflowing Launcher Settings.
    pub(crate) fn render_java_manager_window(&mut self, ctx: &egui::Context) {
        if !self.show_java_manager_window {
            return;
        }
        let p = self.palette;
        let runtimes_root = self.paths.runtimes_root.clone();
        let mut open = self.show_java_manager_window;
        let mut scan_java = false;
        let mut check_latest = false;
        let mut install_major: Option<u32> = None;
        let mut uninstall_exe: Option<PathBuf> = None;

        let java_desc = self.t("launcher_settings.java.description").to_string();
        let scan_lbl = self.t("launcher_settings.java.scan").to_string();
        let none_lbl = self.t("launcher_settings.java.none").to_string();
        let install_hint = self.t("launcher_settings.java.install_hint").to_string();
        let mut install_choices = vec![8u32, 17, 21, 25];
        if let Some(latest) = self.latest_java_major {
            if !install_choices.contains(&latest) {
                install_choices.push(latest);
            }
        }
        install_choices.sort_unstable();
        if !install_choices.contains(&self.java_install_selected_major) {
            self.java_install_selected_major = *install_choices.last().unwrap_or(&21);
        }

        let notice = self.notice_banner.clone();
        let (keep_open, dismiss_notice) = super::dialogs::render_in_app_modal(
            ctx,
            "mcw_modal_java_manager",
            "Java Runtimes",
            [780.0, 540.0],
            [600.0, 400.0],
            p,
            notice.as_ref(),
            |ui| {
                // Subtitle + Top Action Card
                ui.label(RichText::new(&java_desc).size(11.5).color(p.text_muted));
                ui.add_space(8.0);

                // Top Action Card: Temurin installer + dynamic Latest JDK + Scan
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("Install Temurin JDK:").strong());
                            let sel_lbl = self.lang.tr_fmt(
                                "launcher_settings.java.version",
                                &[("major", &self.java_install_selected_major.to_string())],
                            );
                            let cw = auto_combo_width(ui, &sel_lbl, 110.0, 160.0);
                            egui::ComboBox::from_id_salt("jm_major_select_combo")
                                .selected_text(&sel_lbl)
                                .width(cw)
                                .show_ui(ui, |ui| {
                                    for &m in &install_choices {
                                        let txt = self.lang.tr_fmt(
                                            "launcher_settings.java.version",
                                            &[("major", &m.to_string())],
                                        );
                                        ui.selectable_value(
                                            &mut self.java_install_selected_major,
                                            m,
                                            txt,
                                        );
                                    }
                                });

                            let sel_major = self.java_install_selected_major;
                            let sel_busy = self.installing_java_majors.contains(&sel_major);
                            let sel_managed = self
                                .java_diagnostics
                                .iter()
                                .any(|d| d.is_managed && d.major_version == sel_major)
                                || self.java_installations.iter().any(|j| {
                                    j.major == sel_major && j.path.starts_with(&runtimes_root)
                                });
                            let sel_btn_lbl = if sel_busy {
                                format!("Installing Java {sel_major}...")
                            } else if sel_managed {
                                self.lang.tr_fmt(
                                    "launcher_settings.java.reinstall",
                                    &[("major", &sel_major.to_string())],
                                )
                            } else {
                                self.lang.tr_fmt(
                                    "launcher_settings.java.install",
                                    &[("major", &sel_major.to_string())],
                                )
                            };
                            if ui
                                .add_enabled(!sel_busy, Button::new(sel_btn_lbl))
                                .clicked()
                            {
                                install_major = Some(sel_major);
                            }

                            if let Some(latest_major) = self.latest_java_major {
                                let l_busy = self.installing_java_majors.contains(&latest_major);
                                let l_managed = self
                                    .java_diagnostics
                                    .iter()
                                    .any(|d| d.is_managed && d.major_version == latest_major)
                                    || self.java_installations.iter().any(|j| {
                                        j.major == latest_major
                                            && j.path.starts_with(&runtimes_root)
                                    });
                                let l_lbl = if l_busy {
                                    format!("Installing Java {latest_major}...")
                                } else if l_managed {
                                    self.lang.tr_fmt(
                                        "launcher_settings.java.latest_reinstall",
                                        &[("major", &latest_major.to_string())],
                                    )
                                } else {
                                    self.lang.tr_fmt(
                                        "launcher_settings.java.latest_install",
                                        &[("major", &latest_major.to_string())],
                                    )
                                };
                                let l_btn =
                                    Button::new(RichText::new(l_lbl).color(p.accent_text).strong())
                                        .fill(p.accent);
                                if ui.add_enabled(!l_busy, l_btn).clicked() {
                                    install_major = Some(latest_major);
                                }
                            } else {
                                let checking = self.latest_java_checking;
                                let lbl = if checking {
                                    "Checking latest JDK...".to_string()
                                } else {
                                    "Check Latest JDK".to_string()
                                };
                                if ui.add_enabled(!checking, Button::new(lbl)).clicked() {
                                    check_latest = true;
                                }
                            }

                            if ui.button(&scan_lbl).clicked() {
                                scan_java = true;
                            }
                        });

                        ui.add_space(6.0);
                        if self.latest_java_checking {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    RichText::new(self.t("launcher_settings.java.latest_checking"))
                                        .size(11.0)
                                        .color(p.accent),
                                );
                            });
                        } else if let Some(latest_major) = self.latest_java_major {
                            let hint = self
                                .lang
                                .tr_fmt(
                                    "launcher_settings.java.latest_hint",
                                    &[("major", &latest_major.to_string())],
                                )
                                .replace(['\u{2014}', '\u{2013}'], "-");
                            ui.label(RichText::new(hint).size(11.0).color(p.text_muted));
                        } else if let Some(ref err) = self.latest_java_error {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{} ({err})",
                                        self.t("launcher_settings.java.latest_unavailable")
                                    ))
                                    .size(11.0)
                                    .color(p.warning),
                                );
                                if ui.small_button("Refresh").clicked() {
                                    check_latest = true;
                                }
                            });
                        } else {
                            ui.label(RichText::new(&install_hint).size(11.0).color(p.text_muted));
                        }
                    });

                ui.add_space(10.0);

                // Detected Runtimes ScrollArea (clean per-runtime cards, zero overlapping buttons)
                ScrollArea::vertical()
                        .id_salt("java_manager_list_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let card_w = ui.available_width();
                            if self.java_scanning {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(
                                        RichText::new(self.t("java.scan.progress")).color(p.accent),
                                    );
                                });
                            } else if self.java_diagnostics.is_empty()
                                && self.java_installations.is_empty()
                            {
                                Frame::new()
                                    .fill(p.surface)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .corner_radius(CornerRadius::same(10))
                                    .inner_margin(egui::Margin::same(12))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(RichText::new(&none_lbl).color(p.warning));
                                    });
                            } else if !self.java_diagnostics.is_empty() {
                                for diag in &self.java_diagnostics {
                                    let vendor = if diag.vendor.is_empty() {
                                        "Java".to_string()
                                    } else {
                                        diag.vendor.clone()
                                    };
                                    let arch = if diag.architecture.is_empty() {
                                        format!("{}-bit", diag.bitness)
                                    } else {
                                        format!("{} ({}-bit)", diag.architecture, diag.bitness)
                                    };
                                    let exe_disp = diag.executable.display().to_string();
                                    let left_max_w = (card_w - 205.0).max(180.0);
                                    Frame::new()
                                        .fill(p.surface)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .corner_radius(CornerRadius::same(10))
                                        .inner_margin(egui::Margin::symmetric(12, 9))
                                        .show(ui, |ui| {
                                            ui.set_width((card_w - 24.0).max(200.0));
                                            ui.horizontal(|ui| {
                                                ui.vertical(|ui| {
                                                    ui.set_max_width(left_max_w);
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "Java {} ({})",
                                                                diag.major_version,
                                                                diag.version_string
                                                            ))
                                                            .strong()
                                                            .color(if diag.valid {
                                                                p.text
                                                            } else {
                                                                p.warning
                                                            }),
                                                        );
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "{vendor} - {arch} - [{}]",
                                                                diag.source
                                                            ))
                                                            .size(11.0)
                                                            .color(p.text_muted),
                                                        );
                                                    });
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(&exe_disp)
                                                                .monospace()
                                                                .size(11.0)
                                                                .color(p.text_muted),
                                                        )
                                                        .truncate(),
                                                    )
                                                    .on_hover_text(&exe_disp);
                                                });
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if diag.is_managed
                                                            && ui
                                                                .button(
                                                                    RichText::new("Uninstall")
                                                                        .color(p.danger),
                                                                )
                                                                .clicked()
                                                        {
                                                            uninstall_exe =
                                                                Some(diag.executable.clone());
                                                        }
                                                        if ui.button("Open Folder").clicked() {
                                                            let root =
                                                                JavaDiagnosticsManager::java_root_dir(
                                                                    &diag.executable,
                                                                );
                                                            open_external(
                                                                &root.display().to_string(),
                                                            );
                                                        }
                                                    },
                                                );
                                            });
                                        });
                                    ui.add_space(6.0);
                                }
                            } else {
                                for j in &self.java_installations {
                                    let exe_disp = j.path.display().to_string();
                                    let left_max_w = (card_w - 205.0).max(180.0);
                                    Frame::new()
                                        .fill(p.surface)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .corner_radius(CornerRadius::same(10))
                                        .inner_margin(egui::Margin::symmetric(12, 9))
                                        .show(ui, |ui| {
                                            ui.set_width((card_w - 24.0).max(200.0));
                                            ui.horizontal(|ui| {
                                                ui.vertical(|ui| {
                                                    ui.set_max_width(left_max_w);
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "Java {} ({})",
                                                            j.major, j.version_string
                                                        ))
                                                        .strong()
                                                        .color(p.text),
                                                    );
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(&exe_disp)
                                                                .monospace()
                                                                .size(11.0)
                                                                .color(p.text_muted),
                                                        )
                                                        .truncate(),
                                                    )
                                                    .on_hover_text(&exe_disp);
                                                });
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if j.path.starts_with(&runtimes_root)
                                                            && ui
                                                                .button(
                                                                    RichText::new("Uninstall")
                                                                        .color(p.danger),
                                                                )
                                                                .clicked()
                                                        {
                                                            uninstall_exe = Some(j.path.clone());
                                                        }
                                                        if ui.button("Open Folder").clicked() {
                                                            let root =
                                                                JavaDiagnosticsManager::java_root_dir(
                                                                    &j.path,
                                                                );
                                                            open_external(
                                                                &root.display().to_string(),
                                                            );
                                                        }
                                                    },
                                                );
                                            });
                                        });
                                    ui.add_space(6.0);
                                }
                            }

                            let recovery_cnt = JavaRecoveryDiagnostics::snapshot().len();
                            if recovery_cnt > 0 {
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(format!(
                                        "Java auto-selection events recorded this session: {recovery_cnt}"
                                    ))
                                    .size(10.5)
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

        self.show_java_manager_window = open;
        if scan_java {
            self.reload_java();
            self.start_java_diagnostics_scan(ctx);
            self.notice_banner = Some((
                format!(
                    "Detected {} Java runtime(s).",
                    self.java_installations.len()
                ),
                false,
            ));
        }
        if check_latest {
            self.start_check_latest_java_release(ctx);
        }
        if let Some(major) = install_major {
            self.start_java_install(major, ctx);
        }
        if let Some(exe) = uninstall_exe {
            self.uninstall_managed_java(exe, ctx);
        }
    }
}
