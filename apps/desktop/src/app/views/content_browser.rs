use crate::app::controllers_pages::{content_browser_window_title, BROWSER_PAGE_SIZE};
use crate::app::platform::*;
use crate::app::*;
use eframe::egui::Margin;

#[derive(Clone)]
struct BrowserEntry {
    key: String,
    title: String,
    author: String,
    summary: String,
    icon_url: Option<String>,
    web_url: Option<String>,
    metric_label: String,
    action: BrowserInstallAction,
}

#[derive(Clone)]
enum BrowserInstallAction {
    ModrinthModPicker {
        slug: String,
        title: String,
        icon: Option<String>,
        content_kind: Option<ContentKind>,
    },
    CurseForgeModPicker {
        mod_id: u64,
        title: String,
        icon: Option<String>,
        content_kind: Option<ContentKind>,
    },
    ModpackDialog {
        provider: ModpackInstallProvider,
        provider_label: &'static str,
        title: String,
        icon: Option<String>,
        web_url: Option<String>,
        default_loader: &'static str,
    },
    ContentPack {
        kind: ContentKind,
        provider_is_cf: bool,
        item_key: String,
        title: String,
        cf_mod_id: Option<u64>,
    },
}

fn format_compact_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

#[derive(Default)]
struct ContentBrowserActions {
    close_requested: bool,
    switch_category: Option<BrowserCategory>,
    switch_provider: Option<BrowserProvider>,
    trigger_search: bool,
    page_delta: i32,
    switch_instance: Option<String>,
    pending_install: Option<BrowserInstallAction>,
}

impl McwApp {
    fn prepare_content_browser_state(&mut self, ctx: &egui::Context) -> Vec<BrowserEntry> {
        let cat = self.browser_category;
        let prov = self.browser_provider;
        let entries = self.collect_browser_entries(cat, prov);
        let thumb_urls: Vec<String> = entries.iter().filter_map(|e| e.icon_url.clone()).collect();
        for url in thumb_urls {
            self.ensure_thumbnail(&url, ctx);
        }

        if let Some(ref sel) = self.browser_selected_key {
            if !entries.iter().any(|e| &e.key == sel) {
                self.browser_selected_key = entries.first().map(|e| e.key.clone());
            }
        } else if let Some(first) = entries.first() {
            self.browser_selected_key = Some(first.key.clone());
        }
        entries
    }

