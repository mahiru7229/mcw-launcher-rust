use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_modals(&mut self, ctx: &egui::Context) {
        let p = self.palette;

        // 1. Create Instance (2 tabs: Custom | Modpacks) Isolated OS Window
        if self.show_create_modal {
            let mut open = self.show_create_modal;
            let mut open_browser_prov: Option<BrowserProvider> = None;
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
                                            open_browser_prov = Some(BrowserProvider::Modrinth);
                                        }
                                        if ui.button("Browse CurseForge Modpacks...").clicked() {
                                            open_browser_prov = Some(BrowserProvider::CurseForge);
                                        }
                                        if ui.button("Browse FTB Modpacks...").clicked() {
                                            open_browser_prov = Some(BrowserProvider::Ftb);
                                        }
                                        if ui.button("Browse ATLauncher Modpacks...").clicked() {
                                            open_browser_prov = Some(BrowserProvider::AtLauncher);
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
                                        if ui
                                            .add(
                                                TextEdit::singleline(&mut self.modpack_path_input)
                                                    .hint_text(
                                                        r"C:\Users\...\Downloads\modpack.mcwpack / .mrpack / .zip",
                                                    )
                                                    .desired_width(380.0),
                                            )
                                            .changed()
                                        {
                                            self.inspect_local_archive_optional_files();
                                        }
                                        if ui.button(self.t("common.browse")).clicked() {
                                            if let Some(p) = pick_file_dialog(
                                                "Select Modpack / Instance Package",
                                                "MCW & Modpack Archives (*.mcwpack;*.mrpack;*.zip)|*.mcwpack;*.mrpack;*.zip|All Files (*.*)|*.*",
                                            ) {
                                                self.modpack_path_input = p.display().to_string();
                                                self.inspect_local_archive_optional_files();
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
                                    if !self.archive_optional_files.is_empty() {
                                        ui.add_space(6.0);
                                        let opt_hdr = self
                                            .t("modrinth.modpack.optional_files")
                                            .to_string();
                                        ui.checkbox(&mut self.archive_include_optional, &opt_hdr);
                                        if self.archive_include_optional {
                                            ui.horizontal(|ui| {
                                                if ui.small_button("Select All").clicked() {
                                                    self.archive_selected_optional_ids = self
                                                        .archive_optional_files
                                                        .iter()
                                                        .map(|f| f.id.clone())
                                                        .collect();
                                                }
                                                if ui.small_button("Select None").clicked() {
                                                    self.archive_selected_optional_ids.clear();
                                                }
                                            });
                                            ScrollArea::vertical()
                                                .id_salt("archive_optional_files_scroll")
                                                .max_height(110.0)
                                                .auto_shrink([false, false])
                                                .show(ui, |ui| {
                                                    for opt in &self.archive_optional_files {
                                                        let mut checked = self
                                                            .archive_selected_optional_ids
                                                            .contains(&opt.id);
                                                        if ui
                                                            .checkbox(&mut checked, &opt.name)
                                                            .changed()
                                                        {
                                                            if checked {
                                                                self.archive_selected_optional_ids
                                                                    .insert(opt.id.clone());
                                                            } else {
                                                                self.archive_selected_optional_ids
                                                                    .remove(&opt.id);
                                                            }
                                                        }
                                                    }
                                                });
                                        }
                                    }

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
            if let Some(prov) = open_browser_prov {
                self.show_create_modal = false;
                self.open_content_browser(BrowserCategory::Modpacks, Some(prov), ctx);
            } else if !open {
                self.show_create_modal = false;
            }
        }

        // 1a-0. Dedicated Isolated OS Windows for Online Content Browser, Updater, First-Run, Repair, Logs & About
        self.render_content_browser_window(ctx);
        self.render_update_dialog(ctx);
        self.render_first_run_wizard(ctx);
        self.render_repair_center_window(ctx);
        self.render_global_logs_window(ctx);
        self.render_about_window(ctx);
        self.render_lan_agent_log_window(ctx);

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
                                self.render_account_avatar(ui, &acc.uuid, 22.0);
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
            let mut discord_changed = false;
            let mut open_first_run = false;
            let trigger_storage_scan = self.launcher_settings_tab == LauncherSettingsTab::Storage
                && self.storage_summary.is_none()
                && !self.storage_scanning;
            let mut install_java_major: Option<u32> = None;
            let mut uninstall_java_exe: Option<PathBuf> = None;
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
                (
                    LauncherSettingsTab::Storage,
                    self.t("storage.settings.section").to_string(),
                ),
                (LauncherSettingsTab::Updates, "Updates".to_string()),
            ];
            let vp_ctx = ctx;
            let screen = ctx.screen_rect();
            let modal_w = (screen.width() - 80.0).clamp(420.0, 900.0);
            let modal_h = (screen.height() - 90.0).clamp(320.0, 620.0);
            let opened_at = *self.settings_open_since.get_or_insert_with(Instant::now);
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
            let close_lbl = self.t("common.close").to_string();
            let search_hint = self.t("Search settings").to_string();
            let settings_q = self.settings_search.trim().to_lowercase();
            egui::Area::new(egui::Id::new("mcw_settings_backdrop"))
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
            egui::Window::new(win_title.clone())
                .id(egui::Id::new("mcw_settings_modal"))
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
                            246,
                        ))
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(18))
                        .inner_margin(egui::Margin::same(16)),
                )
                .show(ctx, |ui| {
                    ui.set_opacity(0.4 + 0.6 * fade);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&win_title).strong().size(20.0).color(p.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(Button::new(close_lbl.as_str()).corner_radius(CornerRadius::same(14)))
                                .clicked()
                            {
                                open = false;
                            }
                        });
                    });
                    ui.add_space(6.0);
                    let body_h = ui.available_height();
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(190.0, body_h),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.add(
                                    TextEdit::singleline(&mut self.settings_search)
                                        .hint_text(search_hint.as_str())
                                        .desired_width(ui.available_width()),
                                );
                                ui.add_space(8.0);
                                for (tab, label) in &nav_tabs {
                                    if !settings_q.is_empty()
                                        && !label.to_lowercase().contains(&settings_q)
                                    {
                                        continue;
                                    }
                                    let active = self.launcher_settings_tab == *tab;
                                    let btn = Button::new(
                                        RichText::new(label).strong().size(13.0).color(
                                            if active { p.text } else { p.text_muted },
                                        ),
                                    )
                                    .fill(if active {
                                        p.accent.gamma_multiply(0.30)
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .stroke(if active {
                                        Stroke::new(1.0_f32, p.accent)
                                    } else {
                                        Stroke::NONE
                                    })
                                    .min_size(Vec2::new(ui.available_width(), 36.0))
                                    .corner_radius(CornerRadius::same(10));
                                    if ui.add(btn).clicked() {
                                        self.launcher_settings_tab = *tab;
                                    }
                                    ui.add_space(4.0);
                                }
                            },
                        );
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), body_h),
                            Layout::top_down(Align::Min),
                            |ui| {
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
                                                if ui
                                                    .checkbox(
                                                        &mut self.prefs.minimize_on_launch,
                                                        "Minimize launcher window when Minecraft launches",
                                                    )
                                                    .changed()
                                                {
                                                    prefs_dirty = true;
                                                }
                                                if ui
                                                    .checkbox(
                                                        &mut self.prefs.remember_window_size,
                                                        "Remember launcher window size across sessions",
                                                    )
                                                    .changed()
                                                {
                                                    prefs_dirty = true;
                                                }
                                                if ui
                                                    .checkbox(
                                                        &mut self.prefs.prefer_dedicated_gpu,
                                                        "Prefer dedicated GPU for Java / Minecraft launches",
                                                    )
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

                                        // Card 3: Discord Rich Presence
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new("Discord Rich Presence").strong(),
                                                );
                                                ui.label(
                                                    RichText::new(
                                                        "Show active instance status in Discord while browsing or playing.",
                                                    )
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                                ui.add_space(6.0);
                                                if ui
                                                    .checkbox(
                                                        &mut self.prefs.discord_rpc_enabled,
                                                        "Enable Discord Rich Presence",
                                                    )
                                                    .changed()
                                                {
                                                    prefs_dirty = true;
                                                    discord_changed = true;
                                                }
                                                ui.add_space(4.0);
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label("Application ID (optional):");
                                                    ui.add(
                                                        TextEdit::singleline(
                                                            &mut self.prefs.discord_client_id,
                                                        )
                                                        .hint_text(DEFAULT_DISCORD_CLIENT_ID)
                                                        .desired_width(210.0),
                                                    );
                                                    if ui.button("Apply").clicked() {
                                                        prefs_dirty = true;
                                                        discord_changed = true;
                                                    }
                                                });
                                            });

                                        ui.add_space(8.0);

                                        // Card 4: First-Run Setup Wizard
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(
                                                        self.t("launcher_settings.first_run.title"),
                                                    )
                                                    .strong(),
                                                );
                                                ui.label(
                                                    RichText::new(
                                                        self.t("launcher_settings.first_run.detail"),
                                                    )
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                                ui.add_space(6.0);
                                                let btn_lbl = self
                                                    .t("launcher_settings.first_run.button")
                                                    .to_string();
                                                if ui.button(btn_lbl).clicked() {
                                                    open_first_run = true;
                                                }
                                            });

                                        ui.add_space(8.0);

                                        // Card 5: Reset to defaults
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
                                                ui.add_space(6.0);
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label(
                                                        "Speed limit (MB/s, 0 = Unlimited):",
                                                    );
                                                    if ui
                                                        .add(
                                                            egui::DragValue::new(
                                                                &mut self.prefs.download_limit_mbps,
                                                            )
                                                            .range(0.0..=1024.0)
                                                            .speed(0.5),
                                                        )
                                                        .changed()
                                                    {
                                                        let normalized =
                                                            mcw_net::set_global_bandwidth_limit_mbps(
                                                                self.prefs.download_limit_mbps,
                                                            );
                                                        self.prefs.download_limit_mbps = normalized;
                                                        prefs_dirty = true;
                                                    }
                                                });
                                            });

                                        ui.add_space(8.0);

                                        // Card 1b: Custom CurseForge API Key
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new("CurseForge API Key (Optional Override)")
                                                        .strong(),
                                                );
                                                ui.label(
                                                    RichText::new(
                                                        "Leave blank to use the built-in bundled key, or save a custom key into config/curseforge.json.",
                                                    )
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                                ui.add_space(6.0);
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.add(
                                                        TextEdit::singleline(
                                                            &mut self.cf_api_key_input,
                                                        )
                                                        .password(!self.cf_api_key_revealed)
                                                        .hint_text("$2a$10$...")
                                                        .desired_width(240.0),
                                                    );
                                                    let reveal_lbl = if self.cf_api_key_revealed {
                                                        "Hide"
                                                    } else {
                                                        "Reveal"
                                                    };
                                                    if ui.button(reveal_lbl).clicked() {
                                                        self.cf_api_key_revealed =
                                                            !self.cf_api_key_revealed;
                                                    }
                                                    if ui.button("Save Key").clicked() {
                                                        match save_custom_curseforge_api_key(
                                                            &self.paths.config_root,
                                                            &self.cf_api_key_input,
                                                        ) {
                                                            Ok(()) => {
                                                                self.notice_banner = Some((
                                                                    "Saved custom CurseForge API key."
                                                                        .to_string(),
                                                                    false,
                                                                ));
                                                            }
                                                            Err(e) => {
                                                                self.notice_banner = Some((
                                                                    format!(
                                                                        "Failed to save key: {e}"
                                                                    ),
                                                                    true,
                                                                ));
                                                            }
                                                        }
                                                    }
                                                    if !self.cf_api_key_input.trim().is_empty()
                                                        && ui.button("Use Bundled Key").clicked()
                                                    {
                                                        self.cf_api_key_input.clear();
                                                        let _ = save_custom_curseforge_api_key(
                                                            &self.paths.config_root,
                                                            "",
                                                        );
                                                        self.notice_banner = Some((
                                                            "Cleared custom CurseForge API key; reverted to default."
                                                                .to_string(),
                                                            false,
                                                        ));
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

                                        ui.add_space(8.0);

                                        // Card 3: Managed Content Failure Policies (Python ManagedContentPolicy parity)
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(
                                                        self.t("managed_content.launcher.title"),
                                                    )
                                                    .strong(),
                                                );
                                                ui.label(
                                                    RichText::new(
                                                        self.t("managed_content.launcher.detail"),
                                                    )
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                                ui.add_space(6.0);
                                                let block_txt = self
                                                    .t("managed_content.policy.block")
                                                    .to_string();
                                                let allow_txt = self
                                                    .t("managed_content.policy.allow")
                                                    .to_string();
                                                let warn_txt =
                                                    "Warn in logs & continue".to_string();
                                                let policy_label = |val: &str| match val
                                                    .trim()
                                                    .to_ascii_lowercase()
                                                    .as_str()
                                                {
                                                    "warn" | "ask" => warn_txt.as_str(),
                                                    "allow" => allow_txt.as_str(),
                                                    _ => block_txt.as_str(),
                                                };

                                                let mr_lbl = self
                                                    .t("managed_content.modrinth.label")
                                                    .to_string();
                                                let cur_mr = policy_label(
                                                    &self.prefs.modrinth_failure_policy,
                                                );
                                                let mr_w =
                                                    auto_combo_width(ui, cur_mr, 180.0, 280.0);
                                                egui::ComboBox::from_label(mr_lbl)
                                                    .selected_text(cur_mr)
                                                    .width(mr_w)
                                                    .truncate()
                                                    .show_ui(ui, |ui| {
                                                        for (v, lbl) in [
                                                            ("block", &block_txt),
                                                            ("warn", &warn_txt),
                                                            ("allow", &allow_txt),
                                                        ] {
                                                            if ui
                                                                .selectable_value(
                                                                    &mut self
                                                                        .prefs
                                                                        .modrinth_failure_policy,
                                                                    v.to_string(),
                                                                    lbl,
                                                                )
                                                                .changed()
                                                            {
                                                                prefs_dirty = true;
                                                            }
                                                        }
                                                    });

                                                ui.add_space(4.0);
                                                let cf_lbl = self
                                                    .t("managed_content.curseforge.label")
                                                    .to_string();
                                                let cur_cf = policy_label(
                                                    &self.prefs.curseforge_failure_policy,
                                                );
                                                let cf_w =
                                                    auto_combo_width(ui, cur_cf, 180.0, 280.0);
                                                egui::ComboBox::from_label(cf_lbl)
                                                    .selected_text(cur_cf)
                                                    .width(cf_w)
                                                    .truncate()
                                                    .show_ui(ui, |ui| {
                                                        for (v, lbl) in [
                                                            ("block", &block_txt),
                                                            ("warn", &warn_txt),
                                                            ("allow", &allow_txt),
                                                        ] {
                                                            if ui
                                                                .selectable_value(
                                                                    &mut self
                                                                        .prefs
                                                                        .curseforge_failure_policy,
                                                                    v.to_string(),
                                                                    lbl,
                                                                )
                                                                .changed()
                                                            {
                                                                prefs_dirty = true;
                                                            }
                                                        }
                                                    });
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
                                                if self.java_scanning {
                                                    ui.horizontal(|ui| {
                                                        ui.spinner();
                                                        ui.label(
                                                            RichText::new(
                                                                self.t("java.scan.progress"),
                                                            )
                                                            .color(p.accent),
                                                        );
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
                                                            format!(
                                                                "{} ({}-bit)",
                                                                diag.architecture, diag.bitness
                                                            )
                                                        };
                                                        ui.horizontal_wrapped(|ui| {
                                                            ui.label(
                                                                RichText::new(format!(
                                                                    "• Java {} ({}) · {} · {} · [{}] — {}",
                                                                    diag.major_version,
                                                                    diag.version_string,
                                                                    vendor,
                                                                    arch,
                                                                    diag.source,
                                                                    diag.executable.display()
                                                                ))
                                                                .monospace()
                                                                .size(11.0)
                                                                .color(if diag.valid {
                                                                    p.text_muted
                                                                } else {
                                                                    p.warning
                                                                }),
                                                            );
                                                            if ui.small_button("Open Folder").clicked()
                                                            {
                                                                let root = JavaDiagnosticsManager::java_root_dir(
                                                                    &diag.executable,
                                                                );
                                                                open_external(&root.display().to_string());
                                                            }
                                                            if diag.is_managed
                                                                && ui
                                                                    .small_button(
                                                                        RichText::new("Uninstall")
                                                                            .color(p.danger),
                                                                    )
                                                                    .clicked()
                                                            {
                                                                uninstall_java_exe = Some(
                                                                    diag.executable.clone(),
                                                                );
                                                            }
                                                        });
                                                    }
                                                } else if self.java_installations.is_empty() {
                                                    ui.label(
                                                        RichText::new(
                                                            self.t("launcher_settings.java.none"),
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
                                                let recovery_cnt =
                                                    JavaRecoveryDiagnostics::snapshot().len();
                                                if recovery_cnt > 0 {
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "Java auto-selection events recorded this session: {recovery_cnt}"
                                                        ))
                                                        .size(10.5)
                                                        .color(p.text_muted),
                                                    );
                                                }
                                                ui.add_space(6.0);
                                                ui.horizontal_wrapped(|ui| {
                                                    let scan_lbl = self
                                                        .t("Scan Java installations")
                                                        .to_string();
                                                    if ui.button(scan_lbl).clicked() {
                                                        scan_java = true;
                                                    }
                                                    for major in [8u32, 17, 21, 25] {
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

                                        ui.add_space(8.0);

                                        // Card 3: Forge/NeoForge Preflight Failure Policy
                                        Frame::new()
                                            .fill(p.surface)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(3))
                                            .inner_margin(egui::Margin::same(10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(
                                                    RichText::new(
                                                        self.t("forge_preflight.launcher.title"),
                                                    )
                                                    .strong(),
                                                );
                                                ui.label(
                                                    RichText::new(
                                                        self.t("forge_preflight.launcher.detail"),
                                                    )
                                                    .size(11.5)
                                                    .color(p.text_muted),
                                                );
                                                ui.add_space(6.0);
                                                let blk_lbl = self
                                                    .t("forge_preflight.policy.block")
                                                    .to_string();
                                                let alw_lbl = self
                                                    .t("forge_preflight.policy.allow")
                                                    .to_string();
                                                let wrn_lbl =
                                                    "Warn in logs & continue".to_string();
                                                let cur_fp = match self
                                                    .prefs
                                                    .forge_preflight_failure_policy
                                                    .trim()
                                                    .to_ascii_lowercase()
                                                    .as_str()
                                                {
                                                    "block" => blk_lbl.as_str(),
                                                    "allow" => alw_lbl.as_str(),
                                                    _ => wrn_lbl.as_str(),
                                                };
                                                let fp_w =
                                                    auto_combo_width(ui, cur_fp, 210.0, 320.0);
                                                egui::ComboBox::from_label(
                                                    self.t("instance_defaults.policy.forge"),
                                                )
                                                .selected_text(cur_fp)
                                                .width(fp_w)
                                                .truncate()
                                                .show_ui(ui, |ui| {
                                                    for (v, lbl) in [
                                                        ("warn", &wrn_lbl),
                                                        ("block", &blk_lbl),
                                                        ("allow", &alw_lbl),
                                                    ] {
                                                        if ui
                                                            .selectable_value(
                                                                &mut self
                                                                    .prefs
                                                                    .forge_preflight_failure_policy,
                                                                v.to_string(),
                                                                lbl,
                                                            )
                                                            .changed()
                                                        {
                                                            prefs_dirty = true;
                                                        }
                                                    }
                                                });
                                            });
                                    }
                                    LauncherSettingsTab::Appearance => {
                                        self.render_settings_appearance_tab(
                                            ui,
                                            vp_ctx,
                                            &mut prefs_dirty,
                                        );
                                    }
                                    LauncherSettingsTab::Storage => {
                                        self.render_settings_storage_tab(
                                            ui,
                                            vp_ctx,
                                            &mut prefs_dirty,
                                        );
                                    }
                                    LauncherSettingsTab::Updates => {
                                        self.render_settings_updates_tab(
                                            ui,
                                            vp_ctx,
                                            &mut prefs_dirty,
                                        );
                                    }
                                }
                            });
                            },
                        );
                    });
                });

            self.show_launcher_settings = open;
            if reload_langs {
                self.lang.reload(&self.paths);
                self.notice_banner = Some(("Reloaded language packs.".to_string(), false));
            }
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
            if let Some(exe) = uninstall_java_exe {
                self.uninstall_managed_java(exe, ctx);
            }
            if discord_changed {
                self.discord_rpc.configure(
                    self.prefs.discord_rpc_enabled,
                    self.prefs.discord_client_id.clone(),
                );
            }
            if open_first_run {
                self.open_first_run_wizard(ctx);
            }
            if trigger_storage_scan {
                self.start_storage_scan(ctx);
            }
            if let Some(major) = install_java_major {
                self.start_java_install(major, ctx);
            }
            if reset_defaults {
                self.prefs = LauncherPrefs::default();
                self.custom_accent_input = self.prefs.custom_accent_hex.clone();
                self.apply_current_theme(ctx);
                self.lang.set_language(&self.prefs.normalized_locale());
                self.create_show_snapshots = self.prefs.show_snapshots;
                self.discord_rpc.configure(
                    self.prefs.discord_rpc_enabled,
                    self.prefs.discord_client_id.clone(),
                );
                self.save_prefs();
                mcw_net::set_global_bandwidth_limit_mbps(self.prefs.download_limit_mbps);
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

                            ui.add_space(10.0);

                            // 5. Optional Modpack Files Picker (Modrinth / CurseForge / ATLauncher)
                            let opt_lbl = self.t("modrinth.modpack.optional_files").to_string();
                            ui.checkbox(&mut self.modpack_dialog.include_optional_files, &opt_lbl);
                            if self.modpack_dialog.loading_optional_files {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(
                                        RichText::new("Inspecting optional files...")
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                });
                            } else if self.modpack_dialog.include_optional_files
                                && !self.modpack_dialog.optional_files.is_empty()
                            {
                                ui.horizontal(|ui| {
                                    if ui.small_button("Select All").clicked() {
                                        self.modpack_dialog.selected_optional_ids = self
                                            .modpack_dialog
                                            .optional_files
                                            .iter()
                                            .map(|f| f.id.clone())
                                            .collect();
                                    }
                                    if ui.small_button("Select None").clicked() {
                                        self.modpack_dialog.selected_optional_ids.clear();
                                    }
                                });
                                ScrollArea::vertical()
                                    .id_salt("modpack_dialog_optional_files_scroll")
                                    .max_height(130.0)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        for opt in &self.modpack_dialog.optional_files {
                                            let mut checked = self
                                                .modpack_dialog
                                                .selected_optional_ids
                                                .contains(&opt.id);
                                            if ui.checkbox(&mut checked, &opt.name).changed() {
                                                if checked {
                                                    self.modpack_dialog
                                                        .selected_optional_ids
                                                        .insert(opt.id.clone());
                                                } else {
                                                    self.modpack_dialog
                                                        .selected_optional_ids
                                                        .remove(&opt.id);
                                                }
                                            }
                                        }
                                    });
                            }

                            ui.add_space(12.0);
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
            if selected_changed {
                self.load_atlauncher_optional_files(ctx);
            }
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
}
