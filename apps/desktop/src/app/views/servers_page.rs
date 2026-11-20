use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_servers_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.palette;

        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Servers & LAN").size(28.0).strong().color(p.text));
                ui.label(
                    RichText::new(
                        "Manage saved servers in servers.dat and offline LAN / e4mc hosting per instance.",
                    )
                    .size(12.5)
                    .color(p.text_muted),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.instances.is_empty() {
                    let current_name = self
                        .selected_instance()
                        .map(|i| format!("{} ({})", i.name, i.version_id))
                        .unwrap_or_else(|| "Select instance".to_string());
                    let mut picked_id: Option<String> = None;
                    let w = auto_combo_width(ui, &current_name, 180.0, 300.0);
                    egui::ComboBox::from_id_salt("servers_page_instance_combo")
                        .selected_text(&current_name)
                        .width(w)
                        .truncate()
                        .show_ui(ui, |ui| {
                            for inst in &self.instances {
                                let selected = self.selected_instance_id.as_deref()
                                    == Some(inst.instance_id.as_str());
                                let lbl = format!("{} ({})", inst.name, inst.version_id);
                                if ui.selectable_label(selected, lbl).clicked() && !selected {
                                    picked_id = Some(inst.instance_id.clone());
                                }
                            }
                        });
                    ui.label(RichText::new("Instance:").color(p.text_muted));
                    if let Some(new_id) = picked_id {
                        self.selected_instance_id = Some(new_id.clone());
                        self.sync_selected_from_cache();
                        self.load_servers_async(&new_id, ctx, false);
                    }
                }
            });
        });

        ui.add_space(12.0);

        let Some(inst) = self.selected_instance().cloned() else {
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(egui::Margin::same(24))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("Create or select an instance to manage saved servers.")
                                .color(p.text_muted),
                        );
                        ui.add_space(8.0);
                        if ui
                            .add(
                                Button::new(
                                    RichText::new("+ Create Instance")
                                        .color(p.accent_text)
                                        .strong(),
                                )
                                .fill(p.accent)
                                .corner_radius(CornerRadius::same(10)),
                            )
                            .clicked()
                        {
                            self.open_create_instance_dialog();
                        }
                    });
                });
            return;
        };

        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let busy = self.is_instance_busy(&inst.instance_id);
        let has_account = self.active_account().is_some();
        let quick_play_ok = is_quick_play_supported(&inst.version_id);
        let loading = self.loading_servers.contains(&inst.instance_id);

        ScrollArea::vertical()
            .id_salt("servers_page_scroll")
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
                                                    let can_join =
                                                        quick_play_ok && has_account && !busy;
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
                                                    } else if !has_account {
                                                        resp.on_disabled_hover_text(
                                                            "Sign in or add an account first.",
                                                        );
                                                    } else if resp.clicked() {
                                                        join_ip = Some(srv.ip.clone());
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

                // Card 2: MCW LAN Hosting & e4mc Tunnel
                let mut open_lan_log = false;
                let mut prepare_lan_provider: Option<String> = None;
                let mut save_lan = false;
                let lan_log_btn_lbl = self.t("lan.agent.log.view").to_string();

                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("LAN Hosting & e4mc Tunnel")
                                .strong()
                                .size(15.5)
                                .color(p.text),
                        );
                        ui.label(
                            RichText::new(
                                "Allow offline players on your LAN/tunnel and attach e4mc without leaving the Servers view.",
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
                                save_lan = true;
                            }
                            ui.add_space(6.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.label("LAN Connection Provider:");
                                let is_e4mc =
                                    settings.lan_connection_provider.eq_ignore_ascii_case("e4mc");
                                let cur_lbl = if is_e4mc {
                                    "e4mc (Public Tunnel — auto-installs e4mc mod)"
                                } else {
                                    "Manual (Local LAN / VPN)"
                                };
                                let w = auto_combo_width(ui, cur_lbl, 220.0, 340.0);
                                egui::ComboBox::from_id_salt("servers_page_lan_provider_combo")
                                    .selected_text(cur_lbl)
                                    .width(w)
                                    .truncate()
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_label(!is_e4mc, "Manual (Local LAN / VPN)")
                                            .clicked()
                                            && is_e4mc
                                        {
                                            settings.lan_connection_provider = "manual".to_string();
                                            prepare_lan_provider = Some("manual".to_string());
                                            save_lan = true;
                                        }
                                        if ui
                                            .selectable_label(
                                                is_e4mc,
                                                "e4mc (Public Tunnel — auto-installs e4mc mod)",
                                            )
                                            .clicked()
                                            && !is_e4mc
                                        {
                                            settings.lan_connection_provider = "e4mc".to_string();
                                            prepare_lan_provider = Some("e4mc".to_string());
                                            save_lan = true;
                                        }
                                    });
                            });
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                if ui.button(&lan_log_btn_lbl).clicked() {
                                    open_lan_log = true;
                                }
                                if ui
                                    .add(
                                        Button::new(
                                            RichText::new("Apply LAN Hosting")
                                                .color(p.accent_text)
                                                .strong(),
                                        )
                                        .fill(p.accent)
                                        .corner_radius(CornerRadius::same(8)),
                                    )
                                    .clicked()
                                {
                                    prepare_lan_provider =
                                        Some(settings.lan_connection_provider.clone());
                                    save_lan = true;
                                }
                            });
                        }
                    });

                if save_lan {
                    if let Some(ref s) = self.selected_settings {
                        let store = InstanceStore::new(&self.paths);
                        if let Err(e) = store.save_settings(&inst.instance_id, s) {
                            self.notice_banner =
                                Some((format!("Failed to save LAN settings: {e}"), true));
                        } else {
                            self.settings_cache
                                .insert(inst.instance_id.clone(), s.clone());
                        }
                    }
                }
                if let Some(prov) = prepare_lan_provider {
                    self.start_prepare_lan_hosting(prov, ctx);
                }
                if open_lan_log {
                    self.open_lan_agent_log_window(&inst.instance_id);
                }
            });
    }
}
