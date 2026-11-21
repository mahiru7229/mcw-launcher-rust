use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_servers_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
    ) {
        let p = self.palette;

        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let busy = self.is_instance_busy(&inst.instance_id);
        let has_account = self.active_account().is_some();
        let quick_play_ok = is_quick_play_supported(&inst.version_id);
        let loading = self.loading_servers.contains(&inst.instance_id);

        ScrollArea::vertical()
            .id_salt("servers_tab_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Card 1: Saved Servers (servers.dat)
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Saved Servers ({})",
                                    self.selected_servers.len()
                                ))
                                .strong()
                                .size(15.5)
                                .color(p.text),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("Open Folder").clicked() {
                                    let _ = std::fs::create_dir_all(&mc_dir);
                                    open_external(&mc_dir.display().to_string());
                                }
                                if ui.button("Refresh").clicked() {
                                    self.load_servers_async(&inst.instance_id, ctx, true);
                                }
                            });
                        });
                        ui.add_space(8.0);

                        // Add Server inline form
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Name:");
                            ui.add(
                                TextEdit::singleline(&mut self.add_server_name)
                                    .hint_text("My Server")
                                    .desired_width(170.0),
                            );
                            ui.label("Address:");
                            let ip_edit = ui.add(
                                TextEdit::singleline(&mut self.add_server_ip)
                                    .hint_text("play.example.com")
                                    .desired_width(220.0),
                            );
                            let enter =
                                ip_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            let add_btn = Button::new(
                                RichText::new("+ Add Server")
                                    .color(p.accent_text)
                                    .strong(),
                            )
                            .fill(p.accent)
                            .corner_radius(CornerRadius::same(8));
                            if (ui.add(add_btn).clicked() || enter)
                                && !self.add_server_ip.trim().is_empty()
                            {
                                match ServerManager::add_server(
                                    &mc_dir,
                                    &self.add_server_name,
                                    &self.add_server_ip,
                                ) {
                                    Ok(updated) => {
                                        self.add_server_name.clear();
                                        self.add_server_ip.clear();
                                        self.servers_cache
                                            .insert(inst.instance_id.clone(), updated.clone());
                                        self.selected_servers = updated;
                                    }
                                    Err(e) => {
                                        self.notice_banner =
                                            Some((format!("Add server failed: {e}"), true));
                                    }
                                }
                            }
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(8.0);

                        let auth_notice_hint = self.t("accounts.auth_required_notice").to_string();
                        let mut remove_idx: Option<usize> = None;
                        let mut join_ip: Option<String> = None;

                        if loading && self.selected_servers.is_empty() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("Reading servers.dat...").color(p.text_muted));
                            });
                        } else if self.selected_servers.is_empty() {
                            ui.label(
                                RichText::new(
                                    "No saved servers in servers.dat yet. Add a server above to manage it here.",
                                )
                                .color(p.text_muted),
                            );
                        } else {
                            for (idx, srv) in self.selected_servers.iter().enumerate() {
                                Frame::new()
                                    .fill(p.elevated)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .corner_radius(CornerRadius::same(10))
                                    .inner_margin(egui::Margin::symmetric(12, 8))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                let title = if srv.name.is_empty() {
                                                    srv.ip.as_str()
                                                } else {
                                                    srv.name.as_str()
                                                };
                                                ui.label(RichText::new(title).strong().color(p.text));
                                                ui.label(
                                                    RichText::new(&srv.ip)
                                                        .monospace()
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                );
                                            });
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button(
                                                            RichText::new("Remove").color(p.danger),
                                                        )
                                                        .clicked()
                                                    {
                                                        remove_idx = Some(idx);
                                                    }
                                                    if ui.button("Copy IP").clicked() {
                                                        ctx.copy_text(srv.ip.clone());
                                                        self.notice_banner = Some((
                                                            format!("Copied '{}'.", srv.ip),
                                                            false,
                                                        ));
                                                    }
                                                    let can_join = quick_play_ok && !busy;
                                                    let join_btn = Button::new(
                                                        RichText::new("Join")
                                                            .color(p.accent_text)
                                                            .strong(),
                                                    )
                                                    .fill(p.accent)
                                                    .corner_radius(CornerRadius::same(8));
                                                    let resp = ui.add_enabled(can_join, join_btn);
                                                    if !quick_play_ok {
                                                        resp.on_disabled_hover_text(
                                                            "Direct server Quick Play requires Minecraft 1.20 or newer.",
                                                        );
                                                    } else if resp.clicked() {
                                                        join_ip = Some(srv.ip.clone());
                                                    } else if !has_account {
                                                        resp.on_hover_text(&auth_notice_hint);
                                                    }
                                                },
                                            );
                                        });
                                    });
                                ui.add_space(6.0);
                            }
                        }

                        if let Some(idx) = remove_idx {
                            match ServerManager::remove_server(&mc_dir, idx) {
                                Ok(updated) => {
                                    self.servers_cache
                                        .insert(inst.instance_id.clone(), updated.clone());
                                    self.selected_servers = updated;
                                }
                                Err(e) => {
                                    self.notice_banner =
                                        Some((format!("Remove server failed: {e}"), true));
                                }
                            }
                        }
                        if let Some(ip) = join_ip {
                            self.start_launch_with_server(ip, ctx);
                        }
                    });

                ui.add_space(12.0);

                // Card 2: Manual LAN Hosting (instant apply toggle)
                let mut open_lan_log = false;
                let mut saved_toggle_state: Option<bool> = None;
                let lan_log_btn_lbl = self.t("lan.agent.log.view").to_string();

                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("LAN Hosting")
                                .strong()
                                .size(15.5)
                                .color(p.text),
                        );
                        ui.label(
                            RichText::new(
                                "Allow offline players on your local LAN or VPN to join published LAN worlds. Applied immediately on toggle.",
                            )
                            .size(11.5)
                            .color(p.text_muted),
                        );
                        ui.add_space(8.0);

                        if let Some(ref mut settings) = self.selected_settings {
                            let mut lan_on = LanAgentManager::is_enabled(
                                settings.offline_multiplayer_enabled,
                                &settings.lan_auth_mode,
                            );
                            if ui
                                .checkbox(
                                    &mut lan_on,
                                    "Enable MCW LAN Agent (allow offline players to join LAN worlds)",
                                )
                                .changed()
                            {
                                settings.offline_multiplayer_enabled = lan_on;
                                settings.lan_auth_mode = if lan_on {
                                    "private_offline".to_string()
                                } else {
                                    "microsoft_only".to_string()
                                };
                                settings.lan_connection_provider = "manual".to_string();
                                saved_toggle_state = Some(lan_on);
                            }
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("Connection Mode: Manual (Local LAN / VPN)")
                                    .size(11.5)
                                    .color(p.text_muted),
                            );
                            ui.add_space(8.0);
                            if ui.button(&lan_log_btn_lbl).clicked() {
                                open_lan_log = true;
                            }
                        }
                    });

                if let Some(lan_on) = saved_toggle_state {
                    if let Some(ref s) = self.selected_settings {
                        let store = InstanceStore::new(&self.paths);
                        if let Err(e) = store.save_settings(&inst.instance_id, s) {
                            self.notice_banner =
                                Some((format!("Failed to save LAN settings: {e}"), true));
                        } else {
                            self.settings_cache
                                .insert(inst.instance_id.clone(), s.clone());
                            let msg = if lan_on {
                                "MCW LAN Agent enabled (Manual LAN).".to_string()
                            } else {
                                "MCW LAN Agent disabled.".to_string()
                            };
                            self.notice_banner = Some((msg, false));
                        }
                    }
                }
                if open_lan_log {
                    self.open_lan_agent_log_window(&inst.instance_id, ctx);
                }
            });
    }
}
