use crate::app::platform::*;
use crate::app::*;

/// Case-insensitive multi-token search matcher for Global Quick Search (`Ctrl+K`).
pub(crate) fn matches_quick_query(query: &str, fields: &[&str]) -> bool {
    let tokens: Vec<String> = query
        .split_whitespace()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.is_empty() {
        return true;
    }
    let combined = fields
        .iter()
        .map(|f| f.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    tokens.iter().all(|tok| combined.contains(tok))
}

#[derive(Clone)]
enum QuickAction {
    SelectInstance(String),
    GoPage(AppPage),
    OpenServersTab,
    OpenContent(BrowserCategory),
    CreateInstance,
    OpenSettings,
    OpenAccounts,
    RepairSelected(String),
    OpenGlobalLogs,
    OpenAbout,
}

impl McwApp {
    /// Horizon-style centered `Ctrl+K` Quick Search / Command Palette overlay.
    pub(crate) fn render_quick_search_modal(&mut self, ctx: &egui::Context) {
        if !self.show_quick_search {
            return;
        }
        let p = self.palette;
        let mut open = true;
        let screen = ctx.screen_rect();
        let modal_w = (screen.width() - 80.0).clamp(420.0, 640.0);
        let modal_h = (screen.height() - 120.0).clamp(320.0, 480.0);

        let opened_at = *self
            .quick_search_open_since
            .get_or_insert_with(Instant::now);
        let fade_secs = self.motion_secs(0.15);
        let fade = if fade_secs <= 0.0 {
            1.0
        } else {
            (opened_at.elapsed().as_secs_f32() / fade_secs).clamp(0.0, 1.0)
        };
        if fade < 1.0 {
            ctx.request_repaint();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        let enter_pressed = ctx.input(|i| i.key_pressed(egui::Key::Enter));

        egui::Area::new(egui::Id::new("mcw_quick_search_backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    0.0,
                    Color32::from_black_alpha((150.0 * fade) as u8),
                );
                if resp.clicked() {
                    open = false;
                }
            });

        let q = self.quick_search_query.clone();
        let selected_id = self.selected_instance_id.clone();

        // Collect matching items in deterministic priority order:
        // Instances -> Pages -> Content Browser -> Actions
        let matching_instances: Vec<Instance> = self
            .instances
            .iter()
            .filter(|inst| {
                let tags_joined = inst.tags.join(" ");
                matches_quick_query(
                    &q,
                    &[
                        &inst.name,
                        &inst.version_id,
                        &inst.mod_loader.name,
                        &inst.group,
                        &tags_joined,
                    ],
                )
            })
            .cloned()
            .collect();

        let page_entries: Vec<(AppPage, String, &'static str)> = vec![
            (
                AppPage::Home,
                self.t("navigation.home").to_string(),
                "Instance overview & launch",
            ),
            (
                AppPage::Versions,
                "Versions".to_string(),
                "Cached Minecraft versions & manifests",
            ),
            (
                AppPage::Logs,
                self.t("navigation.logs").to_string(),
                "Global launcher & instance logs",
            ),
        ]
        .into_iter()
        .filter(|(_, label, sub)| matches_quick_query(&q, &[label, sub, "page"]))
        .collect();

        let content_entries: Vec<(BrowserCategory, String)> = [
            BrowserCategory::Mods,
            BrowserCategory::Modpacks,
            BrowserCategory::ResourcePacks,
            BrowserCategory::ShaderPacks,
            BrowserCategory::DataPacks,
        ]
        .into_iter()
        .filter_map(|cat| {
            let lbl = cat.label().to_string();
            if matches_quick_query(&q, &[&lbl, "browse install content modrinth curseforge"]) {
                Some((cat, lbl))
            } else {
                None
            }
        })
        .collect();

        let mut action_entries: Vec<(QuickAction, String, &'static str)> = vec![
            (
                QuickAction::CreateInstance,
                self.t("instance.create.title").to_string(),
                "New custom instance or modpack archive",
            ),
            (
                QuickAction::OpenServersTab,
                "Servers & LAN".to_string(),
                "Saved servers & manual LAN hosting for selected instance",
            ),
            (
                QuickAction::OpenSettings,
                self.t("navigation.launcher_settings").to_string(),
                "Themes, background, Java, downloads, storage",
            ),
            (
                QuickAction::OpenAccounts,
                self.t("navigation.accounts").to_string(),
                "Manage Microsoft & Offline accounts",
            ),
            (
                QuickAction::OpenGlobalLogs,
                format!("{} (OS Window)", self.t("navigation.logs")),
                "Pop out global launcher logs window",
            ),
            (
                QuickAction::OpenAbout,
                self.t("navigation.about").to_string(),
                "Version, system RAM, account security, GitHub",
            ),
        ];
        if let Some(ref inst_id) = selected_id {
            action_entries.insert(
                1,
                (
                    QuickAction::RepairSelected(inst_id.clone()),
                    self.t("repair.button").to_string(),
                    "Verify files & check mod compatibility",
                ),
            );
        }
        let matching_actions: Vec<(QuickAction, String, &'static str)> = action_entries
            .into_iter()
            .filter(|(_, lbl, sub)| matches_quick_query(&q, &[lbl, sub, "action"]))
            .collect();

        // First matching action for Enter key
        let first_match: Option<QuickAction> = if let Some(first_inst) = matching_instances.first()
        {
            Some(QuickAction::SelectInstance(first_inst.instance_id.clone()))
        } else if let Some((page, _, _)) = page_entries.first() {
            Some(QuickAction::GoPage(*page))
        } else if let Some((cat, _)) = content_entries.first() {
            Some(QuickAction::OpenContent(*cat))
        } else {
            matching_actions.first().map(|(a, _, _)| a.clone())
        };

        let mut triggered: Option<QuickAction> = None;
        if enter_pressed {
            triggered = first_match;
        }

        let search_lbl = self.t("common.search").to_string();
        let clear_lbl = self.t("common.clear").to_string();

        egui::Window::new("Quick Search")
            .id(egui::Id::new("mcw_quick_search_modal"))
            .order(egui::Order::Foreground)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([modal_w, modal_h])
            .frame(
                Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(
                        p.canvas.r(),
                        p.canvas.g(),
                        p.canvas.b(),
                        248,
                    ))
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(16))
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.set_opacity(0.45 + 0.55 * fade);

                // Search input bar
                ui.horizontal(|ui| {
                    let input_w = (ui.available_width()
                        - if self.quick_search_query.is_empty() {
                            52.0
                        } else {
                            116.0
                        })
                    .max(180.0);
                    let edit = ui.add(
                        TextEdit::singleline(&mut self.quick_search_query)
                            .hint_text(format!("{search_lbl} instances, pages, mods, actions..."))
                            .desired_width(input_w),
                    );
                    if self.quick_search_focus_pending {
                        edit.request_focus();
                        self.quick_search_focus_pending = false;
                    }
                    if !self.quick_search_query.is_empty()
                        && ui
                            .add(Button::new(&clear_lbl).corner_radius(CornerRadius::same(8)))
                            .clicked()
                    {
                        self.quick_search_query.clear();
                        edit.request_focus();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        Frame::new()
                            .fill(p.elevated)
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(6, 3))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("Esc")
                                        .monospace()
                                        .size(10.5)
                                        .color(p.text_muted),
                                );
                            });
                    });
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);

                let total_matches = matching_instances.len()
                    + page_entries.len()
                    + content_entries.len()
                    + matching_actions.len();

                ScrollArea::vertical()
                    .id_salt("mcw_quick_search_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        if total_matches == 0 {
                            ui.add_space(36.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("No matching instances, pages, or actions.")
                                        .color(p.text_muted),
                                );
                            });
                            return;
                        }

                        if !matching_instances.is_empty() {
                            ui.label(
                                RichText::new("INSTANCES")
                                    .strong()
                                    .size(10.5)
                                    .color(p.text_muted),
                            );
                            ui.add_space(4.0);
                            for inst in &matching_instances {
                                let is_sel =
                                    selected_id.as_deref() == Some(inst.instance_id.as_str());
                                let row_resp = Frame::new()
                                    .fill(if is_sel {
                                        p.accent.gamma_multiply(0.18)
                                    } else {
                                        p.surface
                                    })
                                    .stroke(Stroke::new(
                                        1.0_f32,
                                        if is_sel { p.accent } else { p.border },
                                    ))
                                    .corner_radius(CornerRadius::same(10))
                                    .inner_margin(egui::Margin::symmetric(10, 7))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            self.render_instance_icon(ui, inst, 26.0);
                                            ui.add_space(4.0);
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(&inst.name)
                                                        .strong()
                                                        .size(13.0)
                                                        .color(p.text),
                                                );
                                                ui.label(
                                                    RichText::new(format!(
                                                        "MC {} - {}",
                                                        inst.version_id,
                                                        display_loader_name(&inst.mod_loader.name)
                                                    ))
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(p.text_muted),
                                                );
                                            });
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if !inst.group.trim().is_empty() {
                                                        render_badge_pill(
                                                            ui,
                                                            &inst.group,
                                                            true,
                                                            true,
                                                            p,
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                    });
                                let interact = ui.interact(
                                    row_resp.response.rect,
                                    egui::Id::new(("qs_inst", &inst.instance_id)),
                                    egui::Sense::click(),
                                );
                                if interact.clicked() {
                                    triggered =
                                        Some(QuickAction::SelectInstance(inst.instance_id.clone()));
                                }
                                ui.add_space(4.0);
                            }
                            ui.add_space(6.0);
                        }

                        if !page_entries.is_empty() {
                            ui.label(
                                RichText::new("PAGES")
                                    .strong()
                                    .size(10.5)
                                    .color(p.text_muted),
                            );
                            ui.add_space(4.0);
                            for (page, label, sub) in &page_entries {
                                if quick_row_button(ui, p, label, sub, "Page").clicked() {
                                    triggered = Some(QuickAction::GoPage(*page));
                                }
                                ui.add_space(4.0);
                            }
                            ui.add_space(6.0);
                        }

                        if !content_entries.is_empty() {
                            ui.label(
                                RichText::new("CONTENT BROWSER")
                                    .strong()
                                    .size(10.5)
                                    .color(p.text_muted),
                            );
                            ui.add_space(4.0);
                            for (cat, label) in &content_entries {
                                if quick_row_button(
                                    ui,
                                    p,
                                    label,
                                    "Open Content Browser OS window",
                                    "Browse",
                                )
                                .clicked()
                                {
                                    triggered = Some(QuickAction::OpenContent(*cat));
                                }
                                ui.add_space(4.0);
                            }
                            ui.add_space(6.0);
                        }

                        if !matching_actions.is_empty() {
                            ui.label(
                                RichText::new("ACTIONS")
                                    .strong()
                                    .size(10.5)
                                    .color(p.text_muted),
                            );
                            ui.add_space(4.0);
                            for (action, label, sub) in &matching_actions {
                                if quick_row_button(ui, p, label, sub, "Action").clicked() {
                                    triggered = Some(action.clone());
                                }
                                ui.add_space(4.0);
                            }
                        }
                    });
            });

        if let Some(action) = triggered {
            self.show_quick_search = false;
            self.quick_search_open_since = None;
            match action {
                QuickAction::SelectInstance(id) => {
                    self.selected_instance_id = Some(id);
                    self.active_page = AppPage::Home;
                    self.sync_selected_from_cache();
                    self.ensure_active_tab_loaded(ctx);
                }
                QuickAction::GoPage(page) => {
                    self.active_page = page;
                    match page {
                        AppPage::Home => self.ensure_active_tab_loaded(ctx),
                        AppPage::Versions => {
                            if self.cached_versions.is_empty() {
                                self.load_cached_versions_async(ctx);
                            }
                        }
                        AppPage::Logs => {}
                    }
                }
                QuickAction::OpenServersTab => {
                    self.active_page = AppPage::Home;
                    self.active_tab = DetailTab::Servers;
                    self.ensure_active_tab_loaded(ctx);
                }
                QuickAction::OpenContent(cat) => {
                    self.open_content_browser_mode(cat, None, true, ctx);
                }
                QuickAction::CreateInstance => {
                    self.open_create_instance_dialog(ctx);
                }
                QuickAction::OpenSettings => {
                    self.show_launcher_settings = true;
                }
                QuickAction::OpenAccounts => {
                    self.open_accounts_window(ctx);
                }
                QuickAction::RepairSelected(id) => {
                    self.open_repair_center(&id, ctx);
                }
                QuickAction::OpenGlobalLogs => {
                    self.open_global_logs_window(ctx);
                }
                QuickAction::OpenAbout => {
                    self.open_about_window(ctx);
                }
            }
        } else if !open {
            self.show_quick_search = false;
            self.quick_search_open_since = None;
        }
    }
}

fn quick_row_button(
    ui: &mut egui::Ui,
    p: ThemePalette,
    title: &str,
    subtitle: &str,
    badge: &str,
) -> egui::Response {
    let resp = Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).strong().size(13.0).color(p.text));
                    ui.label(RichText::new(subtitle).size(11.0).color(p.text_muted));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(badge)
                            .monospace()
                            .size(10.5)
                            .color(p.text_muted),
                    );
                });
            });
        })
        .response;
    ui.interact(
        resp.rect,
        ui.make_persistent_id(("qs_row", title, badge)),
        egui::Sense::click(),
    )
}

#[cfg(test)]
mod tests {
    use super::matches_quick_query;

    #[test]
    fn quick_query_empty_matches_everything() {
        assert!(matches_quick_query("", &["Horizon", "1.21.4"]));
        assert!(matches_quick_query("   ", &["Servers"]));
    }

    #[test]
    fn quick_query_requires_all_whitespace_tokens_case_insensitive() {
        assert!(matches_quick_query(
            "fab 1.21",
            &["My Pack", "1.21.4", "fabric", "Survival"]
        ));
        assert!(!matches_quick_query(
            "fab forge",
            &["My Pack", "1.21.4", "fabric", "Survival"]
        ));
    }
}
