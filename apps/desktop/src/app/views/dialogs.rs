use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_modals(&mut self, ctx: &egui::Context) {
        let p = self.palette;

        // 1. Create Instance (2 tabs: Custom | Modpacks) Isolated OS Window
        if self.show_create_modal {
            let mut open = self.show_create_modal;
            let vp_id = egui::ViewportId::from_hash_of("mcw_vp_create_instance");
            let builder = centered_viewport_builder(
                ctx,
                "Create Instance — MCW Launcher",
                [580.0, 480.0],
                [460.0, 380.0],
            );
            ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                if vp_ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
                egui::CentralPanel::default()
                    .frame(
                        Frame::new()
                            .fill(p.canvas)
                            .inner_margin(egui::Margin::same(14)),
                    )
                    .show(vp_ctx, |ui| {
                        let is_custom = matches!(self.create_tab, CreateInstanceTab::Custom);
                        ui.horizontal(|ui| {
                            if ui.selectable_label(is_custom, "Custom").clicked() {
                                self.create_tab = CreateInstanceTab::Custom;
                            }
                            if ui.selectable_label(!is_custom, "Modpacks").clicked() {
                                self.create_tab = CreateInstanceTab::ImportArchive;
                            }
                        });
                        ui.separator();
                        ui.add_space(8.0);

                        if is_custom {
                            ui.label(self.t("common.name"));
                            ui.text_edit_singleline(&mut self.create_name);
                            ui.add_space(6.0);

                            ui.label(self.t("instance.minecraft_version"));
                            let snap_label =
                                self.t("instance.create.show_non_release").to_string();
                            ui.checkbox(&mut self.create_show_snapshots, snap_label);
                            egui::ComboBox::from_id_salt("create_mc_version")
                                .selected_text(&self.create_version)
                                .width(260.0)
                                .show_ui(ui, |ui| {
                                    for (ver_id, ver_type) in &self.available_versions {
                                        if !self.create_show_snapshots && ver_type != "release" {
                                            continue;
                                        }
                                        let label = if ver_type == "release" {
                                            ver_id.clone()
                                        } else {
                                            format!("{ver_id} ({ver_type})")
                                        };
                                        ui.selectable_value(
                                            &mut self.create_version,
                                            ver_id.clone(),
                                            label,
                                        );
                                    }
                                });
                            ui.add_space(6.0);

                            ui.label(self.t("instance.mod_loader"));
                            egui::ComboBox::from_id_salt("create_mod_loader_combo")
                                .selected_text(&self.create_loader)
                                .width(200.0)
                                .show_ui(ui, |ui| {
                                    for loader in ["vanilla", "fabric", "quilt", "forge", "neoforge"]
                                    {
                                        ui.selectable_value(
                                            &mut self.create_loader,
                                            loader.to_string(),
                                            loader,
                                        );
                                    }
                                });

                            if let Some(ref err) = self.create_error {
                                ui.add_space(4.0);
                                ui.label(RichText::new(err).color(p.danger));
                            }

                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                let create_btn = Button::new(
                                    RichText::new(self.t("instance.create.title"))
                                        .color(p.accent_text)
                                        .strong(),
                                )
                                .fill(p.accent);
                                if ui.add(create_btn).clicked() {
                                    let store = InstanceStore::new(&self.paths);
                                    let mod_loader = ModLoader {
                                        name: self.create_loader.clone(),
                                        version: if self.create_loader == "vanilla" {
                                            "-1".to_string()
                                        } else {
                                            "latest".to_string()
                                        },
                                    };
                                    match store.create_instance(
                                        &self.create_name,
                                        &self.create_version,
                                        mod_loader,
                                    ) {
                                        Ok((created, mut inst_settings)) => {
                                            inst_settings.min_memory =
                                                self.prefs.default_min_memory;
                                            inst_settings.max_memory =
                                                self.prefs.default_max_memory;
                                            inst_settings.width = self.prefs.default_width;
                                            inst_settings.height = self.prefs.default_height;
                                            inst_settings.fullscreen =
                                                self.prefs.default_fullscreen;
                                            let _ = store.save_settings(
                                                &created.instance_id,
                                                &inst_settings,
                                            );
                                            self.show_create_modal = false;
                                            self.selected_instance_id = Some(created.instance_id);
                                            self.reload_instances();
                                            self.notice_banner = Some((
                                                format!("Created instance '{}'.", created.name),
                                                false,
                                            ));
                                        }
                                        Err(e) => {
                                            self.create_error = Some(e.to_string());
                                        }
                                    }
                                }
                                if ui.button(self.t("common.cancel")).clicked() {
                                    self.show_create_modal = false;
                                }
                            });
                        } else {
                            // Modpacks tab: Browse Online Providers (each opens its own isolated OS window) + Import Local Archive
                            Frame::new()
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .corner_radius(CornerRadius::same(3))
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        RichText::new("Browse Online Modpack Providers")
                                            .strong()
                                            .color(p.text),
                                    );
                                    ui.label(
                                        RichText::new(
                                            "Select a modpack provider below to open its dedicated browser window.",
                                        )
                                        .size(11.5)
                                        .color(p.text_muted),
                                    );
                                    ui.add_space(8.0);
                                    ui.horizontal_wrapped(|ui| {
                                        let mr_btn = Button::new(
                                            RichText::new("Browse Modrinth Modpacks...")
                                                .color(p.accent_text)
                                                .strong(),
                                        )
                                        .fill(p.accent);
                                        if ui.add(mr_btn).clicked() {
                                            self.pack_loader_filter = "fabric".to_string();
                                            self.show_modpack_provider_window =
                                                Some(CreateInstanceTab::ModrinthPacks);
                                        }
                                        if ui.button("Browse CurseForge Modpacks...").clicked() {
                                            self.pack_loader_filter = "forge".to_string();
                                            self.show_modpack_provider_window =
                                                Some(CreateInstanceTab::CurseForgePacks);
                                        }
                                        if ui.button("Browse FTB Modpacks...").clicked() {
                                            self.pack_loader_filter = "forge".to_string();
                                            self.show_modpack_provider_window =
                                                Some(CreateInstanceTab::Ftb);
                                        }
                                        if ui.button("Browse ATLauncher Modpacks...").clicked() {
                                            self.pack_loader_filter = "forge".to_string();
                                            self.show_modpack_provider_window =
                                                Some(CreateInstanceTab::AtLauncher);
                                        }
                                    });
                                });

                            ui.add_space(10.0);

                            Frame::new()
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .corner_radius(CornerRadius::same(3))
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        RichText::new(
                                            "Import Local Archive (.mcwpack, .mrpack, or CurseForge .zip)",
                                        )
                                        .strong()
                                        .color(p.text),
                                    );
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            TextEdit::singleline(&mut self.modpack_path_input)
                                                .hint_text(
                                                    r"C:\Users\...\Downloads\modpack.mcwpack / .mrpack / .zip",
                                                )
                                                .desired_width(380.0),
                                        );
                                        if ui.button(self.t("common.browse")).clicked() {
                                            if let Some(p) = pick_file_dialog(
                                                "Select Modpack / Instance Package",
                                                "MCW & Modpack Archives (*.mcwpack;*.mrpack;*.zip)|*.mcwpack;*.mrpack;*.zip|All Files (*.*)|*.*",
                                            ) {
                                                self.modpack_path_input = p.display().to_string();
                                            }
                                        }
                                    });
                                    ui.add_space(6.0);
                                    ui.label("Instance Name Override (optional):");
                                    ui.add(
                                        TextEdit::singleline(&mut self.modpack_name_input)
                                            .hint_text("Leave blank to use modpack name")
                                            .desired_width(300.0),
                                    );

                                    if !self.modpack_manual_downloads.is_empty() {
                                        ui.add_space(8.0);
                                        Frame::new()
                                            .fill(p.elevated)
                                            .stroke(Stroke::new(1.0_f32, p.warning))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(8))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "Manual downloads required ({} files):",
                                                        self.modpack_manual_downloads.len()
                                                    ))
                                                    .strong()
                                                    .color(p.warning),
                                                );
                                                ScrollArea::vertical()
                                                    .max_height(110.0)
                                                    .show(ui, |ui| {
                                                        for item in &self.modpack_manual_downloads {
                                                            ui.horizontal(|ui| {
                                                                ui.label(
                                                                    RichText::new(&item.file_name)
                                                                        .monospace()
                                                                        .size(11.0),
                                                                );
                                                                if ui.button("Open URL").clicked() {
                                                                    open_external(&item.website_url);
                                                                }
                                                            });
                                                        }
                                                    });
                                            });
                                    }

                                    ui.add_space(10.0);
                                    ui.horizontal(|ui| {
                                        let import_btn = Button::new(
                                            RichText::new(if self.installing_archive_count > 0 {
                                                "Import Another Modpack"
                                            } else {
                                                "Import Archive"
                                            })
                                            .color(p.accent_text)
                                            .strong(),
                                        )
                                        .fill(p.accent);
                                        if ui.add(import_btn).clicked() {
                                            self.start_modpack_install(ctx);
                                        }
                                        if ui.button("Close").clicked() {
                                            self.show_create_modal = false;
                                        }
                                    });
                                });
                        }
                    });
            });
            if !open {
                self.show_create_modal = false;
            }
        }

        // 1a-0. Dedicated Isolated OS Windows for Online Resource Browsers (Modpack Providers, Mods, Resource/Shader Packs)
        self.render_modpack_provider_window(ctx);
        self.render_mod_browser_window(ctx);
        self.render_pack_browser_window(ctx);

        // 1a. Modpack Install Configuration Isolated OS Window (Version + Beta/Alpha + RAM + Java + Open Web)
        self.render_modpack_install_dialog(ctx);

        // 1b. 3-Mode Export Modpack Isolated OS Window (mirroring Python ModpackExportDialog, using Dropdowns)
        if self.show_export_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                let title = self.t("modpack_package.export.title").to_string();
                let mut trigger_export: Option<ModpackExportOptions> = None;
                let mut open = self.show_export_modal;
                let vp_id = egui::ViewportId::from_hash_of("mcw_vp_export_modpack");
                let builder = centered_viewport_builder(
                    ctx,
                    format!("{title} — {}", inst.name),
                    [540.0, 420.0],
                    [420.0, 320.0],
                );
                ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                    if vp_ctx.input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    egui::CentralPanel::default()
                        .frame(
                            Frame::new()
                                .fill(p.canvas)
                                .inner_margin(egui::Margin::same(14)),
                        )
                        .show(vp_ctx, |ui| {
                            ScrollArea::vertical().show(ui, |ui| {
                                ui.label(
                                    RichText::new(self.t("modpack_package.export.heading"))
                                        .strong()
                                        .size(14.5),
                                );
                                ui.label(
                                    RichText::new(self.t("modpack_package.export.description"))
                                        .size(11.5)
                                        .color(p.text_muted),
                                );
                                ui.add_space(10.0);

                                let mrpack_lbl =
                                    self.t("modpack_package.export.mrpack").to_string();
                                let provider_lbl = self
                                    .t("modpack_package.export.provider_profile")
                                    .to_string();
                                let portable_lbl =
                                    self.t("modpack_package.export.portable").to_string();

                                let is_mrpack =
                                    matches!(self.export_mode, ModpackExportMode::Mrpack);
                                let is_provider =
                                    matches!(self.export_mode, ModpackExportMode::ProviderProfile);
                                let is_portable =
                                    matches!(self.export_mode, ModpackExportMode::Portable { .. });

                                let current_mode_label = if is_mrpack {
                                    &mrpack_lbl
                                } else if is_provider {
                                    &provider_lbl
                                } else {
                                    &portable_lbl
                                };

                                ui.label(RichText::new("Export Format:").strong());
                                egui::ComboBox::from_id_salt("export_modpack_format_combo")
                                    .selected_text(current_mode_label)
                                    .width(360.0)
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_label(is_mrpack, &mrpack_lbl).clicked() {
                                            self.export_mode = ModpackExportMode::Mrpack;
                                        }
                                        if ui.selectable_label(is_provider, &provider_lbl).clicked()
                                        {
                                            self.export_mode = ModpackExportMode::ProviderProfile;
                                        }
                                        if ui.selectable_label(is_portable, &portable_lbl).clicked()
                                        {
                                            self.export_mode = ModpackExportMode::Portable {
                                                full: self.export_portable_full,
                                            };
                                        }
                                    });

                                let help_key = if is_mrpack {
                                    "modpack_package.export.mrpack_help"
                                } else if is_provider {
                                    "modpack_package.export.provider_profile_help"
                                } else {
                                    "modpack_package.export.portable_help"
                                };
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(self.t(help_key))
                                        .size(11.0)
                                        .color(p.text_muted),
                                );

                                if is_portable {
                                    ui.add_space(8.0);
                                    let smart_lbl =
                                        self.t("modpack_package.export.smart").to_string();
                                    let full_lbl =
                                        self.t("modpack_package.export.full").to_string();
                                    let cur_portable = if self.export_portable_full {
                                        &full_lbl
                                    } else {
                                        &smart_lbl
                                    };
                                    ui.label(RichText::new("Portable Mode:").strong());
                                    egui::ComboBox::from_id_salt("export_portable_mode_combo")
                                        .selected_text(cur_portable)
                                        .width(280.0)
                                        .show_ui(ui, |ui| {
                                            if ui
                                                .selectable_value(
                                                    &mut self.export_portable_full,
                                                    false,
                                                    &smart_lbl,
                                                )
                                                .changed()
                                            {
                                                self.export_mode =
                                                    ModpackExportMode::Portable { full: false };
                                            }
                                            if ui
                                                .selectable_value(
                                                    &mut self.export_portable_full,
                                                    true,
                                                    &full_lbl,
                                                )
                                                .changed()
                                            {
                                                self.export_mode =
                                                    ModpackExportMode::Portable { full: true };
                                            }
                                        });
                                }

                                ui.add_space(10.0);
                                let saves_enabled = is_mrpack || is_portable;
                                let saves_lbl =
                                    self.t("workspace.export.include_saves").to_string();
                                ui.add_enabled(
                                    saves_enabled,
                                    egui::Checkbox::new(&mut self.export_include_saves, saves_lbl),
                                );

                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(self.t("modpack_package.export.warning"))
                                        .size(11.0)
                                        .color(p.warning),
                                );
                                ui.add_space(12.0);

                                ui.horizontal(|ui| {
                                    let act_btn = Button::new(
                                        RichText::new(self.t("modpack_package.export.action"))
                                            .color(p.accent_text)
                                            .strong(),
                                    )
                                    .fill(p.accent);
                                    if ui.add(act_btn).clicked() {
                                        let mode = match self.export_mode {
                                            ModpackExportMode::Portable { .. } => {
                                                ModpackExportMode::Portable {
                                                    full: self.export_portable_full,
                                                }
                                            }
                                            other => other,
                                        };
                                        trigger_export = Some(ModpackExportOptions {
                                            mode,
                                            include_saves: saves_enabled
                                                && self.export_include_saves,
                                        });
                                    }
                                    if ui.button(self.t("common.cancel")).clicked() {
                                        open = false;
                                    }
                                });
                            });
                        });
                });
                self.show_export_modal = open;
                if let Some(opts) = trigger_export {
                    self.show_export_modal = false;
                    self.start_instance_export(inst.instance_id, opts, ctx);
                }
            }
        }

        // 1c. Mod / Modpack Version Picker Isolated OS Window (with Release/Beta/Alpha, MC Version, Loader & Version Dropdown filters)
        if self.version_picker_open {
            let mut open = self.version_picker_open;
            let mut chosen: Option<ModVersionChoice> = None;
            let mut filter_changed = false;
            let title = format!("Select Version — {}", self.version_picker_title);
            let vp_id = egui::ViewportId::from_hash_of("mcw_vp_version_picker");
            let builder = centered_viewport_builder(ctx, title, [640.0, 540.0], [480.0, 380.0]);

            let beta_lbl = self.t("content.channel.beta").to_string();
            let alpha_lbl = self.t("content.channel.alpha").to_string();
            let open_web_lbl = self.t("content.open_web").to_string();
            let cancel_lbl = self.t("common.cancel").to_string();

            let mut all_mc_versions: Vec<String> = vec!["all".to_string()];
            if let Some(inst) = self.selected_instance() {
                if !all_mc_versions.contains(&inst.version_id) {
                    all_mc_versions.push(inst.version_id.clone());
                }
            }
            for item in &self.version_picker_items {
                for gv in &item.game_versions {
                    if !all_mc_versions.contains(gv) {
                        all_mc_versions.push(gv.clone());
                    }
                }
            }

            let filtered_items: Vec<ModVersionChoice> =
                self.filtered_mod_versions().into_iter().cloned().collect();

            ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                if vp_ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
                egui::CentralPanel::default()
                    .frame(
                        Frame::new()
                            .fill(p.canvas)
                            .inner_margin(egui::Margin::same(14)),
                    )
                    .show(vp_ctx, |ui| {
                        // Header: Thumbnail + Mod Title + Open on Web
                        ui.horizontal(|ui| {
                            self.render_thumbnail(ui, self.version_picker_icon.as_deref(), 44.0);
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(&self.version_picker_title)
                                        .strong()
                                        .size(15.5)
                                        .color(p.text),
                                );
                                ui.label(
                                    RichText::new(if self.version_picker_is_pack {
                                        "Choose a modpack version to download and install."
                                    } else {
                                        "Filter and select a compatible mod version to install into mods/."
                                    })
                                    .size(11.5)
                                    .color(p.text_muted),
                                );
                            });
                            if let Some(ref web_url) = self.version_picker_website_url {
                                let url_clone = web_url.clone();
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button(&open_web_lbl).clicked() {
                                        open_external(&url_clone);
                                    }
                                });
                            }
                        });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(6.0);

                        // Row 1: Release channels (Include Beta / Include Alpha) + auto-sized MC Version & Loader dropdowns
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .checkbox(&mut self.version_picker_include_beta, &beta_lbl)
                                .changed()
                            {
                                filter_changed = true;
                            }
                            if ui
                                .checkbox(&mut self.version_picker_include_alpha, &alpha_lbl)
                                .changed()
                            {
                                filter_changed = true;
                            }
                            ui.separator();
                            ui.label("MC Version:");
                            let mc_w = auto_combo_width(
                                ui,
                                &self.version_picker_mc_filter,
                                68.0,
                                150.0,
                            );
                            egui::ComboBox::from_id_salt("mod_vp_mc_filter_combo")
                                .selected_text(&self.version_picker_mc_filter)
                                .width(mc_w)
                                .truncate()
                                .show_ui(ui, |ui| {
                                    for gv in &all_mc_versions {
                                        if ui
                                            .selectable_value(
                                                &mut self.version_picker_mc_filter,
                                                gv.clone(),
                                                gv,
                                            )
                                            .changed()
                                        {
                                            filter_changed = true;
                                        }
                                    }
                                });
                            ui.label("Loader:");
                            let loader_w = auto_combo_width(
                                ui,
                                &self.version_picker_loader_filter,
                                68.0,
                                130.0,
                            );
                            egui::ComboBox::from_id_salt("mod_vp_loader_filter_combo")
                                .selected_text(&self.version_picker_loader_filter)
                                .width(loader_w)
                                .truncate()
                                .show_ui(ui, |ui| {
                                    for l in ["all", "fabric", "forge", "neoforge", "quilt"] {
                                        if ui
                                            .selectable_value(
                                                &mut self.version_picker_loader_filter,
                                                l.to_string(),
                                                l,
                                            )
                                            .changed()
                                        {
                                            filter_changed = true;
                                        }
                                    }
                                });
                            ui.label(
                                RichText::new(format!("({} available)", filtered_items.len()))
                                    .size(11.0)
                                    .color(p.text_muted),
                            );
                        });

                        ui.add_space(6.0);

                        if self.version_picker_loading {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("Loading versions...").color(p.accent));
                            });
                        } else if let Some(ref err) = self.version_picker_error {
                            ui.label(RichText::new(format!("Error: {err}")).color(p.danger));
                        } else if filtered_items.is_empty() {
                            ui.label(
                                RichText::new(
                                    "No versions match the current filters. Try enabling Beta/Alpha or switching MC Version / Loader to 'all'.",
                                )
                                .color(p.warning),
                            );
                        } else {
                            // Row 2: Responsive Auto-Sized Selected Version Dropdown
                            let selected_item = filtered_items
                                .iter()
                                .find(|v| v.id == self.version_picker_selected_id)
                                .or_else(|| filtered_items.first());
                            let selected_label = selected_item
                                .map(|item| {
                                    let badge = match item.channel {
                                        ModpackChannel::Release => "[R]",
                                        ModpackChannel::Beta => "[B]",
                                        ModpackChannel::Alpha => "[A]",
                                    };
                                    let gvs = item
                                        .game_versions
                                        .iter()
                                        .take(3)
                                        .cloned()
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    let loaders = item.loaders.join(", ");
                                    if loaders.is_empty() {
                                        format!(
                                            "{badge} {} ({}) · MC {} · {}",
                                            item.title,
                                            item.version_number,
                                            gvs,
                                            format_bytes(item.file_size)
                                        )
                                    } else {
                                        format!(
                                            "{badge} {} ({}) · {} · MC {} · {}",
                                            item.title,
                                            item.version_number,
                                            loaders,
                                            gvs,
                                            format_bytes(item.file_size)
                                        )
                                    }
                                })
                                .unwrap_or_else(|| "Select version...".to_string());

                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Selected Version:").strong());
                                let max_combo_w = (ui.available_width() - 4.0).max(160.0);
                                let combo_w =
                                    auto_combo_width(ui, &selected_label, 180.0, max_combo_w);
                                egui::ComboBox::from_id_salt("mod_version_picker_combo")
                                    .selected_text(&selected_label)
                                    .width(combo_w)
                                    .truncate()
                                    .show_ui(ui, |ui| {
                                        for item in &filtered_items {
                                            let badge = match item.channel {
                                                ModpackChannel::Release => "[R]",
                                                ModpackChannel::Beta => "[B]",
                                                ModpackChannel::Alpha => "[A]",
                                            };
                                            let gvs = item
                                                .game_versions
                                                .iter()
                                                .take(3)
                                                .cloned()
                                                .collect::<Vec<_>>()
                                                .join(", ");
                                            let loaders = item.loaders.join(", ");
                                            let text = if loaders.is_empty() {
                                                format!(
                                                    "{badge} {} ({}) · MC {} · {}",
                                                    item.title,
                                                    item.version_number,
                                                    gvs,
                                                    format_bytes(item.file_size)
                                                )
                                            } else {
                                                format!(
                                                    "{badge} {} ({}) · {} · MC {} · {}",
                                                    item.title,
                                                    item.version_number,
                                                    loaders,
                                                    gvs,
                                                    format_bytes(item.file_size)
                                                )
                                            };
                                            ui.selectable_value(
                                                &mut self.version_picker_selected_id,
                                                item.id.clone(),
                                                text,
                                            );
                                        }
                                    });
                            });

                            ui.add_space(6.0);

                            // Footer Buttons + Scrollable list of versions
                            egui::TopBottomPanel::bottom("mod_vp_footer_panel")
                                .frame(
                                    Frame::new()
                                        .fill(p.canvas)
                                        .inner_margin(egui::Margin::symmetric(0, 6)),
                                )
                                .show_inside(ui, |ui| {
                                    ui.separator();
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        let install_btn = Button::new(
                                            RichText::new("Install Selected Version")
                                                .color(p.accent_text)
                                                .strong(),
                                        )
                                        .fill(p.accent);
                                        if ui
                                            .add_enabled(selected_item.is_some(), install_btn)
                                            .clicked()
                                        {
                                            if let Some(sel) = selected_item {
                                                chosen = Some(sel.clone());
                                            }
                                        }
                                        if ui.button(&cancel_lbl).clicked() {
                                            open = false;
                                        }
                                    });
                                });

                            ScrollArea::vertical()
                                .id_salt("version_picker_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let inner_card_w = (ui.available_width() - 24.0).max(200.0);
                                    for item in &filtered_items {
                                        let is_selected =
                                            item.id == self.version_picker_selected_id;
                                        let badge = match item.channel {
                                            ModpackChannel::Release => "[R]",
                                            ModpackChannel::Beta => "[B]",
                                            ModpackChannel::Alpha => "[A]",
                                        };
                                        let badge_col = match item.channel {
                                            ModpackChannel::Release => p.accent,
                                            ModpackChannel::Beta => p.warning,
                                            ModpackChannel::Alpha => p.danger,
                                        };
                                        Frame::new()
                                            .fill(p.elevated)
                                            .stroke(Stroke::new(
                                                1.0_f32,
                                                if is_selected { p.accent } else { p.border },
                                            ))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::symmetric(8, 6))
                                            .show(ui, |ui| {
                                                ui.set_width(inner_card_w);
                                                ui.horizontal(|ui| {
                                                    let text_max_w =
                                                        (inner_card_w - 92.0).max(120.0);
                                                    ui.vertical(|ui| {
                                                        ui.set_max_width(text_max_w);
                                                        ui.horizontal_wrapped(|ui| {
                                                            ui.label(
                                                                RichText::new(badge)
                                                                    .monospace()
                                                                    .strong()
                                                                    .size(11.0)
                                                                    .color(badge_col),
                                                            );
                                                            ui.label(
                                                                RichText::new(&item.title).strong(),
                                                            );
                                                        });
                                                        let gvs = item
                                                            .game_versions
                                                            .iter()
                                                            .take(4)
                                                            .cloned()
                                                            .collect::<Vec<_>>()
                                                            .join(", ");
                                                        let loaders = item.loaders.join(", ");
                                                        let meta = if loaders.is_empty() {
                                                            format!(
                                                                "{} · MC {} · {}",
                                                                item.version_number,
                                                                gvs,
                                                                format_bytes(item.file_size)
                                                            )
                                                        } else {
                                                            format!(
                                                                "{} · {} · MC {} · {}",
                                                                item.version_number,
                                                                loaders,
                                                                gvs,
                                                                format_bytes(item.file_size)
                                                            )
                                                        };
                                                        ui.label(
                                                            RichText::new(meta)
                                                                .monospace()
                                                                .size(10.5)
                                                                .color(p.text_muted),
                                                        );
                                                    });
                                                    ui.with_layout(
                                                        Layout::right_to_left(Align::Center),
                                                        |ui| {
                                                            let btn = Button::new(
                                                                RichText::new("Install")
                                                                    .color(p.accent_text)
                                                                    .strong(),
                                                            )
                                                            .fill(p.accent);
                                                            if ui.add(btn).clicked() {
                                                                chosen = Some(item.clone());
                                                            }
                                                        },
                                                    );
                                                });
                                            });
                                        ui.add_space(4.0);
                                    }
                                });
                        }
                    });
            });
            self.version_picker_open = open;
            if filter_changed {
                self.sync_mod_version_picker_selection();
            }
            if let Some(choice) = chosen {
                self.version_picker_open = false;
                if self.version_picker_is_pack {
                    let slug = self
                        .version_picker_key
                        .strip_prefix("mr:")
                        .unwrap_or(&self.version_picker_key)
                        .to_string();
                    let title = self.version_picker_title.clone();
                    self.start_modrinth_pack_install(slug, title, Some(choice.id), ctx);
                } else {
                    self.start_direct_mod_file_install(choice, ctx);
                }
            }
        }

        // 2. Rename Instance Isolated OS Window (Centered)
        if self.show_rename_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                let mut open = self.show_rename_modal;
                let vp_id = egui::ViewportId::from_hash_of("mcw_vp_rename_instance");
                let builder = centered_viewport_builder(
                    ctx,
                    format!("Rename Instance — {}", inst.name),
                    [420.0, 170.0],
                    [360.0, 150.0],
                );
                ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                    if vp_ctx.input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    egui::CentralPanel::default()
                        .frame(
                            Frame::new()
                                .fill(p.canvas)
                                .inner_margin(egui::Margin::same(14)),
                        )
                        .show(vp_ctx, |ui| {
                            ui.label(format!("New name for '{}':", inst.name));
                            ui.text_edit_singleline(&mut self.rename_input);
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                let ren_btn = Button::new(
                                    RichText::new("Rename").color(p.accent_text).strong(),
                                )
                                .fill(p.accent);
                                if ui.add(ren_btn).clicked() {
                                    let store = InstanceStore::new(&self.paths);
                                    match store
                                        .rename_instance(&inst.instance_id, &self.rename_input)
                                    {
                                        Ok(updated) => {
                                            self.selected_instance_id = Some(updated.instance_id);
                                            open = false;
                                            self.reload_instances();
                                            self.notice_banner = Some((
                                                format!("Renamed instance to '{}'.", updated.name),
                                                false,
                                            ));
                                        }
                                        Err(e) => {
                                            self.notice_banner =
                                                Some((format!("Rename failed: {e}"), true));
                                        }
                                    }
                                }
                                if ui.button("Cancel").clicked() {
                                    open = false;
                                }
                            });
                        });
                });
                self.show_rename_modal = open;
            }
        }

        // 2a. Edit Group & Tags Isolated OS Window (Centered, opened from Actions -> Edit Group & Tags...)
        if self.show_group_tags_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                let mut open = self.show_group_tags_modal;
                let vp_id = egui::ViewportId::from_hash_of("mcw_vp_edit_group_tags");
                let builder = centered_viewport_builder(
                    ctx,
                    format!("Edit Group & Tags — {}", inst.name),
                    [460.0, 230.0],
                    [380.0, 200.0],
                );
                ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                    if vp_ctx.input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    egui::CentralPanel::default()
                        .frame(
                            Frame::new()
                                .fill(p.canvas)
                                .inner_margin(egui::Margin::same(14)),
                        )
                        .show(vp_ctx, |ui| {
                            ui.label(
                                RichText::new(format!("Group & Tags for '{}'", inst.name))
                                    .strong()
                                    .size(14.0),
                            );
                            ui.add_space(8.0);
                            ui.label(RichText::new("Group:").color(p.text_muted));
                            ui.add(
                                TextEdit::singleline(&mut self.edit_group_input)
                                    .hint_text("e.g. Survival, Modded")
                                    .desired_width(ui.available_width()),
                            );
                            ui.add_space(6.0);
                            ui.label(RichText::new("Tags (comma-separated):").color(p.text_muted));
                            ui.add(
                                TextEdit::singleline(&mut self.edit_tags_input)
                                    .hint_text("e.g. RPG, Tech, 1.21")
                                    .desired_width(ui.available_width()),
                            );
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                let save_btn = Button::new(
                                    RichText::new("Save Group & Tags")
                                        .color(p.accent_text)
                                        .strong(),
                                )
                                .fill(p.accent);
                                if ui.add(save_btn).clicked() {
                                    let store = InstanceStore::new(&self.paths);
                                    match store.set_library_metadata(
                                        &inst.instance_id,
                                        None,
                                        Some(&self.edit_group_input),
                                        Some(&self.edit_tags_input),
                                    ) {
                                        Ok(_) => {
                                            open = false;
                                            self.reload_instances();
                                            self.notice_banner = Some((
                                                "Updated instance group and tags.".to_string(),
                                                false,
                                            ));
                                        }
                                        Err(e) => {
                                            self.notice_banner =
                                                Some((format!("Failed to update tags: {e}"), true));
                                        }
                                    }
                                }
                                if ui.button("Cancel").clicked() {
                                    open = false;
                                }
                            });
                        });
                });
                self.show_group_tags_modal = open;
            }
        }

        // 2b. Clone / Duplicate Instance Isolated OS Window (Centered)
        if self.show_clone_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                let mut open = self.show_clone_modal;
                let clone_title = self.t("workspace.clone.title").to_string();
                let include_saves_lbl = self.t("workspace.clone.include_saves").to_string();
                let clone_act_lbl = self.t("workspace.clone.action").to_string();
                let cancel_lbl = self.t("common.cancel").to_string();
                let vp_id = egui::ViewportId::from_hash_of("mcw_vp_clone_instance");
                let builder = centered_viewport_builder(
                    ctx,
                    format!("{clone_title} — {}", inst.name),
                    [440.0, 210.0],
                    [360.0, 180.0],
                );
                ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                    if vp_ctx.input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    egui::CentralPanel::default()
                        .frame(
                            Frame::new()
                                .fill(p.canvas)
                                .inner_margin(egui::Margin::same(14)),
                        )
                        .show(vp_ctx, |ui| {
                            ui.label(format!("New cloned instance name (from '{}'):", inst.name));
                            ui.text_edit_singleline(&mut self.clone_name_input);
                            ui.add_space(6.0);
                            ui.checkbox(&mut self.clone_include_saves, &include_saves_lbl);
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                let act_btn = Button::new(
                                    RichText::new(&clone_act_lbl).color(p.accent_text).strong(),
                                )
                                .fill(p.accent);
                                if ui.add(act_btn).clicked() {
                                    let store = InstanceStore::new(&self.paths);
                                    match store.clone_instance(
                                        &inst.instance_id,
                                        &self.clone_name_input,
                                        self.clone_include_saves,
                                    ) {
                                        Ok(cloned) => {
                                            self.selected_instance_id = Some(cloned.instance_id);
                                            open = false;
                                            self.reload_instances();
                                            self.notice_banner = Some((
                                                format!("Cloned instance to '{}'.", cloned.name),
                                                false,
                                            ));
                                        }
                                        Err(e) => {
                                            self.notice_banner =
                                                Some((format!("Clone failed: {e}"), true));
                                        }
                                    }
                                }
                                if ui.button(&cancel_lbl).clicked() {
                                    open = false;
                                }
                            });
                        });
                });
                self.show_clone_modal = open;
            }
        }

        // 3. Delete Instance Confirmation Isolated OS Window (Centered, Data Safety Gate)
        if self.show_delete_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                let mut open = self.show_delete_modal;
                let vp_id = egui::ViewportId::from_hash_of("mcw_vp_delete_instance");
                let builder = centered_viewport_builder(
                    ctx,
                    format!("Delete Instance — {}", inst.name),
                    [460.0, 190.0],
                    [380.0, 160.0],
                );
                ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                    if vp_ctx.input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    egui::CentralPanel::default()
                        .frame(
                            Frame::new()
                                .fill(p.canvas)
                                .inner_margin(egui::Margin::same(14)),
                        )
                        .show(vp_ctx, |ui| {
                            ui.label(
                                RichText::new(format!("Permanently delete '{}'?", inst.name))
                                    .strong()
                                    .color(p.danger),
                            );
                            ui.label(
                                "This will permanently delete the instance folder including all saves, mods, and configs inside .minecraft/.",
                            );
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                let confirm_btn = Button::new(
                                    RichText::new("Delete Permanently")
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(p.danger);
                                if ui.add(confirm_btn).clicked() {
                                    let store = InstanceStore::new(&self.paths);
                                    match store.delete_instance(&inst.instance_id) {
                                        Ok(()) => {
                                            open = false;
                                            self.selected_instance_id = None;
                                            self.reload_instances();
                                            self.notice_banner = Some((
                                                format!("Deleted instance '{}'.", inst.name),
                                                false,
                                            ));
                                        }
                                        Err(e) => {
                                            open = false;
                                            self.notice_banner =
                                                Some((format!("Delete failed: {e}"), true));
                                        }
                                    }
                                }
                                if ui.button("Cancel").clicked() {
                                    open = false;
                                }
                            });
                        });
                });
                self.show_delete_modal = open;
            }
        }

        // 4. Account Manager Isolated OS Window (Centered)
        if self.show_account_modal {
            let mut open = self.show_account_modal;
            let vp_id = egui::ViewportId::from_hash_of("mcw_vp_accounts");
            let builder = centered_viewport_builder(
                ctx,
                "Accounts — MCW Launcher",
                [480.0, 440.0],
                [380.0, 340.0],
            );
            ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                if vp_ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
                egui::CentralPanel::default()
                    .frame(Frame::new().fill(p.canvas).inner_margin(egui::Margin::same(14)))
                    .show(vp_ctx, |ui| {
                    ScrollArea::vertical().show(ui, |ui| {
                    ui.label(RichText::new("Saved Accounts").strong());
                    ui.add_space(4.0);

                    if self.accounts.is_empty() {
                        ui.label(RichText::new("No accounts added yet.").color(p.text_muted));
                    } else {
                        let mut set_active: Option<StoredAccount> = None;
                        let mut remove_uuid: Option<String> = None;
                        for acc in &self.accounts {
                            ui.horizontal(|ui| {
                                let badge = if acc.account_type == AccountSource::Microsoft {
                                    "MSA"
                                } else {
                                    "Offline"
                                };
                                let prefix = if acc.active { "● " } else { "○ " };
                                if ui
                                    .selectable_label(
                                        acc.active,
                                        format!("{prefix}{} ({badge})", acc.username),
                                    )
                                    .clicked()
                                {
                                    set_active = Some(acc.clone());
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .button(RichText::new("Remove").color(p.danger))
                                        .clicked()
                                    {
                                        remove_uuid = Some(acc.uuid.clone());
                                    }
                                });
                            });
                        }
                        if let Some(acc) = set_active {
                            let _ = AccountStore::new(&self.paths).upsert(acc, true);
                            self.reload_accounts();
                        }
                        if let Some(uuid) = remove_uuid {
                            let _ = AccountStore::new(&self.paths).remove(&uuid);
                            self.reload_accounts();
                        }
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label(RichText::new("Microsoft Account").strong());
                    ui.label(
                        RichText::new(
                            "Browser (PKCE) always asks which Microsoft account to use.",
                        )
                        .size(11.5)
                        .color(p.text_muted),
                    );
                    ui.horizontal(|ui| {
                        let pkce_btn = Button::new(
                            RichText::new("Sign in with Browser (PKCE)")
                                .color(p.accent_text)
                                .strong(),
                        )
                        .fill(p.accent);
                        if ui
                            .add_enabled(!self.ms_login_in_progress, pkce_btn)
                            .clicked()
                        {
                            self.start_microsoft_login(true, ctx);
                        }
                        if self.ms_login_in_progress && self.ms_login_mode == Some("pkce") {
                            let cancel_btn = Button::new(RichText::new("Cancel").color(p.danger))
                                .stroke(Stroke::new(1.0_f32, p.danger));
                            if ui.add(cancel_btn).clicked() {
                                self.cancel_microsoft_login();
                            }
                        }

                        if ui
                            .add_enabled(
                                !self.ms_login_in_progress,
                                Button::new("Sign in with Device Code"),
                            )
                            .clicked()
                        {
                            self.start_microsoft_login(false, ctx);
                        }
                        if self.ms_login_in_progress && self.ms_login_mode == Some("device_code") {
                            let cancel_btn = Button::new(RichText::new("Cancel").color(p.danger))
                                .stroke(Stroke::new(1.0_f32, p.danger));
                            if ui.add(cancel_btn).clicked() {
                                self.cancel_microsoft_login();
                            }
                        }
                    });

                    if self.ms_login_in_progress && self.ms_login_mode == Some("pkce") {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Waiting for Microsoft sign-in in your browser (listening on localhost:8400)...",
                            )
                            .size(11.5)
                            .color(p.accent),
                        );
                    }

                    if let Some(ref dc) = self.ms_device_code {
                        ui.add_space(6.0);
                        Frame::new()
                            .fill(p.elevated)
                            .stroke(Stroke::new(1.0_f32, p.accent))
                            .corner_radius(CornerRadius::same(3))
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.label("Enter this code in your browser:");
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&dc.user_code)
                                            .monospace()
                                            .size(18.0)
                                            .strong()
                                            .color(p.accent),
                                    );
                                    if ui.button("Copy Code").clicked() {
                                        ui.ctx().copy_text(dc.user_code.clone());
                                    }
                                    if ui.button("Copy URL (for InPrivate)").clicked() {
                                        ui.ctx().copy_text(dc.verification_uri.clone());
                                    }
                                });
                                ui.label(
                                    RichText::new(&dc.verification_uri)
                                        .monospace()
                                        .size(11.0)
                                        .color(p.text_muted),
                                );
                                ui.label(
                                    RichText::new(
                                        "Tip: Microsoft Device Login reuses your active browser session. To pick a different account, paste the URL into an InPrivate/Incognito window or use Browser (PKCE).",
                                    )
                                    .size(11.0)
                                    .color(p.text_muted),
                                );
                            });
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label(RichText::new("Add Offline Account").strong());
                    ui.horizontal(|ui| {
                        ui.add(
                            TextEdit::singleline(&mut self.offline_username_input)
                                .hint_text("Username")
                                .desired_width(220.0),
                        );
                        if ui.button("Add Offline").clicked() {
                            match OfflineAuth::authenticate(&self.offline_username_input) {
                                Ok(auth) => {
                                    let stored = StoredAccount {
                                        uuid: auth.uuid,
                                        username: auth.player_name,
                                        account_type: AccountSource::Offline,
                                        skin_url: None,
                                        active: true,
                                    };
                                    let _ = AccountStore::new(&self.paths).upsert(stored, true);
                                    self.offline_username_input.clear();
                                    self.reload_accounts();
                                }
                                Err(e) => {
                                    self.notice_banner = Some((e.to_string(), true));
                                }
                            }
                        }
                    });

                    ui.add_space(10.0);
                    if ui.button("Close").clicked() {
                        open = false;
                    }
                    });
                    });
            });
            if !open {
                if self.ms_login_in_progress {
                    self.cancel_microsoft_login();
                }
                self.show_account_modal = false;
            }
        }

        // 5. Restore Backup Confirmation Isolated OS Window (Centered)
        if let Some(backup_id) = self.pending_restore_backup_id.clone() {
            let mut open = true;
            let mut confirm_restore = false;
            let vp_id = egui::ViewportId::from_hash_of("mcw_vp_restore_backup");
            let builder = centered_viewport_builder(
                ctx,
                format!("Restore Backup — {backup_id}"),
                [460.0, 190.0],
                [380.0, 160.0],
            );
            ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                if vp_ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
                egui::CentralPanel::default()
                    .frame(
                        Frame::new()
                            .fill(p.canvas)
                            .inner_margin(egui::Margin::same(14)),
                    )
                    .show(vp_ctx, |ui| {
                        ui.label(
                            RichText::new(format!("Restore backup '{backup_id}'?"))
                                .strong()
                                .color(p.warning),
                        );
                        ui.label(
                            "Current .minecraft/ contents will be replaced using a safe staging & rollback swap (original data is preserved on rollback if restore fails).",
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            let btn = Button::new(
                                RichText::new("Confirm Restore")
                                    .color(p.accent_text)
                                    .strong(),
                            )
                            .fill(p.accent);
                            if ui.add(btn).clicked() {
                                confirm_restore = true;
                                open = false;
                            }
                            if ui.button("Cancel").clicked() {
                                open = false;
                            }
                        });
                    });
            });
            if !open {
                self.pending_restore_backup_id = None;
            }
            if confirm_restore {
                self.start_backup_restore(backup_id, ctx);
            }
        }

        // 5b. Delete World Confirmation Isolated OS Window (Centered, Data Safety Gate)
        if let Some((inst_id, folder_name)) = self.pending_delete_world.clone() {
            let mut open = true;
            let mut confirm_delete = false;
            let vp_id = egui::ViewportId::from_hash_of("mcw_vp_delete_world");
            let builder = centered_viewport_builder(
                ctx,
                format!("Delete World — {folder_name}"),
                [440.0, 180.0],
                [360.0, 150.0],
            );
            ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                if vp_ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
                egui::CentralPanel::default()
                    .frame(
                        Frame::new()
                            .fill(p.canvas)
                            .inner_margin(egui::Margin::same(14)),
                    )
                    .show(vp_ctx, |ui| {
                        ui.label(
                            RichText::new(format!("Permanently delete world '{folder_name}'?"))
                                .strong()
                                .color(p.danger),
                        );
                        ui.label("This will permanently remove the world save folder from saves/.");
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            let confirm_btn = Button::new(
                                RichText::new("Delete World").color(Color32::WHITE).strong(),
                            )
                            .fill(p.danger);
                            if ui.add(confirm_btn).clicked() {
                                confirm_delete = true;
                                open = false;
                            }
                            if ui.button("Cancel").clicked() {
                                open = false;
                            }
                        });
                    });
            });
            if !open {
                self.pending_delete_world = None;
            }
            if confirm_delete {
                let mc_dir = self.paths.minecraft_dir(&inst_id);
                match WorldManager::delete_world(&mc_dir, &folder_name) {
                    Ok(()) => {
                        self.worlds_cache.remove(&inst_id);
                        self.load_worlds_async(&inst_id, ctx, true);
                        self.notice_banner =
                            Some((format!("Deleted world '{folder_name}'."), false));
                    }
                    Err(e) => {
                        self.notice_banner = Some((format!("Delete world failed: {e}"), true));
                    }
                }
            }
        }

        // 6. Launcher Settings Isolated OS Window (responsive: SidePanel when wide, TopBar when narrow)
        if self.show_launcher_settings {
            let mut open = self.show_launcher_settings;
            let mut prefs_dirty = false;
            let mut reset_defaults = false;
            let mut reload_langs = false;
            let mut scan_java = false;
            let mut install_java_major: Option<u32> = None;
            let win_title = self.t("launcher_settings.page.title").to_string();
            let nav_tabs = [
                (
                    LauncherSettingsTab::General,
                    self.t("settings.section.general").to_string(),
                ),
                (
                    LauncherSettingsTab::Downloads,
                    self.t("settings.section.downloads").to_string(),
                ),
                (
                    LauncherSettingsTab::Runtime,
                    self.t("settings.section.runtime").to_string(),
                ),
                (
                    LauncherSettingsTab::Appearance,
                    self.t("settings.section.appearance").to_string(),
                ),
            ];
            let vp_id = egui::ViewportId::from_hash_of("mcw_vp_launcher_settings");
            let builder = centered_viewport_builder(
                ctx,
                format!("{win_title} — MCW Launcher"),
                [740.0, 520.0],
                [400.0, 340.0],
            );

            ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                if vp_ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
                let vp_width = vp_ctx.screen_rect().width();
                let is_wide = vp_width >= 580.0;

                if is_wide {
                    egui::SidePanel::left("launcher_settings_nav_rail")
                        .exact_width(172.0)
                        .resizable(false)
                        .frame(
                            Frame::new()
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .inner_margin(egui::Margin::same(10)),
                        )
                        .show(vp_ctx, |ui| {
                            ui.vertical(|ui| {
                                for (tab, label) in &nav_tabs {
                                    let active = self.launcher_settings_tab == *tab;
                                    let btn = Button::new(
                                        RichText::new(label).strong().size(13.0).color(if active {
                                            p.accent_text
                                        } else {
                                            p.text
                                        }),
                                    )
                                    .fill(if active { p.accent } else { p.elevated })
                                    .min_size(Vec2::new(ui.available_width(), 34.0))
                                    .corner_radius(CornerRadius::same(3));
                                    if ui.add(btn).clicked() {
                                        self.launcher_settings_tab = *tab;
                                    }
                                    ui.add_space(6.0);
                                }
                            });
                        });
                } else {
                    egui::TopBottomPanel::top("launcher_settings_nav_top")
                        .frame(
                            Frame::new()
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .inner_margin(egui::Margin::symmetric(10, 8)),
                        )
                        .show(vp_ctx, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for (tab, label) in &nav_tabs {
                                    let active = self.launcher_settings_tab == *tab;
                                    let btn = Button::new(
                                        RichText::new(label).strong().size(12.5).color(if active {
                                            p.accent_text
                                        } else {
                                            p.text
                                        }),
                                    )
                                    .fill(if active { p.accent } else { p.elevated })
                                    .corner_radius(CornerRadius::same(3));
                                    if ui.add(btn).clicked() {
                                        self.launcher_settings_tab = *tab;
                                    }
                                }
                            });
                        });
                }

                egui::CentralPanel::default()
                    .frame(
                        Frame::new()
                            .fill(p.canvas)
                            .inner_margin(egui::Margin::same(14)),
                    )
                    .show(vp_ctx, |ui| {
                        ScrollArea::vertical()
                            .id_salt("launcher_settings_content_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                match self.launcher_settings_tab {
                                    LauncherSettingsTab::General => {
                                        ui.label(
                                            RichText::new(self.t("settings.section.general"))
                                                .strong()
                                                .size(15.0)
                                                .color(p.text),
                                        );
                                        ui.label(
                                            RichText::new(
                                                self.t("settings.section.general_detail"),
                                            )
                                            .size(11.5)
                                            .color(p.text_muted),
                                        );
                                        ui.add_space(10.0);

                                        // Card 1: Startup and behavior
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(self.t("Startup and behavior"))
                                                        .strong(),
                                                );
                                                ui.add_space(6.0);
                                                let snap_lbl = self
                                                    .t("Show non-release versions by default")
                                                    .to_string();
                                                if ui
                                                    .checkbox(
                                                        &mut self.prefs.show_snapshots,
                                                        snap_lbl,
                                                    )
                                                    .changed()
                                                {
                                                    self.create_show_snapshots =
                                                        self.prefs.show_snapshots;
                                                    prefs_dirty = true;
                                                }
                                                let dbg_lbl = self
                                                    .t("Enable debug launch information")
                                                    .to_string();
                                                if ui
                                                    .checkbox(&mut self.prefs.debug_mode, dbg_lbl)
                                                    .changed()
                                                {
                                                    prefs_dirty = true;
                                                }
                                                ui.add_space(4.0);
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label(
                                                        "Notification auto-dismiss (seconds, 0 = disabled):",
                                                    );
                                                    if ui
                                                        .add(
                                                            egui::DragValue::new(
                                                                &mut self
                                                                    .prefs
                                                                    .notification_timeout_secs,
                                                            )
                                                            .range(0..=60)
                                                            .suffix("s"),
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                });
                                            });

                                        ui.add_space(8.0);

                                        // Card 2: Language (official Python LanguageManager — Dropdown)
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(
                                                        self.t("launcher_settings.language.title"),
                                                    )
                                                    .strong(),
                                                );
                                                ui.label(
                                                    RichText::new(
                                                        self.t("launcher_settings.language.detail"),
                                                    )
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                                ui.add_space(8.0);
                                                let available: Vec<(String, String)> = self
                                                    .lang
                                                    .available_languages()
                                                    .into_iter()
                                                    .map(|(loc, name)| {
                                                        (loc.to_string(), name.to_string())
                                                    })
                                                    .collect();
                                                let current_loc =
                                                    self.lang.current_locale().to_string();
                                                let current_lang_label = available
                                                    .iter()
                                                    .find(|(loc, _)| loc == &current_loc)
                                                    .map(|(loc, name)| format!("{name} ({loc})"))
                                                    .unwrap_or_else(|| current_loc.clone());
                                                let mut picked_locale: Option<String> = None;
                                                ui.horizontal_wrapped(|ui| {
                                                    egui::ComboBox::from_id_salt(
                                                        "launcher_settings_lang_combo",
                                                    )
                                                    .selected_text(current_lang_label)
                                                    .width(240.0)
                                                    .show_ui(ui, |ui| {
                                                        for (loc, name) in &available {
                                                            let selected = loc == &current_loc;
                                                            let label = format!("{name} ({loc})");
                                                            if ui
                                                                .selectable_label(selected, label)
                                                                .clicked()
                                                                && !selected
                                                            {
                                                                picked_locale = Some(loc.clone());
                                                            }
                                                        }
                                                    });
                                                    let reload_lbl = self
                                                        .t("launcher_settings.language.reload")
                                                        .to_string();
                                                    if ui.button(reload_lbl).clicked() {
                                                        reload_langs = true;
                                                    }
                                                });
                                                if let Some(loc) = picked_locale {
                                                    self.lang.set_language(&loc);
                                                    prefs_dirty = true;
                                                }
                                            });

                                        ui.add_space(8.0);

                                        // Card 3: Reset to defaults
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(self.t(
                                                        "launcher_settings.reset_defaults.title",
                                                    ))
                                                    .strong(),
                                                );
                                                ui.label(
                                                    RichText::new(self.t(
                                                        "launcher_settings.reset_defaults.detail",
                                                    ))
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                                ui.add_space(6.0);
                                                let reset_lbl =
                                                    self.t("launcher_settings.reset").to_string();
                                                let btn = Button::new(
                                                    RichText::new(reset_lbl).color(p.danger),
                                                )
                                                .stroke(Stroke::new(1.0_f32, p.danger));
                                                if ui.add(btn).clicked() {
                                                    reset_defaults = true;
                                                }
                                            });
                                    }
                                    LauncherSettingsTab::Downloads => {
                                        ui.label(
                                            RichText::new(self.t("settings.section.downloads"))
                                                .strong()
                                                .size(15.0)
                                                .color(p.text),
                                        );
                                        ui.label(
                                            RichText::new(
                                                self.t("settings.section.downloads_detail"),
                                            )
                                            .size(11.5)
                                            .color(p.text_muted),
                                        );
                                        ui.add_space(10.0);

                                        // Card 1: Download concurrency
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(self.t("Download bandwidth"))
                                                        .strong(),
                                                );
                                                ui.add_space(6.0);
                                                let conc_label =
                                                    self.t("network.concurrency.label").to_string();
                                                let auto_label =
                                                    self.t("network.concurrency.auto").to_string();
                                                let cur_text =
                                                    if self.prefs.download_concurrency == 0 {
                                                        auto_label.clone()
                                                    } else {
                                                        format!(
                                                            "{} concurrent",
                                                            self.prefs.download_concurrency
                                                        )
                                                    };
                                                egui::ComboBox::from_label(conc_label)
                                                    .selected_text(cur_text)
                                                    .show_ui(ui, |ui| {
                                                        if ui
                                                            .selectable_value(
                                                                &mut self
                                                                    .prefs
                                                                    .download_concurrency,
                                                                0,
                                                                auto_label,
                                                            )
                                                            .changed()
                                                        {
                                                            prefs_dirty = true;
                                                        }
                                                        for v in [2u32, 4, 6, 8, 12, 16] {
                                                            if ui
                                                                .selectable_value(
                                                                    &mut self
                                                                        .prefs
                                                                        .download_concurrency,
                                                                    v,
                                                                    format!("{v} concurrent"),
                                                                )
                                                                .changed()
                                                            {
                                                                prefs_dirty = true;
                                                            }
                                                        }
                                                    });
                                            });

                                        ui.add_space(8.0);

                                        // Card 2: Content browser
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(self.t("content.settings.title"))
                                                        .strong(),
                                                );
                                                ui.label(
                                                    RichText::new(
                                                        self.t("content.settings.detail"),
                                                    )
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                                ui.add_space(6.0);
                                                let desc_lbl = self
                                                    .t("content.settings.show_descriptions")
                                                    .to_string();
                                                if ui
                                                    .checkbox(
                                                        &mut self.prefs.show_content_descriptions,
                                                        desc_lbl,
                                                    )
                                                    .changed()
                                                {
                                                    prefs_dirty = true;
                                                }
                                                let beta_lbl = self
                                                    .t("Include Beta mod and modpack versions")
                                                    .to_string();
                                                if ui
                                                    .checkbox(
                                                        &mut self.prefs.modrinth_include_beta,
                                                        beta_lbl,
                                                    )
                                                    .changed()
                                                {
                                                    prefs_dirty = true;
                                                }
                                                let alpha_lbl = self
                                                    .t("Include Alpha mod and modpack versions")
                                                    .to_string();
                                                if ui
                                                    .checkbox(
                                                        &mut self.prefs.modrinth_include_alpha,
                                                        alpha_lbl,
                                                    )
                                                    .changed()
                                                {
                                                    prefs_dirty = true;
                                                }
                                            });
                                    }
                                    LauncherSettingsTab::Runtime => {
                                        ui.label(
                                            RichText::new(self.t("settings.section.runtime"))
                                                .strong()
                                                .size(15.0)
                                                .color(p.text),
                                        );
                                        ui.label(
                                            RichText::new(
                                                self.t("settings.section.runtime_detail"),
                                            )
                                            .size(11.5)
                                            .color(p.text_muted),
                                        );
                                        ui.add_space(10.0);

                                        // Card 1: Java runtimes
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(
                                                        self.t("launcher_settings.java.title"),
                                                    )
                                                    .strong(),
                                                );
                                                ui.label(
                                                    RichText::new(
                                                        self.t(
                                                            "launcher_settings.java.description",
                                                        ),
                                                    )
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                                ui.add_space(6.0);
                                                if self.java_installations.is_empty() {
                                                    ui.label(
                                                        RichText::new(
                                                            "No Java runtimes detected yet.",
                                                        )
                                                        .color(p.warning),
                                                    );
                                                } else {
                                                    for j in &self.java_installations {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "• Java {} ({}) — {}",
                                                                j.major,
                                                                j.version_string,
                                                                j.path.display()
                                                            ))
                                                            .monospace()
                                                            .size(11.0)
                                                            .color(p.text_muted),
                                                        );
                                                    }
                                                }
                                                ui.add_space(6.0);
                                                ui.horizontal_wrapped(|ui| {
                                                    let scan_lbl = self
                                                        .t("Scan Java installations")
                                                        .to_string();
                                                    if ui.button(scan_lbl).clicked() {
                                                        scan_java = true;
                                                    }
                                                    for major in [8u32, 17, 21] {
                                                        let busy = self
                                                            .installing_java_majors
                                                            .contains(&major);
                                                        let lbl = if busy {
                                                            format!("Installing Java {major}...")
                                                        } else {
                                                            format!("Install Temurin Java {major}")
                                                        };
                                                        if ui
                                                            .add_enabled(!busy, Button::new(lbl))
                                                            .clicked()
                                                        {
                                                            install_java_major = Some(major);
                                                        }
                                                    }
                                                });
                                            });

                                        ui.add_space(8.0);

                                        // Card 2: Default settings for new instances
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(
                                                        self.t("instance_defaults.launcher.title"),
                                                    )
                                                    .strong(),
                                                );
                                                ui.label(
                                                    RichText::new(self.t(
                                                        "instance_defaults.launcher.description",
                                                    ))
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                                ui.add_space(6.0);
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label("Min RAM (MB):");
                                                    if ui
                                                        .add(
                                                            egui::DragValue::new(
                                                                &mut self.prefs.default_min_memory,
                                                            )
                                                            .range(256..=32768),
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                    ui.label("Max RAM (MB):");
                                                    if ui
                                                        .add(
                                                            egui::DragValue::new(
                                                                &mut self.prefs.default_max_memory,
                                                            )
                                                            .range(512..=65536),
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                });
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label("Width:");
                                                    if ui
                                                        .add(
                                                            egui::DragValue::new(
                                                                &mut self.prefs.default_width,
                                                            )
                                                            .range(320..=7680),
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                    ui.label("Height:");
                                                    if ui
                                                        .add(
                                                            egui::DragValue::new(
                                                                &mut self.prefs.default_height,
                                                            )
                                                            .range(240..=4320),
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                    if ui
                                                        .checkbox(
                                                            &mut self.prefs.default_fullscreen,
                                                            "Fullscreen",
                                                        )
                                                        .changed()
                                                    {
                                                        prefs_dirty = true;
                                                    }
                                                });
                                            });
                                    }
                                    LauncherSettingsTab::Appearance => {
                                        ui.label(
                                            RichText::new(self.t("settings.section.appearance"))
                                                .strong()
                                                .size(15.0)
                                                .color(p.text),
                                        );
                                        ui.label(
                                            RichText::new(
                                                self.t("settings.section.appearance_detail"),
                                            )
                                            .size(11.5)
                                            .color(p.text_muted),
                                        );
                                        ui.add_space(10.0);

                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(self.t("Launcher theme"))
                                                        .strong(),
                                                );
                                                ui.add_space(8.0);
                                                let cur_theme_lbl = match self.theme_mode {
                                                    ThemeMode::Dark => "Dark (Default)",
                                                    ThemeMode::Light => "Light",
                                                };
                                                egui::ComboBox::from_id_salt(
                                                    "launcher_settings_theme_combo",
                                                )
                                                .selected_text(cur_theme_lbl)
                                                .width(220.0)
                                                .show_ui(ui, |ui| {
                                                    if ui
                                                        .selectable_value(
                                                            &mut self.theme_mode,
                                                            ThemeMode::Dark,
                                                            "Dark (Default)",
                                                        )
                                                        .changed()
                                                    {
                                                        self.prefs.theme = "dark".to_string();
                                                        self.palette =
                                                            apply_theme(ctx, self.theme_mode);
                                                        prefs_dirty = true;
                                                    }
                                                    if ui
                                                        .selectable_value(
                                                            &mut self.theme_mode,
                                                            ThemeMode::Light,
                                                            "Light",
                                                        )
                                                        .changed()
                                                    {
                                                        self.prefs.theme = "light".to_string();
                                                        self.palette =
                                                            apply_theme(ctx, self.theme_mode);
                                                        prefs_dirty = true;
                                                    }
                                                });
                                            });
                                    }
                                }
                            });
                    });
            });

            self.show_launcher_settings = open;
            if reload_langs {
                self.lang.reload(&self.paths);
                self.notice_banner = Some(("Reloaded language packs.".to_string(), false));
            }
            if scan_java {
                self.reload_java();
                self.notice_banner = Some((
                    format!(
                        "Detected {} Java runtime(s).",
                        self.java_installations.len()
                    ),
                    false,
                ));
            }
            if let Some(major) = install_java_major {
                self.start_java_install(major, ctx);
            }
            if reset_defaults {
                self.prefs = LauncherPrefs::default();
                self.theme_mode = self.prefs.theme_mode();
                self.palette = apply_theme(ctx, self.theme_mode);
                self.lang.set_language(&self.prefs.normalized_locale());
                self.create_show_snapshots = self.prefs.show_snapshots;
                self.save_prefs();
                self.notice_banner =
                    Some(("Launcher settings restored to defaults.".to_string(), false));
            } else if prefs_dirty {
                self.save_prefs();
            }
        }

        // 7. Dedicated Per-Instance Manual Downloads Isolated OS Windows (CurseForge opt-out mods)
        let open_ids = self.open_manual_modals.clone();
        let mut to_close: Vec<String> = Vec::new();
        let mut to_scan: Vec<String> = Vec::new();
        let mut launch_anyway_id: Option<String> = None;

        for inst_id in open_ids {
            let inst_name = self
                .instances
                .iter()
                .find(|i| i.instance_id == inst_id)
                .map(|i| i.name.clone())
                .unwrap_or_else(|| inst_id.clone());
            let items = self
                .manual_downloads_cache
                .get(&inst_id)
                .cloned()
                .unwrap_or_default();
            let status_text = self.manual_scan_status.get(&inst_id).cloned();

            let mut win_open = true;
            let vp_id = egui::ViewportId::from_hash_of(("mcw_vp_manual_downloads", &inst_id));
            let builder = centered_viewport_builder(
                ctx,
                format!(
                    "Manual Downloads — {} ({} remaining)",
                    inst_name,
                    items.len()
                ),
                [600.0, 500.0],
                [440.0, 340.0],
            );
            ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
                if vp_ctx.input(|i| i.viewport().close_requested()) {
                    win_open = false;
                }
                egui::CentralPanel::default()
                    .frame(Frame::new().fill(p.canvas).inner_margin(egui::Margin::same(12)))
                    .show(vp_ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Instance: {inst_name}"))
                            .strong()
                            .size(13.5)
                            .color(p.accent),
                    );
                    ui.label(
                        RichText::new(format!("({inst_id})"))
                            .monospace()
                            .size(11.0)
                            .color(p.text_muted),
                    );
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "These mods have third-party distribution disabled by their authors on CurseForge.",
                    )
                    .strong()
                    .color(p.warning),
                );
                ui.label(
                    RichText::new(
                        "1. Click 'Open Page' (or 'Open Next 5') to download the .jar files in your browser.\n2. Auto-scan imports completed downloads every 2s, or click 'Scan Now' manually.",
                    )
                    .size(11.5)
                    .color(p.text_muted),
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("Downloads Folder:");
                    ui.add(
                        TextEdit::singleline(&mut self.manual_downloads_dir_input)
                            .desired_width(300.0),
                    );
                    let scan_btn = Button::new(
                        RichText::new("Scan Now")
                            .color(p.accent_text)
                            .strong(),
                    )
                    .fill(p.accent);
                    if ui.add(scan_btn).clicked() {
                        to_scan.push(inst_id.clone());
                    }
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.manual_auto_scan,
                        "Auto-scan Downloads & mods/ every 2s",
                    );
                    if self.manual_auto_scan && !items.is_empty() {
                        ui.label(RichText::new("● Watching").size(11.0).color(p.accent));
                    }
                });
                if let Some(ref status) = status_text {
                    ui.label(RichText::new(status).size(11.5).color(p.text_muted));
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!items.is_empty(), Button::new("Open Next 5 Links"))
                        .clicked()
                    {
                        for item in items.iter().take(5) {
                            open_external(&item.website_url);
                        }
                    }
                    if ui.button("Open Instance mods/ Folder").clicked() {
                        let mods_dir = self.paths.minecraft_dir(&inst_id).join("mods");
                        let _ = std::fs::create_dir_all(&mods_dir);
                        open_external(&mods_dir.display().to_string());
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);

                if items.is_empty() {
                    ui.label(
                        RichText::new("All manual mods have been imported!")
                            .strong()
                            .color(p.accent),
                    );
                } else {
                    ScrollArea::vertical()
                        .id_salt(("manual_downloads_modal_scroll", &inst_id))
                        .max_height(280.0)
                        .show(ui, |ui| {
                            for item in &items {
                                Frame::new()
                                    .fill(p.elevated)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .corner_radius(CornerRadius::same(3))
                                    .inner_margin(egui::Margin::symmetric(8, 5))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(&item.file_name)
                                                        .monospace()
                                                        .strong(),
                                                );
                                                ui.label(
                                                    RichText::new(format!(
                                                        "Mod #{} · File #{}",
                                                        item.mod_id, item.file_id
                                                    ))
                                                    .size(11.0)
                                                    .color(p.text_muted),
                                                );
                                            });
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if ui.button("Open Page").clicked() {
                                                        open_external(&item.website_url);
                                                    }
                                                },
                                            );
                                        });
                                    });
                                ui.add_space(4.0);
                            }
                        });
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        to_close.push(inst_id.clone());
                    }
                    if !items.is_empty() && self.active_account().is_some() {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .button(
                                    RichText::new("Launch Anyway (Incomplete)").color(p.warning),
                                )
                                .on_hover_text(
                                    "Not recommended: this modpack is missing required manual mods and may crash or lack content.",
                                )
                                .clicked()
                            {
                                launch_anyway_id = Some(inst_id.clone());
                            }
                        });
                    }
                });
                });
            });

            if !win_open {
                to_close.push(inst_id);
            }
        }

        for id in to_scan {
            self.run_manual_downloads_scan(&id, true);
        }
        for id in to_close {
            self.open_manual_modals.retain(|open_id| open_id != &id);
        }
        if let Some(id) = launch_anyway_id {
            self.open_manual_modals.retain(|open_id| open_id != &id);
            self.selected_instance_id = Some(id);
            self.sync_selected_from_cache();
            self.start_launch(ctx);
        }
    }

    pub(crate) fn render_modpack_install_dialog(&mut self, ctx: &egui::Context) {
        if !self.modpack_dialog.open {
            return;
        }
        let p = self.palette;
        let mut open = self.modpack_dialog.open;
        let mut trigger_install = false;
        let mut filter_changed = false;
        let mut selected_changed = false;
        let mut browse_java = false;

        let title = format!(
            "{} ({}) — MCW Launcher",
            self.modpack_dialog.pack_title, self.modpack_dialog.provider_label
        );
        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_modpack_install");
        let builder = centered_viewport_builder(ctx, title, [560.0, 560.0], [420.0, 420.0]);

        let filtered_options: Vec<ModpackVersionOption> = self
            .filtered_modpack_versions()
            .into_iter()
            .cloned()
            .collect();

        let beta_lbl = self.t("content.channel.beta").to_string();
        let alpha_lbl = self.t("content.channel.alpha").to_string();
        let open_web_lbl = self.t("content.open_web").to_string();
        let inst_name_lbl = self.t("modrinth.modpack.instance_name").to_string();
        let min_ram_lbl = self.t("instance_defaults.memory.minimum").to_string();
        let max_ram_lbl = self.t("instance_defaults.memory.maximum").to_string();
        let java_mode_lbl = self.t("java.selection.mode_label").to_string();
        let java_auto_lbl = self.t("java.selection.mode.auto").to_string();
        let java_custom_lbl = self.t("java.selection.mode.custom").to_string();
        let java_browse_lbl = self.t("java.selection.browse").to_string();
        let cancel_lbl = self.t("common.cancel").to_string();

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested()) {
                open = false;
            }
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    ScrollArea::vertical()
                        .id_salt("modpack_install_dialog_scroll")
                        .show(ui, |ui| {
                            // Header: Icon + Pack Name + Provider + Open on Web
                            ui.horizontal(|ui| {
                                self.render_thumbnail(
                                    ui,
                                    self.modpack_dialog.icon_url.as_deref(),
                                    48.0,
                                );
                                ui.add_space(8.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(&self.modpack_dialog.pack_title)
                                            .strong()
                                            .size(16.0)
                                            .color(p.text),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{} Modpack",
                                            self.modpack_dialog.provider_label
                                        ))
                                        .size(11.5)
                                        .color(p.text_muted),
                                    );
                                });
                                if let Some(ref web_url) = self.modpack_dialog.website_url {
                                    let url_clone = web_url.clone();
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.button(&open_web_lbl).clicked() {
                                            open_external(&url_clone);
                                        }
                                    });
                                }
                            });

                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(8.0);

                            // 1. TOP PRIORITY: Version selector + Beta & Alpha checkboxes
                            ui.label(RichText::new("Version").strong().size(13.0).color(p.accent));
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .checkbox(&mut self.modpack_dialog.include_beta, &beta_lbl)
                                    .changed()
                                {
                                    filter_changed = true;
                                }
                                if ui
                                    .checkbox(&mut self.modpack_dialog.include_alpha, &alpha_lbl)
                                    .changed()
                                {
                                    filter_changed = true;
                                }
                                ui.label(
                                    RichText::new(format!(
                                        "({} available)",
                                        filtered_options.len()
                                    ))
                                    .size(11.0)
                                    .color(p.text_muted),
                                );
                            });
                            ui.add_space(4.0);

                            if self.modpack_dialog.loading_versions {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(
                                        RichText::new("Loading modpack versions...")
                                            .color(p.text_muted),
                                    );
                                });
                            } else if let Some(ref err) = self.modpack_dialog.error {
                                ui.label(RichText::new(err).color(p.danger));
                            } else if filtered_options.is_empty() {
                                ui.label(
                                    RichText::new("No downloadable versions found.")
                                        .color(p.warning),
                                );
                            } else {
                                let selected_label = filtered_options
                                    .iter()
                                    .find(|v| v.id == self.modpack_dialog.selected_version_id)
                                    .map(|v| v.label.as_str())
                                    .unwrap_or("Select version...");
                                let max_w = (ui.available_width() - 8.0).max(200.0);
                                let combo_w = auto_combo_width(ui, selected_label, 220.0, max_w);
                                egui::ComboBox::from_id_salt("modpack_install_version_combo")
                                    .selected_text(selected_label)
                                    .width(combo_w)
                                    .truncate()
                                    .show_ui(ui, |ui| {
                                        for opt in &filtered_options {
                                            let badge = match opt.channel {
                                                ModpackChannel::Release => "[R]",
                                                ModpackChannel::Beta => "[B]",
                                                ModpackChannel::Alpha => "[A]",
                                            };
                                            let text = format!("{badge} {}", opt.label);
                                            if ui
                                                .selectable_value(
                                                    &mut self.modpack_dialog.selected_version_id,
                                                    opt.id.clone(),
                                                    text,
                                                )
                                                .changed()
                                            {
                                                selected_changed = true;
                                            }
                                        }
                                    });
                            }

                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(8.0);

                            // 2. Instance Name
                            ui.label(RichText::new(&inst_name_lbl).strong().size(12.5));
                            ui.add(
                                TextEdit::singleline(&mut self.modpack_dialog.instance_name)
                                    .desired_width((ui.available_width() - 8.0).max(240.0)),
                            );

                            ui.add_space(10.0);

                            // 3. RAM Settings (Forge/NeoForge: min(PC_RAM, 8192); general: min(PC_RAM, 4096))
                            let active_loader = filtered_options
                                .iter()
                                .find(|v| v.id == self.modpack_dialog.selected_version_id)
                                .map(|v| v.loader.as_str())
                                .filter(|l| !l.is_empty())
                                .unwrap_or(&self.modpack_dialog.default_loader);
                            let ram_cap = self.pc_ram_mb.max(1024);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("RAM (MB)").strong().size(12.5));
                                ui.label(
                                    RichText::new(format!(
                                        "(PC RAM: {} MB · Loader: {})",
                                        self.pc_ram_mb, active_loader
                                    ))
                                    .size(11.0)
                                    .color(p.text_muted),
                                );
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.label(&min_ram_lbl);
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut self.modpack_dialog.min_ram_mb)
                                            .speed(128)
                                            .range(256..=ram_cap)
                                            .suffix(" MB"),
                                    )
                                    .changed()
                                {
                                    self.modpack_dialog.ram_user_edited = true;
                                    if self.modpack_dialog.max_ram_mb
                                        < self.modpack_dialog.min_ram_mb
                                    {
                                        self.modpack_dialog.max_ram_mb =
                                            self.modpack_dialog.min_ram_mb;
                                    }
                                }
                                ui.add_space(8.0);
                                ui.label(&max_ram_lbl);
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut self.modpack_dialog.max_ram_mb)
                                            .speed(128)
                                            .range(512..=ram_cap)
                                            .suffix(" MB"),
                                    )
                                    .changed()
                                {
                                    self.modpack_dialog.ram_user_edited = true;
                                    if self.modpack_dialog.min_ram_mb
                                        > self.modpack_dialog.max_ram_mb
                                    {
                                        self.modpack_dialog.min_ram_mb =
                                            self.modpack_dialog.max_ram_mb;
                                    }
                                }
                            });

                            ui.add_space(10.0);

                            // 4. Java Selection (Dropdown)
                            ui.label(RichText::new(&java_mode_lbl).strong().size(12.5));
                            let cur_java_mode = if self.modpack_dialog.java_custom {
                                &java_custom_lbl
                            } else {
                                &java_auto_lbl
                            };
                            egui::ComboBox::from_id_salt("modpack_install_java_mode_combo")
                                .selected_text(cur_java_mode)
                                .width(280.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.modpack_dialog.java_custom,
                                        false,
                                        &java_auto_lbl,
                                    );
                                    ui.selectable_value(
                                        &mut self.modpack_dialog.java_custom,
                                        true,
                                        &java_custom_lbl,
                                    );
                                });
                            if self.modpack_dialog.java_custom {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.add(
                                        TextEdit::singleline(&mut self.modpack_dialog.java_path)
                                            .hint_text("Path to javaw.exe / java.exe")
                                            .desired_width(
                                                (ui.available_width() - 120.0).max(160.0),
                                            ),
                                    );
                                    if ui.button(&java_browse_lbl).clicked() {
                                        browse_java = true;
                                    }
                                });
                                if !self.java_installations.is_empty() {
                                    ui.horizontal_wrapped(|ui| {
                                        for j in &self.java_installations {
                                            let lbl = format!("Java {}", j.major);
                                            if ui.small_button(lbl).clicked() {
                                                self.modpack_dialog.java_path =
                                                    j.path.display().to_string();
                                            }
                                        }
                                    });
                                }
                            }

                            ui.add_space(14.0);
                            ui.separator();
                            ui.add_space(8.0);

                            // Footer: Install + Cancel
                            let can_install = !self.modpack_dialog.loading_versions
                                && !filtered_options.is_empty()
                                && !self.modpack_dialog.selected_version_id.is_empty();
                            ui.horizontal(|ui| {
                                let install_btn = Button::new(
                                    RichText::new("Install Modpack")
                                        .color(p.accent_text)
                                        .strong(),
                                )
                                .fill(p.accent);
                                if ui.add_enabled(can_install, install_btn).clicked() {
                                    trigger_install = true;
                                }
                                if ui.button(&cancel_lbl).clicked() {
                                    open = false;
                                }
                            });
                        });
                });
        });

        self.modpack_dialog.open = open;
        if filter_changed || selected_changed {
            self.sync_modpack_dialog_selection();
        }
        if browse_java {
            if let Some(p) = pick_file_dialog(
                &java_browse_lbl,
                "Java Executable (javaw.exe;java.exe)|javaw.exe;java.exe|All Files (*.*)|*.*",
            ) {
                self.modpack_dialog.java_path = p.display().to_string();
            }
        }
        if trigger_install {
            self.confirm_modpack_install_dialog(ctx);
        }
    }

    pub(crate) fn render_modpack_provider_window(&mut self, ctx: &egui::Context) {
        let Some(mut provider_tab) = self.show_modpack_provider_window else {
            return;
        };
        let p = self.palette;
        let mut open = true;
        let provider_name = match provider_tab {
            CreateInstanceTab::ModrinthPacks => "Modrinth Modpacks",
            CreateInstanceTab::CurseForgePacks => "CurseForge Modpacks",
            CreateInstanceTab::Ftb => "FTB Modpacks",
            CreateInstanceTab::AtLauncher => "ATLauncher Modpacks",
            _ => "Modrinth Modpacks",
        };
        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_modpack_provider_browser");
        let builder = centered_viewport_builder(
            ctx,
            format!("Browse {provider_name} — MCW Launcher"),
            [720.0, 560.0],
            [520.0, 400.0],
        );

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested()) {
                open = false;
            }
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Provider:").strong());
                        egui::ComboBox::from_id_salt("modpack_provider_window_combo")
                            .selected_text(provider_name)
                            .width(180.0)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_value(
                                        &mut provider_tab,
                                        CreateInstanceTab::ModrinthPacks,
                                        "Modrinth Modpacks",
                                    )
                                    .clicked()
                                {
                                    self.pack_loader_filter = "fabric".to_string();
                                }
                                if ui
                                    .selectable_value(
                                        &mut provider_tab,
                                        CreateInstanceTab::CurseForgePacks,
                                        "CurseForge Modpacks",
                                    )
                                    .clicked()
                                {
                                    self.pack_loader_filter = "forge".to_string();
                                }
                                if ui
                                    .selectable_value(
                                        &mut provider_tab,
                                        CreateInstanceTab::Ftb,
                                        "FTB Modpacks",
                                    )
                                    .clicked()
                                {
                                    self.pack_loader_filter = "forge".to_string();
                                }
                                if ui
                                    .selectable_value(
                                        &mut provider_tab,
                                        CreateInstanceTab::AtLauncher,
                                        "ATLauncher Modpacks",
                                    )
                                    .clicked()
                                {
                                    self.pack_loader_filter = "forge".to_string();
                                }
                            });

                        if matches!(
                            provider_tab,
                            CreateInstanceTab::ModrinthPacks | CreateInstanceTab::CurseForgePacks
                        ) {
                            ui.label("Loader:");
                            egui::ComboBox::from_id_salt("modpack_provider_loader_combo")
                                .selected_text(&self.pack_loader_filter)
                                .width(100.0)
                                .show_ui(ui, |ui| {
                                    for l in ["fabric", "forge", "neoforge", "quilt", "all"] {
                                        ui.selectable_value(
                                            &mut self.pack_loader_filter,
                                            l.to_string(),
                                            l,
                                        );
                                    }
                                });
                        }

                        let hint = match provider_tab {
                            CreateInstanceTab::ModrinthPacks => {
                                "Search Modrinth modpacks (e.g. Fabulously Optimized)..."
                            }
                            CreateInstanceTab::CurseForgePacks => {
                                "Search CurseForge modpacks (e.g. RLCraft, ATM10)..."
                            }
                            CreateInstanceTab::Ftb => {
                                "Search FTB modpacks (e.g. StoneBlock, Skies)..."
                            }
                            CreateInstanceTab::AtLauncher => {
                                "Search ATLauncher modpacks (or leave empty)..."
                            }
                            _ => "Search modpacks...",
                        };
                        let te = ui.add(
                            TextEdit::singleline(&mut self.pack_browser_query)
                                .hint_text(hint)
                                .desired_width(240.0),
                        );
                        let go = ui
                            .add_enabled(!self.pack_browser_searching, Button::new("Search"))
                            .clicked();
                        if go
                            || (te.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            match provider_tab {
                                CreateInstanceTab::ModrinthPacks => {
                                    self.start_modrinth_modpacks_search(ctx)
                                }
                                CreateInstanceTab::CurseForgePacks => {
                                    self.start_curseforge_modpack_search(ctx)
                                }
                                CreateInstanceTab::Ftb => self.start_ftb_search(ctx),
                                CreateInstanceTab::AtLauncher => self.start_atlauncher_search(ctx),
                                _ => {}
                            }
                        }
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    match provider_tab {
                        CreateInstanceTab::ModrinthPacks => {
                            let mr_urls: Vec<String> = self
                                .mr_pack_results
                                .iter()
                                .filter_map(|h| h.icon_url.clone())
                                .collect();
                            for u in mr_urls {
                                self.ensure_thumbnail(&u, ctx);
                            }
                            let mut mr_pick: Option<(String, String, Option<String>)> = None;
                            ScrollArea::vertical()
                                .id_salt("vp_mr_packs_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let inner_card_w = (ui.available_width() - 24.0).max(200.0);
                                    if self.mr_pack_results.is_empty() {
                                        ui.label(
                                            RichText::new(
                                                "Click Search to browse Modrinth modpacks.",
                                            )
                                            .color(p.text_muted),
                                        );
                                    } else {
                                        for hit in &self.mr_pack_results {
                                            let key = format!("mr:{}", hit.slug);
                                            let busy_this = self.installing_ext_packs.contains(&key);
                                            Frame::new()
                                                .fill(p.elevated)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(3))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.set_width(inner_card_w);
                                                    ui.horizontal(|ui| {
                                                        self.render_thumbnail(
                                                            ui,
                                                            hit.icon_url.as_deref(),
                                                            40.0,
                                                        );
                                                        ui.add_space(6.0);
                                                        let text_max_w =
                                                            (inner_card_w - 150.0).max(120.0);
                                                        ui.vertical(|ui| {
                                                            ui.set_max_width(text_max_w);
                                                            ui.horizontal_wrapped(|ui| {
                                                                ui.label(
                                                                    RichText::new(&hit.title)
                                                                        .strong(),
                                                                );
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "{} downloads",
                                                                        hit.downloads
                                                                    ))
                                                                    .monospace()
                                                                    .size(11.0)
                                                                    .color(p.text_muted),
                                                                );
                                                            });
                                                            if !hit.description.is_empty() {
                                                                ui.label(
                                                                    RichText::new(&hit.description)
                                                                        .size(11.5)
                                                                        .color(p.text_muted),
                                                                );
                                                            }
                                                        });
                                                        ui.with_layout(
                                                            Layout::right_to_left(Align::Center),
                                                            |ui| {
                                                                let btn = Button::new(
                                                                    RichText::new(if busy_this {
                                                                        "Installing..."
                                                                    } else {
                                                                        "Install..."
                                                                    })
                                                                    .color(p.accent_text)
                                                                    .strong(),
                                                                )
                                                                .fill(p.accent);
                                                                if ui
                                                                    .add_enabled(!busy_this, btn)
                                                                    .clicked()
                                                                {
                                                                    mr_pick = Some((
                                                                        hit.slug.clone(),
                                                                        hit.title.clone(),
                                                                        hit.icon_url.clone(),
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
                            if let Some((slug, title, icon)) = mr_pick {
                                let web = format!("https://modrinth.com/modpack/{slug}");
                                self.open_modpack_install_dialog(
                                    ModpackInstallProvider::Modrinth { slug },
                                    "Modrinth",
                                    title,
                                    icon,
                                    Some(web),
                                    "fabric",
                                    ctx,
                                );
                            }
                        }
                        CreateInstanceTab::CurseForgePacks => {
                            if self.resolved_cf_api_key().is_empty() {
                                ui.label(
                                    RichText::new(
                                        "CurseForge API key is not configured. Add CURSEFORGE_API_KEY to .env.",
                                    )
                                    .color(p.warning),
                                );
                                ui.add_space(6.0);
                            }
                            let cf_urls: Vec<String> = self
                                .cf_pack_results
                                .iter()
                                .filter_map(|m| m.icon_url().map(str::to_string))
                                .collect();
                            for u in cf_urls {
                                self.ensure_thumbnail(&u, ctx);
                            }
                            #[allow(clippy::type_complexity)]
                            let mut install_cf_pack: Option<(
                                u64,
                                String,
                                String,
                                Option<String>,
                                Option<String>,
                            )> = None;
                            ScrollArea::vertical()
                                .id_salt("vp_cf_packs_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let inner_card_w = (ui.available_width() - 24.0).max(200.0);
                                    if self.cf_pack_results.is_empty() {
                                        ui.label(
                                            RichText::new(
                                                "Click Search to browse CurseForge modpacks.",
                                            )
                                            .color(p.text_muted),
                                        );
                                    } else {
                                        for pack in &self.cf_pack_results {
                                            Frame::new()
                                                .fill(p.elevated)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(3))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.set_width(inner_card_w);
                                                    ui.horizontal(|ui| {
                                                        self.render_thumbnail(
                                                            ui,
                                                            pack.icon_url(),
                                                            40.0,
                                                        );
                                                        ui.add_space(6.0);
                                                        let text_max_w =
                                                            (inner_card_w - 160.0).max(120.0);
                                                        ui.vertical(|ui| {
                                                            ui.set_max_width(text_max_w);
                                                            ui.horizontal_wrapped(|ui| {
                                                                ui.label(
                                                                    RichText::new(&pack.name)
                                                                        .strong(),
                                                                );
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "#{} · {} downloads",
                                                                        pack.id,
                                                                        pack.download_count
                                                                    ))
                                                                    .monospace()
                                                                    .size(11.0)
                                                                    .color(p.text_muted),
                                                                );
                                                            });
                                                            if !pack.summary.is_empty() {
                                                                ui.label(
                                                                    RichText::new(&pack.summary)
                                                                        .size(11.5)
                                                                        .color(p.text_muted),
                                                                );
                                                            }
                                                        });
                                                        ui.with_layout(
                                                            Layout::right_to_left(Align::Center),
                                                            |ui| {
                                                                let is_this_installing = self
                                                                    .installing_cf_packs
                                                                    .contains(&pack.id);
                                                                let btn = Button::new(
                                                                    RichText::new(
                                                                        if is_this_installing {
                                                                            "Installing..."
                                                                        } else {
                                                                            "Install Pack"
                                                                        },
                                                                    )
                                                                    .color(p.accent_text)
                                                                    .strong(),
                                                                )
                                                                .fill(p.accent);
                                                                if ui
                                                                    .add_enabled(
                                                                        !is_this_installing,
                                                                        btn,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    install_cf_pack = Some((
                                                                        pack.id,
                                                                        pack.slug.clone(),
                                                                        pack.name.clone(),
                                                                        pack.icon_url()
                                                                            .map(str::to_string),
                                                                        pack.links
                                                                            .as_ref()
                                                                            .and_then(|l| {
                                                                                l.website_url.clone()
                                                                            })
                                                                            .or_else(|| {
                                                                                Some(format!(
                                                                                    "https://www.curseforge.com/minecraft/modpacks/{}",
                                                                                    pack.slug
                                                                                ))
                                                                            }),
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
                            if let Some((mid, slug, name, icon, web)) = install_cf_pack {
                                self.open_modpack_install_dialog(
                                    ModpackInstallProvider::CurseForge { mod_id: mid, slug },
                                    "CurseForge",
                                    name,
                                    icon,
                                    web,
                                    "forge",
                                    ctx,
                                );
                            }
                        }
                        CreateInstanceTab::Ftb => {
                            let ftb_urls: Vec<String> = self
                                .ftb_results
                                .iter()
                                .filter_map(|p| p.icon_url().map(str::to_string))
                                .collect();
                            for u in ftb_urls {
                                self.ensure_thumbnail(&u, ctx);
                            }
                            let mut ftb_install_req: Option<(u64, String, Option<String>, String)> =
                                None;
                            ScrollArea::vertical()
                                .id_salt("vp_ftb_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if self.ftb_results.is_empty() {
                                        ui.label(
                                            RichText::new(
                                                "Enter a search term to browse FTB modpacks.",
                                            )
                                            .color(p.text_muted),
                                        );
                                    } else {
                                        for pack in &self.ftb_results {
                                            let key = format!("ftb:{}", pack.id);
                                            let busy_this = self.installing_ext_packs.contains(&key);
                                            Frame::new()
                                                .fill(p.elevated)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(3))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.horizontal(|ui| {
                                                        ui.label(RichText::new(&pack.name).strong());
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "#{} · {} installs",
                                                                pack.id, pack.installs
                                                            ))
                                                            .monospace()
                                                            .size(11.0)
                                                            .color(p.text_muted),
                                                        );
                                                        ui.with_layout(
                                                            Layout::right_to_left(Align::Center),
                                                            |ui| {
                                                                let btn = Button::new(
                                                                    RichText::new(if busy_this {
                                                                        "Installing..."
                                                                    } else {
                                                                        "Install Pack"
                                                                    })
                                                                    .color(p.accent_text)
                                                                    .strong(),
                                                                )
                                                                .fill(p.accent);
                                                                if ui
                                                                    .add_enabled(!busy_this, btn)
                                                                    .clicked()
                                                                {
                                                                    ftb_install_req = Some((
                                                                        pack.id,
                                                                        pack.name.clone(),
                                                                        pack.icon_url()
                                                                            .map(str::to_string),
                                                                        pack.website_url(),
                                                                    ));
                                                                }
                                                            },
                                                        );
                                                    });
                                                    if !pack.synopsis.is_empty() {
                                                        ui.label(
                                                            RichText::new(&pack.synopsis)
                                                                .size(11.5)
                                                                .color(p.text_muted),
                                                        );
                                                    }
                                                });
                                            ui.add_space(4.0);
                                        }
                                    }
                                });
                            if let Some((pid, pname, icon, web)) = ftb_install_req {
                                self.open_modpack_install_dialog(
                                    ModpackInstallProvider::Ftb { pack_id: pid },
                                    "FTB",
                                    pname,
                                    icon,
                                    Some(web),
                                    "forge",
                                    ctx,
                                );
                            }
                        }
                        CreateInstanceTab::AtLauncher => {
                            let mut atl_install_req: Option<AtLauncherPack> = None;
                            ScrollArea::vertical()
                                .id_salt("vp_atl_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if self.atl_results.is_empty() {
                                        ui.label(
                                            RichText::new(
                                                "Click Search to browse ATLauncher modpacks.",
                                            )
                                            .color(p.text_muted),
                                        );
                                    } else {
                                        for pack in &self.atl_results {
                                            let key = format!("atl:{}", pack.id);
                                            let busy_this = self.installing_ext_packs.contains(&key);
                                            let has_ver = !pack.versions.is_empty();
                                            Frame::new()
                                                .fill(p.elevated)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(3))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.horizontal(|ui| {
                                                        ui.label(RichText::new(&pack.name).strong());
                                                        if let Some(latest) = pack.versions.first() {
                                                            ui.label(
                                                                RichText::new(format!(
                                                                    "v{} · MC {}",
                                                                    latest.version,
                                                                    latest.minecraft
                                                                ))
                                                                .monospace()
                                                                .size(11.0)
                                                                .color(p.text_muted),
                                                            );
                                                        }
                                                        ui.with_layout(
                                                            Layout::right_to_left(Align::Center),
                                                            |ui| {
                                                                let btn = Button::new(
                                                                    RichText::new(if busy_this {
                                                                        "Installing..."
                                                                    } else {
                                                                        "Install Pack"
                                                                    })
                                                                    .color(p.accent_text)
                                                                    .strong(),
                                                                )
                                                                .fill(p.accent);
                                                                if ui
                                                                    .add_enabled(
                                                                        !busy_this && has_ver,
                                                                        btn,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    atl_install_req =
                                                                        Some(pack.clone());
                                                                }
                                                            },
                                                        );
                                                    });
                                                    if !pack.description.is_empty() {
                                                        ui.label(
                                                            RichText::new(&pack.description)
                                                                .size(11.5)
                                                                .color(p.text_muted),
                                                        );
                                                    }
                                                });
                                            ui.add_space(4.0);
                                        }
                                    }
                                });
                            if let Some(pack) = atl_install_req {
                                let name = pack.name.clone();
                                let icon = Some(pack.icon_url());
                                let web = Some(pack.website_url());
                                self.open_modpack_install_dialog(
                                    ModpackInstallProvider::AtLauncher { pack },
                                    "ATLauncher",
                                    name,
                                    icon,
                                    web,
                                    "forge",
                                    ctx,
                                );
                            }
                        }
                        _ => {}
                    }
                });
        });

        if open {
            self.show_modpack_provider_window = Some(provider_tab);
        } else {
            self.show_modpack_provider_window = None;
        }
    }

    pub(crate) fn render_mod_browser_window(&mut self, ctx: &egui::Context) {
        if !self.show_mod_browser_window {
            return;
        }
        let Some(inst) = self.selected_instance().cloned() else {
            self.show_mod_browser_window = false;
            return;
        };
        let p = self.palette;
        let mut open = self.show_mod_browser_window;
        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_browse_mods");
        let builder = centered_viewport_builder(
            ctx,
            format!(
                "Add Mods — {} (MC {} · {})",
                inst.name, inst.version_id, inst.mod_loader.name
            ),
            [720.0, 560.0],
            [520.0, 400.0],
        );

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested()) {
                open = false;
            }
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    let provider_lbl = match self.mod_provider_tab {
                        ModProviderTab::Modrinth => "Modrinth",
                        ModProviderTab::CurseForge => "CurseForge",
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Provider:").strong());
                        let prov_w = auto_combo_width(ui, provider_lbl, 95.0, 140.0);
                        egui::ComboBox::from_id_salt("mod_browser_provider_combo")
                            .selected_text(provider_lbl)
                            .width(prov_w)
                            .truncate()
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.mod_provider_tab,
                                    ModProviderTab::Modrinth,
                                    "Modrinth",
                                );
                                ui.selectable_value(
                                    &mut self.mod_provider_tab,
                                    ModProviderTab::CurseForge,
                                    "CurseForge",
                                );
                            });
                        ui.label("Loader:");
                        let loader_w =
                            auto_combo_width(ui, &self.mod_loader_filter, 72.0, 120.0);
                        egui::ComboBox::from_id_salt("mod_browser_loader_filter")
                            .selected_text(&self.mod_loader_filter)
                            .width(loader_w)
                            .truncate()
                            .show_ui(ui, |ui| {
                                for l in ["fabric", "forge", "neoforge", "quilt", "all"] {
                                    ui.selectable_value(
                                        &mut self.mod_loader_filter,
                                        l.to_string(),
                                        l,
                                    );
                                }
                            });
                        let hint = match self.mod_provider_tab {
                            ModProviderTab::Modrinth => {
                                "Search Modrinth mods (e.g. sodium, fabric-api)..."
                            }
                            ModProviderTab::CurseForge => {
                                "Search CurseForge mods (e.g. jei, create)..."
                            }
                        };
                        let search_w = (ui.available_width() - 80.0).clamp(140.0, 320.0);
                        let te = ui.add(
                            TextEdit::singleline(&mut self.mod_search_query)
                                .hint_text(hint)
                                .desired_width(search_w),
                        );
                        let search_clicked = ui
                            .add_enabled(!self.mod_searching, Button::new("Search"))
                            .clicked();
                        if search_clicked
                            || (te.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            match self.mod_provider_tab {
                                ModProviderTab::Modrinth => self.start_modrinth_search(ctx),
                                ModProviderTab::CurseForge => self.start_curseforge_search(ctx),
                            }
                        }
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    match self.mod_provider_tab {
                        ModProviderTab::Modrinth => {
                            let urls: Vec<String> = self
                                .modrinth_results
                                .iter()
                                .filter_map(|h| h.icon_url.clone())
                                .collect();
                            for u in urls {
                                self.ensure_thumbnail(&u, ctx);
                            }
                            let mut mr_pick: Option<(String, String, Option<String>)> = None;
                            ScrollArea::vertical()
                                .id_salt("modrinth_results_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let inner_card_w = (ui.available_width() - 24.0).max(200.0);
                                    if self.modrinth_results.is_empty() {
                                        ui.label(
                                            RichText::new(
                                                "Enter a search query and click Search to browse Modrinth mods.",
                                            )
                                            .color(p.text_muted),
                                        );
                                    } else {
                                        for hit in &self.modrinth_results {
                                            Frame::new()
                                                .fill(p.elevated)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(3))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.set_width(inner_card_w);
                                                    ui.horizontal(|ui| {
                                                        self.render_thumbnail(
                                                            ui,
                                                            hit.icon_url.as_deref(),
                                                            44.0,
                                                        );
                                                        ui.add_space(6.0);
                                                        let text_max_w =
                                                            (inner_card_w - 150.0).max(120.0);
                                                        ui.vertical(|ui| {
                                                            ui.set_max_width(text_max_w);
                                                            ui.horizontal_wrapped(|ui| {
                                                                ui.label(
                                                                    RichText::new(&hit.title)
                                                                        .strong()
                                                                        .size(13.0),
                                                                );
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "by {} · {} downloads",
                                                                        hit.author, hit.downloads
                                                                    ))
                                                                    .monospace()
                                                                    .size(10.5)
                                                                    .color(p.text_muted),
                                                                );
                                                            });
                                                            if !hit.description.is_empty() {
                                                                ui.label(
                                                                    RichText::new(&hit.description)
                                                                        .size(11.5)
                                                                        .color(p.text_muted),
                                                                );
                                                            }
                                                        });
                                                        ui.with_layout(
                                                            Layout::right_to_left(Align::Center),
                                                            |ui| {
                                                                let can_install =
                                                                    !inst.mod_loader.is_vanilla();
                                                                let btn = Button::new(
                                                                    RichText::new("Install...")
                                                                        .color(p.accent_text)
                                                                        .strong(),
                                                                )
                                                                .fill(p.accent);
                                                                let resp =
                                                                    ui.add_enabled(can_install, btn);
                                                                if !can_install {
                                                                    resp.on_disabled_hover_text(
                                                                        "Vanilla instances cannot install mods",
                                                                    );
                                                                } else if resp.clicked() {
                                                                    mr_pick = Some((
                                                                        hit.slug.clone(),
                                                                        hit.title.clone(),
                                                                        hit.icon_url.clone(),
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
                            if let Some((slug, title, icon)) = mr_pick {
                                self.open_modrinth_version_picker(slug, title, icon, false, ctx);
                            }
                        }
                        ModProviderTab::CurseForge => {
                            let urls: Vec<String> = self
                                .curseforge_results
                                .iter()
                                .filter_map(|m| m.icon_url().map(str::to_string))
                                .collect();
                            for u in urls {
                                self.ensure_thumbnail(&u, ctx);
                            }
                            let mut cf_pick: Option<(u64, String, Option<String>)> = None;
                            ScrollArea::vertical()
                                .id_salt("curseforge_results_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let inner_card_w = (ui.available_width() - 24.0).max(200.0);
                                    if self.curseforge_results.is_empty() {
                                        ui.label(
                                            RichText::new(
                                                "Enter a search query and click Search to browse CurseForge mods.",
                                            )
                                            .color(p.text_muted),
                                        );
                                    } else {
                                        for cf_mod in &self.curseforge_results {
                                            Frame::new()
                                                .fill(p.elevated)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(3))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.set_width(inner_card_w);
                                                    ui.horizontal(|ui| {
                                                        self.render_thumbnail(
                                                            ui,
                                                            cf_mod.icon_url(),
                                                            44.0,
                                                        );
                                                        ui.add_space(6.0);
                                                        let text_max_w =
                                                            (inner_card_w - 200.0).max(120.0);
                                                        ui.vertical(|ui| {
                                                            ui.set_max_width(text_max_w);
                                                            ui.horizontal_wrapped(|ui| {
                                                                ui.label(
                                                                    RichText::new(&cf_mod.name)
                                                                        .strong()
                                                                        .size(13.0),
                                                                );
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "{} downloads",
                                                                        cf_mod.download_count
                                                                    ))
                                                                    .monospace()
                                                                    .size(10.5)
                                                                    .color(p.text_muted),
                                                                );
                                                            });
                                                            if !cf_mod.summary.is_empty() {
                                                                ui.label(
                                                                    RichText::new(&cf_mod.summary)
                                                                        .size(11.5)
                                                                        .color(p.text_muted),
                                                                );
                                                            }
                                                        });
                                                        ui.with_layout(
                                                            Layout::right_to_left(Align::Center),
                                                            |ui| {
                                                                let can_install =
                                                                    !inst.mod_loader.is_vanilla();
                                                                let btn = Button::new(
                                                                    RichText::new("Install...")
                                                                        .color(p.accent_text)
                                                                        .strong(),
                                                                )
                                                                .fill(p.accent);
                                                                let resp =
                                                                    ui.add_enabled(can_install, btn);
                                                                if !can_install {
                                                                    resp.on_disabled_hover_text(
                                                                        "Vanilla instances cannot install mods",
                                                                    );
                                                                } else if resp.clicked() {
                                                                    cf_pick = Some((
                                                                        cf_mod.id,
                                                                        cf_mod.name.clone(),
                                                                        cf_mod
                                                                            .icon_url()
                                                                            .map(str::to_string),
                                                                    ));
                                                                }
                                                                if let Some(url) = cf_mod
                                                                    .links
                                                                    .as_ref()
                                                                    .and_then(|l| {
                                                                        l.website_url.as_ref()
                                                                    })
                                                                {
                                                                    if ui.button("Web").clicked() {
                                                                        open_external(url);
                                                                    }
                                                                }
                                                            },
                                                        );
                                                    });
                                                });
                                            ui.add_space(4.0);
                                        }
                                    }
                                });
                            if let Some((mid, name, icon)) = cf_pick {
                                self.open_curseforge_version_picker(mid, name, icon, ctx);
                            }
                        }
                    }
                });
        });

        self.show_mod_browser_window = open;
    }

    pub(crate) fn render_pack_browser_window(&mut self, ctx: &egui::Context) {
        if !self.show_pack_browser_window {
            return;
        }
        let Some(inst) = self.selected_instance().cloned() else {
            self.show_pack_browser_window = false;
            return;
        };
        let p = self.palette;
        let mut open = self.show_pack_browser_window;
        let kind_label = match self.packs_kind {
            ContentKind::ResourcePack => "Resource Packs",
            ContentKind::ShaderPack => "Shader Packs",
        };
        let vp_id = egui::ViewportId::from_hash_of("mcw_vp_browse_packs");
        let builder = centered_viewport_builder(
            ctx,
            format!("Add {kind_label} — {} (MC {})", inst.name, inst.version_id),
            [720.0, 540.0],
            [520.0, 380.0],
        );

        ctx.show_viewport_immediate(vp_id, builder, |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested()) {
                open = false;
            }
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.canvas)
                        .inner_margin(egui::Margin::same(14)),
                )
                .show(vp_ctx, |ui| {
                    let prev_kind = self.packs_kind;
                    let provider_lbl = match self.packs_provider {
                        ModProviderTab::Modrinth => "Modrinth",
                        ModProviderTab::CurseForge => "CurseForge",
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Type:").strong());
                        let kind_w = auto_combo_width(ui, kind_label, 110.0, 160.0);
                        egui::ComboBox::from_id_salt("pack_browser_kind_combo")
                            .selected_text(kind_label)
                            .width(kind_w)
                            .truncate()
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.packs_kind,
                                    ContentKind::ResourcePack,
                                    "Resource Packs",
                                );
                                ui.selectable_value(
                                    &mut self.packs_kind,
                                    ContentKind::ShaderPack,
                                    "Shader Packs",
                                );
                            });
                        ui.label(RichText::new("Provider:").strong());
                        let prov_w = auto_combo_width(ui, provider_lbl, 95.0, 140.0);
                        egui::ComboBox::from_id_salt("pack_browser_provider_combo")
                            .selected_text(provider_lbl)
                            .width(prov_w)
                            .truncate()
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.packs_provider,
                                    ModProviderTab::Modrinth,
                                    "Modrinth",
                                );
                                ui.selectable_value(
                                    &mut self.packs_provider,
                                    ModProviderTab::CurseForge,
                                    "CurseForge",
                                );
                            });
                        let hint = match self.packs_kind {
                            ContentKind::ResourcePack => {
                                "Search resource packs (e.g. Faithful, Bare Bones)..."
                            }
                            ContentKind::ShaderPack => {
                                "Search shaders (e.g. Complementary, BSL)..."
                            }
                        };
                        let search_w = (ui.available_width() - 80.0).clamp(140.0, 320.0);
                        let te = ui.add(
                            TextEdit::singleline(&mut self.packs_query)
                                .hint_text(hint)
                                .desired_width(search_w),
                        );
                        let go = ui
                            .add_enabled(!self.packs_searching, Button::new("Search"))
                            .clicked();
                        if go
                            || (te.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            self.start_content_pack_search(ctx);
                        }
                    });

                    if self.packs_kind != prev_kind {
                        self.packs_mr_results.clear();
                        self.packs_cf_results.clear();
                        self.selected_content_packs = self
                            .content_packs_cache
                            .get(&(inst.instance_id.clone(), self.packs_kind))
                            .cloned()
                            .unwrap_or_default();
                        self.load_content_packs_async(
                            &inst.instance_id,
                            self.packs_kind,
                            ctx,
                            false,
                        );
                    }

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    match self.packs_provider {
                        ModProviderTab::Modrinth => {
                            let mut to_install: Option<(String, String)> = None;
                            ScrollArea::vertical()
                                .id_salt("packs_mr_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let inner_card_w = (ui.available_width() - 24.0).max(200.0);
                                    if self.packs_mr_results.is_empty() {
                                        ui.label(
                                            RichText::new(
                                                "Enter a search query and click Search to browse Modrinth packs.",
                                            )
                                            .color(p.text_muted),
                                        );
                                    } else {
                                        for hit in &self.packs_mr_results {
                                            Frame::new()
                                                .fill(p.elevated)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(3))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.set_width(inner_card_w);
                                                    ui.horizontal(|ui| {
                                                        let text_max_w =
                                                            (inner_card_w - 100.0).max(120.0);
                                                        ui.vertical(|ui| {
                                                            ui.set_max_width(text_max_w);
                                                            ui.horizontal_wrapped(|ui| {
                                                                ui.label(
                                                                    RichText::new(&hit.title)
                                                                        .strong(),
                                                                );
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "({}) · {} downloads",
                                                                        hit.slug, hit.downloads
                                                                    ))
                                                                    .monospace()
                                                                    .size(11.0)
                                                                    .color(p.text_muted),
                                                                );
                                                            });
                                                            if !hit.description.is_empty() {
                                                                ui.label(
                                                                    RichText::new(&hit.description)
                                                                        .size(11.5)
                                                                        .color(p.text_muted),
                                                                );
                                                            }
                                                        });
                                                        ui.with_layout(
                                                            Layout::right_to_left(Align::Center),
                                                            |ui| {
                                                                let busy_item = self
                                                                    .installing_content_packs
                                                                    .contains(&hit.slug);
                                                                let label = if busy_item {
                                                                    "Installing..."
                                                                } else {
                                                                    "Install"
                                                                };
                                                                let btn = Button::new(
                                                                    RichText::new(label)
                                                                        .color(p.accent_text)
                                                                        .strong(),
                                                                )
                                                                .fill(p.accent);
                                                                if ui
                                                                    .add_enabled(!busy_item, btn)
                                                                    .clicked()
                                                                {
                                                                    to_install = Some((
                                                                        hit.slug.clone(),
                                                                        hit.title.clone(),
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
                            if let Some((slug, title)) = to_install {
                                self.start_content_pack_install(false, slug, title, None, ctx);
                            }
                        }
                        ModProviderTab::CurseForge => {
                            let mut to_install: Option<(u64, String, String)> = None;
                            ScrollArea::vertical()
                                .id_salt("packs_cf_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let inner_card_w = (ui.available_width() - 24.0).max(200.0);
                                    if self.packs_cf_results.is_empty() {
                                        ui.label(
                                            RichText::new(
                                                "Enter a search query and click Search to browse CurseForge packs.",
                                            )
                                            .color(p.text_muted),
                                        );
                                    } else {
                                        for cf in &self.packs_cf_results {
                                            Frame::new()
                                                .fill(p.elevated)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(3))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.set_width(inner_card_w);
                                                    ui.horizontal(|ui| {
                                                        let text_max_w =
                                                            (inner_card_w - 100.0).max(120.0);
                                                        ui.vertical(|ui| {
                                                            ui.set_max_width(text_max_w);
                                                            ui.horizontal_wrapped(|ui| {
                                                                ui.label(
                                                                    RichText::new(&cf.name)
                                                                        .strong(),
                                                                );
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "#{} · {} downloads",
                                                                        cf.id, cf.download_count
                                                                    ))
                                                                    .monospace()
                                                                    .size(11.0)
                                                                    .color(p.text_muted),
                                                                );
                                                            });
                                                            if !cf.summary.is_empty() {
                                                                ui.label(
                                                                    RichText::new(&cf.summary)
                                                                        .size(11.5)
                                                                        .color(p.text_muted),
                                                                );
                                                            }
                                                        });
                                                        ui.with_layout(
                                                            Layout::right_to_left(Align::Center),
                                                            |ui| {
                                                                let key = format!("cf-{}", cf.id);
                                                                let busy_item = self
                                                                    .installing_content_packs
                                                                    .contains(&key);
                                                                let label = if busy_item {
                                                                    "Installing..."
                                                                } else {
                                                                    "Install"
                                                                };
                                                                let btn = Button::new(
                                                                    RichText::new(label)
                                                                        .color(p.accent_text)
                                                                        .strong(),
                                                                )
                                                                .fill(p.accent);
                                                                if ui
                                                                    .add_enabled(!busy_item, btn)
                                                                    .clicked()
                                                                {
                                                                    to_install = Some((
                                                                        cf.id,
                                                                        cf.slug.clone(),
                                                                        cf.name.clone(),
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
                            if let Some((mid, slug, name)) = to_install {
                                self.start_content_pack_install(
                                    true,
                                    format!("cf-{mid}"),
                                    name,
                                    Some(mid),
                                    ctx,
                                );
                                let _ = slug;
                            }
                        }
                    }
                });
        });

        self.show_pack_browser_window = open;
    }
}