    /// Inline Mod & Modpacks browser page rendered inside the main workspace (matching Versions/Logs).
    pub(crate) fn render_modpacks_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.browser_global_mode = true;
        self.show_content_browser = false;
        let entries = self.prepare_content_browser_state(ctx);
        let actions = self.render_content_browser_body(ui, ctx, &entries, true);
        self.apply_content_browser_actions(actions, ctx);
    }

    /// Dedicated centered OS window for per-instance content browsing (+ Add Mod / Resource Pack / etc.).
    pub(crate) fn render_content_browser_window(&mut self, ctx: &egui::Context) {
        if !self.show_content_browser || self.browser_global_mode {
            return;
        }
        let p = self.palette;
        let entries = self.prepare_content_browser_state(ctx);
        let cat = self.browser_category;
        let mut window_closed = false;
        let mut actions = ContentBrowserActions::default();

        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_content_browser");
        let builder = centered_viewport_builder(
            ctx,
            content_browser_window_title(self.browser_global_mode, cat),
            [1020.0, 660.0],
            [760.0, 480.0],
        );

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested()) {
                window_closed = true;
            }
            if vp_ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                window_closed = true;
            }

            egui::CentralPanel::default()
                .frame(Frame::new().fill(p.canvas).inner_margin(Margin::same(16)))
                .show(vp_ctx, |ui| {
                    actions = self.render_content_browser_body(ui, vp_ctx, &entries, false);
                });
        });

        self.show_content_browser = !(window_closed || actions.close_requested);
        self.apply_content_browser_actions(actions, ctx);
    }

    fn render_content_browser_body(
        &mut self,
        ui: &mut egui::Ui,
        key_ctx: &egui::Context,
        entries: &[BrowserEntry],
        inline_page: bool,
    ) -> ContentBrowserActions {
        let p = self.palette;
        let cat = self.browser_category;
        let prov = self.browser_provider;
        let selected_inst = self.selected_instance().cloned();

        let target_subtitle = if cat == BrowserCategory::Modpacks {
            "Installs as a standalone new instance".to_string()
        } else if self.browser_global_mode {
            "Browse globally - click Install to pick an existing instance or create a new matching one."
                .to_string()
        } else if let Some(ref inst) = selected_inst {
            format!(
                "Target: {} (MC {} - {})",
                inst.name,
                inst.version_id,
                display_loader_name(&inst.mod_loader.name)
            )
        } else {
            "Select or create a Minecraft instance first".to_string()
        };

        let mut close_requested = false;
        let mut switch_category: Option<BrowserCategory> = None;
        let mut switch_provider: Option<BrowserProvider> = None;
        let mut trigger_search = false;
        let mut page_delta: i32 = 0;
        let mut switch_instance: Option<String> = None;
        let mut pending_install: Option<BrowserInstallAction> = None;

        // Row 1: Title + Subtitle (left) and optional Close button (right for OS window)
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(cat.label())
                        .size(if inline_page { 26.0 } else { 22.0 })
                        .strong()
                        .color(p.text),
                );
                ui.label(
                    RichText::new(&target_subtitle)
                        .size(12.0)
                        .color(p.text_muted),
                );
            });
            if !inline_page {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(Button::new("Close").corner_radius(CornerRadius::same(10)))
                        .clicked()
                    {
                        close_requested = true;
                    }
                });
            }
        });

        ui.add_space(6.0);

        // Row 2: Full-width Category Pill Bar (responsive, zero wrapping)
        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(6, 5))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    for c in [
                        BrowserCategory::Mods,
                        BrowserCategory::Modpacks,
                        BrowserCategory::ResourcePacks,
                        BrowserCategory::ShaderPacks,
                        BrowserCategory::DataPacks,
                    ] {
                        let active = c == cat;
                        let btn = Button::new(
                            RichText::new(c.label())
                                .size(12.0)
                                .strong()
                                .color(if active { p.text } else { p.text_muted }),
                        )
                        .fill(if active {
                            with_alpha(p.accent, 64)
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(if active {
                            Stroke::new(1.0_f32, p.accent)
                        } else {
                            Stroke::NONE
                        })
                        .corner_radius(CornerRadius::same(9));
                        if ui.add(btn).clicked() && !active {
                            switch_category = Some(c);
                        }
                    }
                });
            });

        ui.add_space(10.0);

        // Filter Bar (ComboBoxes per Rule 6!)
        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let te = ui.add(
                        TextEdit::singleline(&mut self.browser_query)
                            .hint_text(format!("Search {}...", cat.label()))
                            .desired_width(230.0),
                    );
                    let enter =
                        te.lost_focus() && key_ctx.input(|i| i.key_pressed(egui::Key::Enter));

                    let mut picked_prov = prov;
                    let prov_w = auto_combo_width(ui, picked_prov.label(), 96.0, 145.0);
                    egui::ComboBox::from_id_salt("cb_provider_combo")
                        .selected_text(picked_prov.label())
                        .width(prov_w)
                        .truncate()
                        .show_ui(ui, |ui| {
                            let list: &[BrowserProvider] = if cat == BrowserCategory::Modpacks {
                                &[
                                    BrowserProvider::Modrinth,
                                    BrowserProvider::CurseForge,
                                    BrowserProvider::Ftb,
                                    BrowserProvider::AtLauncher,
                                ]
                            } else {
                                &[BrowserProvider::Modrinth, BrowserProvider::CurseForge]
                            };
                            for &p_opt in list {
                                ui.selectable_value(&mut picked_prov, p_opt, p_opt.label());
                            }
                        });
                    if picked_prov != prov {
                        switch_provider = Some(picked_prov);
                    }

                    if matches!(
                        prov,
                        BrowserProvider::Modrinth | BrowserProvider::CurseForge
                    ) {
                        let prev_sort = self.browser_sort.clone();
                        let sort_lbl = match self.browser_sort.as_str() {
                            "downloads" => "Downloads",
                            "newest" => "Newest",
                            "updated" => "Updated",
                            _ => "Relevance",
                        };
                        let sort_w = auto_combo_width(ui, sort_lbl, 95.0, 140.0);
                        egui::ComboBox::from_id_salt("cb_sort_combo")
                            .selected_text(sort_lbl)
                            .width(sort_w)
                            .truncate()
                            .show_ui(ui, |ui| {
                                for (val, txt) in [
                                    ("relevance", "Relevance"),
                                    ("downloads", "Downloads"),
                                    ("newest", "Newest"),
                                    ("updated", "Updated"),
                                ] {
                                    ui.selectable_value(
                                        &mut self.browser_sort,
                                        val.to_string(),
                                        txt,
                                    );
                                }
                            });
                        if self.browser_sort != prev_sort {
                            trigger_search = true;
                        }

                        if matches!(cat, BrowserCategory::Mods | BrowserCategory::Modpacks) {
                            let prev_loader = self.browser_loader_filter.clone();
                            let cur_loader_lbl = display_loader_name(&self.browser_loader_filter);
                            let lw = auto_combo_width(ui, &cur_loader_lbl, 88.0, 140.0);
                            egui::ComboBox::from_id_salt("cb_loader_combo")
                                .selected_text(&cur_loader_lbl)
                                .width(lw)
                                .truncate()
                                .show_ui(ui, |ui| {
                                    for l in ["fabric", "forge", "neoforge", "quilt", "all"] {
                                        ui.selectable_value(
                                            &mut self.browser_loader_filter,
                                            l.to_string(),
                                            display_loader_name(l),
                                        );
                                    }
                                });
                            if self.browser_loader_filter != prev_loader {
                                trigger_search = true;
                            }
                        }
                    }

                    if self.browser_global_mode
                        && cat != BrowserCategory::Modpacks
                        && matches!(
                            prov,
                            BrowserProvider::Modrinth | BrowserProvider::CurseForge
                        )
                    {
                        let prev_mc = self.browser_mc_version_filter.clone();
                        let mc_lbl =
                            if prev_mc.trim().is_empty() || prev_mc.eq_ignore_ascii_case("all") {
                                "All Versions".to_string()
                            } else {
                                format!("MC {prev_mc}")
                            };
                        let mw = auto_combo_width(ui, &mc_lbl, 102.0, 155.0);
                        egui::ComboBox::from_id_salt("cb_mc_ver_filter_combo")
                            .selected_text(&mc_lbl)
                            .width(mw)
                            .truncate()
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.browser_mc_version_filter,
                                    String::new(),
                                    "All Versions",
                                );
                                for (vid, vtype) in &self.available_versions {
                                    if vtype != "release" {
                                        continue;
                                    }
                                    ui.selectable_value(
                                        &mut self.browser_mc_version_filter,
                                        vid.clone(),
                                        vid,
                                    );
                                }
                            });
                        if self.browser_mc_version_filter != prev_mc {
                            trigger_search = true;
                        }
                    } else if !self.browser_global_mode
                        && cat != BrowserCategory::Modpacks
                        && !self.instances.is_empty()
                    {
                        let cur_inst = selected_inst
                            .as_ref()
                            .map(|i| format!("{} ({})", i.name, i.version_id))
                            .unwrap_or_else(|| "Target Instance".to_string());
                        let iw = auto_combo_width(ui, &cur_inst, 130.0, 220.0);
                        egui::ComboBox::from_id_salt("cb_target_inst_combo")
                            .selected_text(&cur_inst)
                            .width(iw)
                            .truncate()
                            .show_ui(ui, |ui| {
                                for inst in &self.instances {
                                    let sel = self.selected_instance_id.as_deref()
                                        == Some(inst.instance_id.as_str());
                                    let lbl = format!("{} ({})", inst.name, inst.version_id);
                                    if ui.selectable_label(sel, lbl).clicked() && !sel {
                                        switch_instance = Some(inst.instance_id.clone());
                                    }
                                }
                            });
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let search_btn = Button::new(
                            RichText::new(if self.browser_searching {
                                "Searching..."
                            } else {
                                "Search"
                            })
                            .color(p.accent_text)
                            .strong(),
                        )
                        .fill(p.accent)
                        .corner_radius(CornerRadius::same(8));
                        if ui
                            .add_enabled(!self.browser_searching, search_btn)
                            .clicked()
                            || (enter && !self.browser_searching)
                        {
                            trigger_search = true;
                        }
                        if self.browser_searching {
                            ui.spinner();
                        }
                    });
                });
            });

        ui.add_space(10.0);

        // Split Body: Master Result List (left) + Detail Card (right)
        let avail = ui.available_size();
        let detail_w = (avail.x * 0.34).clamp(280.0, 340.0);
        let list_w = (avail.x - detail_w - 12.0).max(260.0);
        let body_h = avail.y.max(200.0);

        ui.horizontal_top(|ui| {
                        // Left Result List Panel (explicit top-down layout so cards stack vertically)
                        ui.allocate_ui_with_layout(
                            Vec2::new(list_w, body_h),
                            Layout::top_down(Align::Min),
                            |ui| {
                                Frame::new()
                                    .fill(p.surface)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .corner_radius(CornerRadius::same(12))
                                    .inner_margin(Margin::same(10))
                                    .show(ui, |ui| {
                                        ui.set_width((list_w - 22.0).max(180.0));
                                        ui.set_height((body_h - 22.0).max(160.0));
                                        let page_size = BROWSER_PAGE_SIZE.max(1);
                                        let total_pages = if self.browser_total_hits == 0 {
                                            1
                                        } else {
                                            self.browser_total_hits.div_ceil(page_size)
                                        };
                                        let current_page_disp = self.browser_page + 1;
                                        let can_prev =
                                            self.browser_page > 0 && !self.browser_searching;
                                        let can_next = !self.browser_searching
                                            && (self.browser_page + 1) < total_pages;

                                        ScrollArea::vertical()
                                            .id_salt("mcw_browser_results_scroll")
                                            .max_height((body_h - 64.0).max(120.0))
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                let card_w = (ui.available_width() - 6.0).max(180.0);
                                        if entries.is_empty() {
                                            ui.add_space(20.0);
                                            ui.vertical_centered(|ui| {
                                                let empty_msg = if self.browser_searching {
                                                    "Searching..."
                                                } else {
                                                    "No results. Enter a query or click Search."
                                                };
                                                ui.label(
                                                    RichText::new(empty_msg).color(p.text_muted),
                                                );
                                            });
                                        } else {
                                            for entry in entries {
                                                let is_sel = self
                                                    .browser_selected_key
                                                    .as_deref()
                                                    == Some(entry.key.as_str());
                                                let card_resp = Frame::new()
                                                    .fill(if is_sel {
                                                        with_alpha(p.accent, 36)
                                                    } else {
                                                        p.elevated
                                                    })
                                                    .stroke(Stroke::new(
                                                        1.0_f32,
                                                        if is_sel { p.accent } else { p.border },
                                                    ))
                                                    .corner_radius(CornerRadius::same(10))
                                                    .inner_margin(Margin::same(8))
                                                    .show(ui, |ui| {
                                                        ui.set_width(card_w);
                                                        ui.horizontal(|ui| {
                                                            self.render_thumbnail(
                                                                ui,
                                                                entry.icon_url.as_deref(),
                                                                40.0,
                                                            );
                                                            ui.add_space(6.0);
                                                            let text_w =
                                                                (card_w - 150.0).max(100.0);
                                                            ui.vertical(|ui| {
                                                                ui.set_max_width(text_w);
                                                                ui.horizontal_wrapped(|ui| {
                                                                    ui.label(
                                                                        RichText::new(&entry.title)
                                                                            .strong()
                                                                            .size(13.0)
                                                                            .color(p.text),
                                                                    );
                                                                    if !entry.author.is_empty() {
                                                                        ui.label(
                                                                            RichText::new(format!(
                                                                                "by {}",
                                                                                entry.author
                                                                            ))
                                                                            .size(11.0)
                                                                            .color(p.text_muted),
                                                                        );
                                                                    }
                                                                });
                                                                if !entry.summary.is_empty() {
                                                                    ui.add(
                                                                        egui::Label::new(
                                                                            RichText::new(
                                                                                &entry.summary,
                                                                            )
                                                                            .size(11.0)
                                                                            .color(p.text_muted),
                                                                        )
                                                                        .truncate(),
                                                                    );
                                                                }
                                                            });
                                                            ui.with_layout(
                                                                Layout::right_to_left(Align::Center),
                                                                |ui| {
                                                                    let can = self
                                                                        .can_install_browser_entry(
                                                                            cat,
                                                                            selected_inst.as_ref(),
                                                                        );
                                                                    let install_lbl =
                                                                        if self.browser_global_mode
                                                                            && cat != BrowserCategory::Modpacks
                                                                        {
                                                                            "Install..."
                                                                        } else {
                                                                            "Install"
                                                                        };
                                                                    let btn = Button::new(
                                                                        RichText::new(install_lbl)
                                                                            .color(p.accent_text)
                                                                            .strong()
                                                                            .size(11.5),
                                                                    )
                                                                    .fill(p.accent)
                                                                    .corner_radius(
                                                                        CornerRadius::same(8),
                                                                    );
                                                                    if ui
                                                                        .add_enabled(can, btn)
                                                                        .clicked()
                                                                    {
                                                                        pending_install = Some(
                                                                            entry.action.clone(),
                                                                        );
                                                                    }
                                                                    if !entry
                                                                        .metric_label
                                                                        .is_empty()
                                                                    {
                                                                        ui.label(
                                                                            RichText::new(
                                                                                &entry.metric_label,
                                                                            )
                                                                            .monospace()
                                                                            .size(10.5)
                                                                            .color(p.text_muted),
                                                                        );
                                                                    }
                                                                },
                                                            );
                                                        });
                                                    });
                                                if ui.rect_contains_pointer(card_resp.response.rect)
                                                    && key_ctx.input(|i| i.pointer.primary_clicked())
                                                {
                                                    self.browser_selected_key =
                                                        Some(entry.key.clone());
                                                }
                                                ui.add_space(6.0);
                                            }
                                        }
                                            });

                                        ui.add_space(4.0);
                                        ui.separator();
                                        ui.add_space(2.0);
                                        ui.horizontal(|ui| {
                                            if ui
                                                .add_enabled(
                                                    can_prev,
                                                    Button::new(
                                                        RichText::new("Previous").size(11.5),
                                                    )
                                                    .corner_radius(CornerRadius::same(7)),
                                                )
                                                .clicked()
                                            {
                                                page_delta = -1;
                                            }
                                            ui.add_space(6.0);
                                            let page_info = if self.browser_total_hits > 0 {
                                                format!(
                                                    "Page {} / {} ({} results)",
                                                    current_page_disp,
                                                    total_pages.max(current_page_disp),
                                                    self.browser_total_hits
                                                )
                                            } else {
                                                format!("Page {current_page_disp}")
                                            };
                                            ui.label(
                                                RichText::new(page_info)
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(p.text_muted),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if ui
                                                        .add_enabled(
                                                            can_next,
                                                            Button::new(
                                                                RichText::new("Next").size(11.5),
                                                            )
                                                            .corner_radius(CornerRadius::same(7)),
                                                        )
                                                        .clicked()
                                                    {
                                                        page_delta = 1;
                                                    }
                                                },
                                            );
                                        });
                                    });
                            },
                        );

                        ui.add_space(10.0);

                        // Right Detail Panel (explicit top-down layout)
                        ui.allocate_ui_with_layout(
                            Vec2::new(detail_w, body_h),
                            Layout::top_down(Align::Min),
                            |ui| {
                                Frame::new()
                                    .fill(p.surface)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .corner_radius(CornerRadius::same(12))
                                    .inner_margin(Margin::same(14))
                                    .show(ui, |ui| {
                                        ui.set_width((detail_w - 30.0).max(180.0));
                                        ui.set_height((body_h - 30.0).max(160.0));
                                        ScrollArea::vertical()
                                            .id_salt("mcw_browser_detail_scroll")
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                        let selected_entry = self
                                            .browser_selected_key
                                            .as_deref()
                                            .and_then(|k| entries.iter().find(|e| e.key == k));
                                        let Some(entry) = selected_entry else {
                                            ui.add_space(30.0);
                                            ui.vertical_centered(|ui| {
                                                ui.label(
                                                    RichText::new(
                                                        "Click any item on the left to inspect details.",
                                                    )
                                                    .color(p.text_muted),
                                                );
                                            });
                                            return;
                                        };

                                        self.render_thumbnail(
                                            ui,
                                            entry.icon_url.as_deref(),
                                            60.0,
                                        );
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new(&entry.title)
                                                .size(17.0)
                                                .strong()
                                                .color(p.text),
                                        );
                                        let meta = if entry.author.is_empty() {
                                            format!(
                                                "{} · {}",
                                                prov.label(),
                                                entry.metric_label
                                            )
                                        } else {
                                            format!(
                                                "by {} · {} · {}",
                                                entry.author,
                                                prov.label(),
                                                entry.metric_label
                                            )
                                        };
                                        ui.label(
                                            RichText::new(meta)
                                                .size(11.5)
                                                .color(p.text_muted),
                                        );
                                        ui.add_space(10.0);

                                        if !entry.summary.is_empty() {
                                            ui.label(
                                                RichText::new(&entry.summary)
                                                    .size(12.5)
                                                    .color(p.text),
                                            );
                                            ui.add_space(10.0);
                                        }

                                        // Compatibility note + DataPack world picker
                                        if !self.browser_global_mode && cat == BrowserCategory::DataPacks {
                                            ui.separator();
                                            ui.add_space(6.0);
                                            ui.label(
                                                RichText::new("Data Pack Target Folder")
                                                    .strong()
                                                    .size(12.0),
                                            );
                                            let cur_world_lbl =
                                                if self.browser_datapack_world.is_empty() {
                                                    "Global (.minecraft/datapacks)".to_string()
                                                } else {
                                                    format!("World: {}", self.browser_datapack_world)
                                                };
                                            egui::ComboBox::from_id_salt("cb_datapack_world_combo")
                                                .selected_text(&cur_world_lbl)
                                                .width(ui.available_width())
                                                .truncate()
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(
                                                        &mut self.browser_datapack_world,
                                                        String::new(),
                                                        "Global (.minecraft/datapacks)",
                                                    );
                                                    for w in &self.selected_worlds {
                                                        let lbl = format!(
                                                            "{} ({})",
                                                            w.display_name, w.folder_name
                                                        );
                                                        ui.selectable_value(
                                                            &mut self.browser_datapack_world,
                                                            w.folder_name.clone(),
                                                            lbl,
                                                        );
                                                    }
                                                });
                                            ui.add_space(8.0);
                                        }

                                        let compat_msg: Option<(String, Color32)> = if cat == BrowserCategory::Modpacks {
                                            Some((
                                                "Creates a separate new instance with its own mods and settings."
                                                    .to_string(),
                                                p.accent,
                                            ))
                                        } else if self.browser_global_mode {
                                            Some((
                                                "Choose an existing compatible instance or create a new matching instance on install."
                                                    .to_string(),
                                                p.accent,
                                            ))
                                        } else {
                                            match &selected_inst {
                                                None => Some((
                                                    "Select or create an instance to install content."
                                                        .to_string(),
                                                    p.warning,
                                                )),
                                                Some(inst) if cat == BrowserCategory::Mods && inst.mod_loader.is_vanilla() => Some((
                                                    "Vanilla instances cannot load mods. Switch to Fabric/Quilt/Forge/NeoForge in Settings."
                                                        .to_string(),
                                                    p.warning,
                                                )),
                                                Some(inst) => Some((
                                                    if cat == BrowserCategory::Mods {
                                                        format!(
                                                            "Opens version picker for MC {} ({}).",
                                                            inst.version_id,
                                                            display_loader_name(&inst.mod_loader.name)
                                                        )
                                                    } else {
                                                        format!(
                                                            "Installs compatible file for MC {} automatically.",
                                                            inst.version_id
                                                        )
                                                    },
                                                    p.text_muted,
                                                )),
                                            }
                                        };
                                        if let Some((msg, col)) = compat_msg {
                                            ui.label(RichText::new(msg).size(11.5).color(col));
                                        }
                                        ui.add_space(12.0);

                                        let can = self.can_install_browser_entry(
                                            cat,
                                            selected_inst.as_ref(),
                                        );
                                        let cta_lbl = match (&selected_inst, cat) {
                                            (_, BrowserCategory::Modpacks) => {
                                                "Install Modpack...".to_string()
                                            }
                                            (_, _) if self.browser_global_mode => {
                                                "Choose Instance & Install...".to_string()
                                            }
                                            (Some(inst), _) => format!("Install in {}", inst.name),
                                            (None, _) => "Select Instance First".to_string(),
                                        };
                                        let cta_btn = Button::new(
                                            RichText::new(cta_lbl)
                                                .color(p.accent_text)
                                                .strong()
                                                .size(13.0),
                                        )
                                        .fill(p.accent)
                                        .min_size(Vec2::new(ui.available_width(), 36.0))
                                        .corner_radius(CornerRadius::same(10));
                                        if ui.add_enabled(can, cta_btn).clicked() {
                                            pending_install = Some(entry.action.clone());
                                        }

                                        if let Some(ref url) = entry.web_url {
                                            ui.add_space(6.0);
                                            if ui
                                                .add(
                                                    Button::new("Open Page")
                                                        .min_size(Vec2::new(
                                                            ui.available_width(),
                                                            30.0,
                                                        ))
                                                        .corner_radius(CornerRadius::same(8)),
                                                )
                                                .clicked()
                                            {
                                                open_external(url);
                                            }
                                        }
                                            });
                                    });
                            },
                        );
                    });

        ContentBrowserActions {
            close_requested,
            switch_category,
            switch_provider,
            trigger_search,
            page_delta,
            switch_instance,
            pending_install,
        }
    }

    fn apply_content_browser_actions(
        &mut self,
        actions: ContentBrowserActions,
        ctx: &egui::Context,
    ) {
        if let Some(new_id) = actions.switch_instance {
            self.selected_instance_id = Some(new_id);
            self.sync_selected_from_cache();
            self.open_content_browser_mode(
                self.browser_category,
                Some(self.browser_provider),
                self.browser_global_mode,
                ctx,
            );
            return;
        }
        if let Some(new_cat) = actions.switch_category {
            self.open_content_browser_mode(new_cat, None, self.browser_global_mode, ctx);
            return;
        }
        if let Some(new_prov) = actions.switch_provider {
            self.open_content_browser_mode(
                self.browser_category,
                Some(new_prov),
                self.browser_global_mode,
                ctx,
            );
            return;
        }
        if actions.trigger_search {
            self.browser_page = 0;
            self.browser_selected_key = None;
            self.start_content_browser_search(ctx);
        } else if actions.page_delta != 0 {
            let target_page = if actions.page_delta < 0 {
                self.browser_page.saturating_sub(1)
            } else {
                self.browser_page.saturating_add(1)
            };
            if target_page != self.browser_page {
                self.browser_page = target_page;
                self.browser_selected_key = None;
                self.start_content_browser_search(ctx);
            }
        }
        if let Some(action) = actions.pending_install {
            match action {
                BrowserInstallAction::ModrinthModPicker {
                    slug,
                    title,
                    icon,
                    content_kind,
                } => {
                    self.open_modrinth_version_picker(
                        slug,
                        title,
                        icon,
                        false,
                        content_kind,
                        self.browser_global_mode,
                        ctx,
                    );
                }
                BrowserInstallAction::CurseForgeModPicker {
                    mod_id,
                    title,
                    icon,
                    content_kind,
                } => {
                    self.open_curseforge_version_picker(
                        mod_id,
                        title,
                        icon,
                        content_kind,
                        self.browser_global_mode,
                        ctx,
                    );
                }
                BrowserInstallAction::ModpackDialog {
                    provider,
                    provider_label,
                    title,
                    icon,
                    web_url,
                    default_loader,
                } => {
                    self.open_modpack_install_dialog(
                        provider,
                        provider_label,
                        title,
                        icon,
                        web_url,
                        default_loader,
                        ctx,
                    );
                }
                BrowserInstallAction::ContentPack {
                    kind,
                    provider_is_cf,
                    item_key,
                    title,
                    cf_mod_id,
                } => {
                    let world = if kind == ContentKind::DataPack
                        && !self.browser_datapack_world.trim().is_empty()
                    {
                        Some(self.browser_datapack_world.clone())
                    } else {
                        None
                    };
                    self.start_content_pack_install(
                        kind,
                        provider_is_cf,
                        item_key,
                        title,
                        cf_mod_id,
                        world,
                        ctx,
                    );
                }
            }
        }
    }

    fn can_install_browser_entry(&self, cat: BrowserCategory, inst: Option<&Instance>) -> bool {
        if cat == BrowserCategory::Modpacks || self.browser_global_mode {
            return true;
        }
        let Some(inst) = inst else {
            return false;
        };
        !(cat == BrowserCategory::Mods && inst.mod_loader.is_vanilla())
    }

    fn collect_browser_entries(
        &self,
        cat: BrowserCategory,
        prov: BrowserProvider,
    ) -> Vec<BrowserEntry> {
        match prov {
            BrowserProvider::Modrinth => self
                .browser_mr_results
                .iter()
                .map(|hit| {
                    let web = format!(
                        "https://modrinth.com/{}/{}",
                        cat.modrinth_project_type(),
                        hit.slug
                    );
                    let action = match cat {
                        BrowserCategory::Mods => BrowserInstallAction::ModrinthModPicker {
                            slug: hit.slug.clone(),
                            title: hit.title.clone(),
                            icon: hit.icon_url.clone(),
                            content_kind: None,
                        },
                        BrowserCategory::Modpacks => BrowserInstallAction::ModpackDialog {
                            provider: ModpackInstallProvider::Modrinth {
                                slug: hit.slug.clone(),
                            },
                            provider_label: "Modrinth",
                            title: hit.title.clone(),
                            icon: hit.icon_url.clone(),
                            web_url: Some(web.clone()),
                            default_loader: "fabric",
                        },
                        _ if self.browser_global_mode => BrowserInstallAction::ModrinthModPicker {
                            slug: hit.slug.clone(),
                            title: hit.title.clone(),
                            icon: hit.icon_url.clone(),
                            content_kind: cat.content_kind(),
                        },
                        _ => BrowserInstallAction::ContentPack {
                            kind: cat.content_kind().unwrap_or(ContentKind::ResourcePack),
                            provider_is_cf: false,
                            item_key: hit.slug.clone(),
                            title: hit.title.clone(),
                            cf_mod_id: None,
                        },
                    };
                    BrowserEntry {
                        key: format!("mr:{}", hit.slug),
                        title: hit.title.clone(),
                        author: hit.author.clone(),
                        summary: hit.description.clone(),
                        icon_url: hit.icon_url.clone(),
                        web_url: Some(web),
                        metric_label: format_compact_count(hit.downloads),
                        action,
                    }
                })
                .collect(),
            BrowserProvider::CurseForge => self
                .browser_cf_results
                .iter()
                .map(|cf| {
                    let web = cf
                        .links
                        .as_ref()
                        .and_then(|l| l.website_url.clone())
                        .unwrap_or_else(|| {
                            format!(
                                "https://www.curseforge.com/minecraft/search?search={}",
                                cf.slug
                            )
                        });
                    let icon = cf.icon_url().map(str::to_string);
                    let action = match cat {
                        BrowserCategory::Mods => BrowserInstallAction::CurseForgeModPicker {
                            mod_id: cf.id,
                            title: cf.name.clone(),
                            icon: icon.clone(),
                            content_kind: None,
                        },
                        BrowserCategory::Modpacks => BrowserInstallAction::ModpackDialog {
                            provider: ModpackInstallProvider::CurseForge {
                                mod_id: cf.id,
                                slug: cf.slug.clone(),
                            },
                            provider_label: "CurseForge",
                            title: cf.name.clone(),
                            icon: icon.clone(),
                            web_url: Some(web.clone()),
                            default_loader: "forge",
                        },
                        _ if self.browser_global_mode => {
                            BrowserInstallAction::CurseForgeModPicker {
                                mod_id: cf.id,
                                title: cf.name.clone(),
                                icon: icon.clone(),
                                content_kind: cat.content_kind(),
                            }
                        }
                        _ => BrowserInstallAction::ContentPack {
                            kind: cat.content_kind().unwrap_or(ContentKind::ResourcePack),
                            provider_is_cf: true,
                            item_key: cf.slug.clone(),
                            title: cf.name.clone(),
                            cf_mod_id: Some(cf.id),
                        },
                    };
                    BrowserEntry {
                        key: format!("cf:{}", cf.id),
                        title: cf.name.clone(),
                        author: String::new(),
                        summary: cf.summary.clone(),
                        icon_url: icon,
                        web_url: Some(web),
                        metric_label: format_compact_count(cf.download_count),
                        action,
                    }
                })
                .collect(),
            BrowserProvider::Ftb => self
                .browser_ftb_results
                .iter()
                .map(|pack| {
                    let web = pack.website_url();
                    let icon = pack.icon_url().map(str::to_string);
                    BrowserEntry {
                        key: format!("ftb:{}", pack.id),
                        title: pack.name.clone(),
                        author: "FTB".to_string(),
                        summary: pack.synopsis.clone(),
                        icon_url: icon.clone(),
                        web_url: Some(web.clone()),
                        metric_label: format_compact_count(pack.installs),
                        action: BrowserInstallAction::ModpackDialog {
                            provider: ModpackInstallProvider::Ftb { pack_id: pack.id },
                            provider_label: "FTB",
                            title: pack.name.clone(),
                            icon,
                            web_url: Some(web),
                            default_loader: "forge",
                        },
                    }
                })
                .collect(),
            BrowserProvider::AtLauncher => self
                .browser_atl_results
                .iter()
                .map(|pack| {
                    let web = pack.website_url();
                    let icon = Some(pack.icon_url());
                    BrowserEntry {
                        key: format!("atl:{}", pack.effective_safe_name()),
                        title: pack.name.clone(),
                        author: "ATLauncher".to_string(),
                        summary: pack.description.clone(),
                        icon_url: icon.clone(),
                        web_url: Some(web.clone()),
                        metric_label: format!("{} ver(s)", pack.versions.len()),
                        action: BrowserInstallAction::ModpackDialog {
                            provider: ModpackInstallProvider::AtLauncher { pack: pack.clone() },
                            provider_label: "ATLauncher",
                            title: pack.name.clone(),
                            icon,
                            web_url: Some(web),
                            default_loader: "forge",
                        },
                    }
                })
                .collect(),
        }
    }
}
