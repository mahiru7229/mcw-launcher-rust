use crate::app::controllers::apply_lan_agent_option;
use crate::app::platform::*;
use crate::app::*;

pub(crate) fn render_notice_banner_box(
    ui: &mut egui::Ui,
    p: ThemePalette,
    msg: &str,
    is_err: bool,
) -> bool {
    let mut dismiss = false;
    Frame::new()
        .fill(if is_err {
            Color32::from_rgba_unmultiplied(p.danger.r(), p.danger.g(), p.danger.b(), 26)
        } else {
            Color32::from_rgba_unmultiplied(p.accent.r(), p.accent.g(), p.accent.b(), 22)
        })
        .stroke(Stroke::new(
            1.0_f32,
            if is_err { p.danger } else { p.accent },
        ))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(msg).size(12.0).color(if is_err {
                    p.danger
                } else {
                    p.text
                }));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Dismiss").clicked() {
                        dismiss = true;
                    }
                });
            });
        });
    dismiss
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_in_app_modal(
    ctx: &egui::Context,
    id_salt: &str,
    title: &str,
    preferred_size: [f32; 2],
    min_size: [f32; 2],
    p: ThemePalette,
    notice: Option<&(String, bool)>,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> (bool, bool) {
    let screen = ctx.screen_rect();
    let max_w = (screen.width() - 32.0).max(280.0);
    let max_h = (screen.height() - 28.0).max(240.0);
    let notice_bump = if notice.is_some() { 46.0 } else { 0.0 };
    let modal_w = preferred_size[0].clamp(min_size[0].min(max_w), max_w);
    let modal_h = (preferred_size[1] + notice_bump).clamp(min_size[1].min(max_h), max_h);

    let mut keep_open = true;
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        keep_open = false;
    }

    let backdrop_id = egui::Id::new(id_salt).with("backdrop");
    let modal_id = egui::Id::new(id_salt);
    let mut dismiss_notice = false;

    egui::Area::new(backdrop_id)
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(150));
            if resp.clicked() {
                keep_open = false;
            }
        });

    egui::Window::new(title)
        .id(modal_id)
        .order(egui::Order::Foreground)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([modal_w, modal_h])
        .frame(
            Frame::new()
                .fill(p.canvas)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(16))
                .inner_margin(egui::Margin::same(16)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).size(17.5).strong().color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            Button::new(RichText::new("X").strong().size(12.0).color(p.text_muted))
                                .min_size(Vec2::splat(28.0))
                                .corner_radius(CornerRadius::same(8)),
                        )
                        .clicked()
                    {
                        keep_open = false;
                    }
                });
            });
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            if let Some((msg, is_err)) = notice {
                if render_notice_banner_box(ui, p, msg, *is_err) {
                    dismiss_notice = true;
                }
                ui.add_space(8.0);
            }

            add_contents(ui);
        });

    (keep_open, dismiss_notice)
}

impl McwApp {
    pub(crate) fn render_modals(&mut self, ctx: &egui::Context) {
        let p = self.palette;

        // 1. Create Instance (2 tabs: Custom | Modpacks) In-App Centered Modal
        if self.show_create_modal {
            let mut open = self.show_create_modal;
            let mut browse_modpacks_clicked = false;
            let mut open_py_import = false;
            let notice = self.modal_notice_for("mcw_modal_create_instance");
            let (keep_open, dismiss_notice) = render_in_app_modal(
                ctx,
                "mcw_modal_create_instance",
                "Create Instance",
                [600.0, 510.0],
                [460.0, 380.0],
                p,
                notice.as_ref(),
                |ui| {
                    let is_custom = matches!(self.create_tab, CreateInstanceTab::Custom);
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(egui::Margin::symmetric(5, 4))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (custom_tab, lbl) in [(true, "Custom"), (false, "Modpacks")] {
                                    let active = is_custom == custom_tab;
                                    let pill = Button::new(
                                        RichText::new(lbl).strong().size(12.5).color(if active {
                                            p.text
                                        } else {
                                            p.text_muted
                                        }),
                                    )
                                    .fill(if active {
                                        p.accent.gamma_multiply(0.32)
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .stroke(if active {
                                        Stroke::new(1.0_f32, p.accent)
                                    } else {
                                        Stroke::NONE
                                    })
                                    .corner_radius(CornerRadius::same(8));
                                    if ui.add(pill).clicked() {
                                        self.create_tab = if custom_tab {
                                            CreateInstanceTab::Custom
                                        } else {
                                            CreateInstanceTab::ImportArchive
                                        };
                                    }
                                }
                            });
                        });
                    ui.add_space(10.0);

                    if is_custom {
                        ui.label(self.t("common.name"));
                        ui.text_edit_singleline(&mut self.create_name);
                        ui.add_space(6.0);

                        ui.label(self.t("instance.minecraft_version"));
                        let snap_label = self.t("instance.create.show_non_release").to_string();
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
                                        format!("{ver_id} ({})", display_version_type(ver_type))
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
                        let create_loader_disp = display_loader_name(&self.create_loader);
                        egui::ComboBox::from_id_salt("create_mod_loader_combo")
                            .selected_text(&create_loader_disp)
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for loader in ["vanilla", "fabric", "quilt", "forge", "neoforge"] {
                                    ui.selectable_value(
                                        &mut self.create_loader,
                                        loader.to_string(),
                                        display_loader_name(loader),
                                    );
                                }
                            });

                        ui.add_space(6.0);
                        let lan_lbl = self.t("instance.create.enable_lan_agent").to_string();
                        ui.checkbox(&mut self.create_enable_lan_agent, &lan_lbl);

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
                                        inst_settings.min_memory = self.prefs.default_min_memory;
                                        inst_settings.max_memory = self.prefs.default_max_memory;
                                        inst_settings.width = self.prefs.default_width;
                                        inst_settings.height = self.prefs.default_height;
                                        inst_settings.fullscreen = self.prefs.default_fullscreen;
                                        if !self.prefs.default_java_path.trim().is_empty() {
                                            inst_settings.java_path = Some(PathBuf::from(
                                                self.prefs.default_java_path.trim(),
                                            ));
                                        }
                                        apply_lan_agent_option(
                                            &mut inst_settings,
                                            self.create_enable_lan_agent,
                                        );
                                        let _ = store
                                            .save_settings(&created.instance_id, &inst_settings);
                                        open = false;
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
                                open = false;
                            }
                        });
                    } else {
                        // Modpacks tab: Navigate to Mod & Modpacks -> Modpacks or Import Local Archive
                        Frame::new()
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(egui::Margin::same(12))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        RichText::new("Browse Online Modpacks")
                                            .strong()
                                            .color(p.text),
                                    );
                                    ui.label(
                                        RichText::new(
                                            "Open the Mod & Modpacks -> Modpacks page to browse Modrinth, CurseForge, FTB, ATLauncher, and Technic modpacks.",
                                        )
                                        .size(11.5)
                                        .color(p.text_muted),
                                    );
                                    ui.add_space(8.0);
                                    let browse_btn = Button::new(
                                        RichText::new("Browse Modpacks...")
                                            .color(p.accent_text)
                                            .strong(),
                                    )
                                    .fill(p.accent);
                                    if ui.add(browse_btn).clicked() {
                                        browse_modpacks_clicked = true;
                                        open = false;
                                    }
                                });

                        ui.add_space(10.0);

                        Frame::new()
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(egui::Margin::same(12))
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
                                    ui.horizontal_wrapped(|ui| {
                                        let te_w = (ui.available_width() - 96.0).clamp(180.0, 390.0);
                                        if ui
                                            .add(
                                                TextEdit::singleline(&mut self.modpack_path_input)
                                                    .hint_text(
                                                        "Select or paste path to .mcwpack / .mrpack / .zip...",
                                                    )
                                                    .desired_width(te_w),
                                            )
                                            .changed()
                                        {
                                            self.inspect_local_archive_optional_files();
                                        }
                                        if ui.button(self.t("instance_defaults.java.browse")).clicked() {
                                            if let Some(p) = pick_file_dialog(
                                                "Select Modpack / Instance Package",
                                                "MCW & Modpack Archives (*.mcwpack;*.mrpack;*.zip)|*.mcwpack;*.mrpack;*.zip|All Files (*.*)|*.*",
                                            ) {
                                                self.modpack_path_input = p.display().to_string();
                                                self.modpack_name_input.clear();
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
                                    ui.add_space(6.0);
                                    let lan_lbl =
                                        self.t("instance.create.enable_lan_agent").to_string();
                                    ui.checkbox(&mut self.archive_enable_lan_agent, &lan_lbl);
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
                                            .corner_radius(CornerRadius::same(10))
                                            .inner_margin(egui::Margin::same(10))
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
                                                    .max_height(130.0)
                                                    .auto_shrink([false, false])
                                                    .show(ui, |ui| {
                                                        for item in &self.modpack_manual_downloads {
                                                            ui.with_layout(
                                                                Layout::right_to_left(Align::Center),
                                                                |ui| {
                                                                    if ui.button("Open URL").clicked()
                                                                    {
                                                                        open_external(
                                                                            &item.website_url,
                                                                        );
                                                                    }
                                                                    ui.with_layout(
                                                                        Layout::top_down(
                                                                            Align::LEFT,
                                                                        ),
                                                                        |ui| {
                                                                            ui.set_width(
                                                                                ui.available_width(),
                                                                            );
                                                                            ui.add(
                                                                                egui::Label::new(
                                                                                    RichText::new(
                                                                                        &item.file_name,
                                                                                    )
                                                                                    .monospace()
                                                                                    .size(11.0),
                                                                                )
                                                                                .wrap(),
                                                                            );
                                                                        },
                                                                    );
                                                                },
                                                            );
                                                            ui.add_space(2.0);
                                                        }
                                                    });
                                            });
                                    }

                                    ui.add_space(10.0);
                                    ui.horizontal_wrapped(|ui| {
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
                                        if ui
                                            .button(self.t("python_import.open_button"))
                                            .clicked()
                                        {
                                            open_py_import = true;
                                            open = false;
                                        }
                                        if ui.button(self.t("common.close")).clicked() {
                                            open = false;
                                        }
                                    });
                                });
                    }
                },
            );
            if !keep_open {
                open = false;
            }
            if dismiss_notice {
                self.notice_banner = None;
            }
            if browse_modpacks_clicked {
                self.show_create_modal = false;
                self.open_content_browser_mode(BrowserCategory::Modpacks, None, true, ctx);
            } else if open_py_import {
                self.show_create_modal = false;
                self.open_python_import_window(ctx);
            } else if !open {
                self.show_create_modal = false;
            }
        }

        // 1a-0. Dedicated In-App Modals & Log OS Windows
        self.render_update_dialog(ctx);
        if !self.show_python_import_window {
            self.render_first_run_wizard(ctx);
        }
        self.render_repair_center_window(ctx);
        self.render_global_logs_window(ctx);
        self.render_about_window(ctx);
        self.render_lan_agent_log_window(ctx);
        self.render_java_manager_window(ctx);
        self.render_stage6_modals(ctx);

        // 1a. Modpack Install Configuration In-App Modal (Version + Beta/Alpha + RAM + Java + Open Web)
        self.render_modpack_install_dialog(ctx);

        // 1b. 3-Mode Export Modpack In-App Modal
        if self.show_export_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                let title = self.t("modpack_package.export.title").to_string();
                let mut trigger_export: Option<ModpackExportOptions> = None;
                let mut open = self.show_export_modal;
                let notice = self.modal_notice_for("mcw_modal_export_modpack");
                let (keep_open, dismiss_notice) = render_in_app_modal(
                    ctx,
                    "mcw_modal_export_modpack",
                    &format!("{title} - {}", inst.name),
                    [560.0, 470.0],
                    [420.0, 340.0],
                    p,
                    notice.as_ref(),
                    |ui| {
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

                                let origin = detect_export_origin(
                                    &self.paths.instance_dir(&inst.instance_id),
                                    &inst.icon_origin,
                                );
                                let allow_mrpack = origin == InstanceExportOrigin::Modrinth;
                                let allow_provider = matches!(
                                    origin,
                                    InstanceExportOrigin::CurseForge
                                        | InstanceExportOrigin::OtherProvider
                                );

                                let mrpack_lbl =
                                    self.t("modpack_package.export.mrpack").to_string();
                                let provider_lbl = if origin == InstanceExportOrigin::CurseForge {
                                    "CurseForge Modpack (.zip)".to_string()
                                } else {
                                    self.t("modpack_package.export.provider_profile")
                                        .to_string()
                                };
                                let provider_help = if origin == InstanceExportOrigin::CurseForge {
                                    "Exports a CurseForge-compatible manifest archive (.zip) referencing CurseForge project and file IDs plus overrides."
                                        .to_string()
                                } else {
                                    self.t("modpack_package.export.provider_profile_help")
                                        .to_string()
                                };
                                let portable_lbl =
                                    self.t("modpack_package.export.portable").to_string();

                                // Coerce invalid mode for this instance origin
                                match self.export_mode {
                                    ModpackExportMode::Mrpack if !allow_mrpack => {
                                        self.export_mode = if allow_provider {
                                            ModpackExportMode::ProviderProfile
                                        } else {
                                            ModpackExportMode::Portable {
                                                full: self.export_portable_full,
                                            }
                                        };
                                    }
                                    ModpackExportMode::ProviderProfile if !allow_provider => {
                                        self.export_mode = if allow_mrpack {
                                            ModpackExportMode::Mrpack
                                        } else {
                                            ModpackExportMode::Portable {
                                                full: self.export_portable_full,
                                            }
                                        };
                                    }
                                    _ => {}
                                }

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
                                        if allow_mrpack
                                            && ui.selectable_label(is_mrpack, &mrpack_lbl).clicked()
                                        {
                                            self.export_mode = ModpackExportMode::Mrpack;
                                        }
                                        if allow_provider
                                            && ui
                                                .selectable_label(is_provider, &provider_lbl)
                                                .clicked()
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

                                let help_text = if is_mrpack {
                                    self.t("modpack_package.export.mrpack_help").to_string()
                                } else if is_provider {
                                    provider_help
                                } else {
                                    self.t("modpack_package.export.portable_help").to_string()
                                };
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(help_text)
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
                                if is_portable {
                                    ui.add_space(4.0);
                                    ui.checkbox(
                                        &mut self.export_disable_repair_and_updates,
                                        "Disable Mod Repair & Mod Updates (Recommended for modpacks)",
                                    )
                                    .on_hover_text(
                                        "When imported from .mcwpack, per-mod updates and dependency repair checks default to disabled so curated mods stay intact.",
                                    );
                                    ui.checkbox(
                                        &mut self.export_disable_launch_checking,
                                        "Disable pre-launch dependency checking (Recommended for modpacks)",
                                    )
                                    .on_hover_text(
                                        "When imported from .mcwpack, skips pre-launch dependency warning dialogs across all modloaders (findings remain logged silently).",
                                    );
                                }

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
                                            disable_mod_repair_and_updates: if is_portable {
                                                self.export_disable_repair_and_updates
                                            } else {
                                                true
                                            },
                                            disable_launch_checking: if is_portable {
                                                self.export_disable_launch_checking
                                            } else {
                                                true
                                            },
                                        });
                                    }
                                    if ui.button(self.t("common.cancel")).clicked() {
                                        open = false;
                                    }
                                });
                            });
                    },
                );
                if !keep_open {
                    open = false;
                }
                if dismiss_notice {
                    self.notice_banner = None;
                }
                self.show_export_modal = open;
                if let Some(opts) = trigger_export {
                    self.show_export_modal = false;
                    self.start_instance_export(inst.instance_id, opts, ctx);
                }
            }
        }

        // 1c. Mod / Content / Modpack Version Picker In-App Modal (Step 1: select file version)
        if self.version_picker_open {
            let mut open = self.version_picker_open;
            let mut chosen: Option<ModVersionChoice> = None;
            let mut filter_changed = false;
            let title = format!("Select Version - {}", self.version_picker_title);

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

            let notice = self.modal_notice_for("mcw_modal_version_picker");
            let (keep_open, dismiss_notice) = render_in_app_modal(
                ctx,
                "mcw_modal_version_picker",
                &title,
                [760.0, 560.0],
                [520.0, 380.0],
                p,
                notice.as_ref(),
                |ui| {
                    // Header: Thumbnail + Mod Title + Open on Web
                    ui.horizontal(|ui| {
                        self.render_thumbnail(ui, self.version_picker_icon.as_deref(), 44.0);
                        ui.add_space(8.0);
                        let web_reserve = if self.version_picker_website_url.is_some() {
                            126.0
                        } else {
                            12.0
                        };
                        ui.vertical(|ui| {
                            ui.set_max_width((ui.available_width() - web_reserve).max(160.0));
                            ui.label(
                                RichText::new(&self.version_picker_title)
                                    .strong()
                                    .size(15.5)
                                    .color(p.text),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(if self.version_picker_is_pack {
                                        "Choose a modpack version to download and install."
                                    } else if self.version_picker_global_mode {
                                        "Select a file version first, then click Install to pick or create a matching instance."
                                    } else {
                                        "Filter and select a compatible file version to install."
                                    })
                                    .size(11.5)
                                    .color(p.text_muted),
                                )
                                .wrap(),
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

                    // Row 1: Release channels + auto-sized MC Version & Loader dropdowns (wrapped safely)
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
                        let mc_disp = if self.version_picker_mc_filter == "all" {
                            "All Versions".to_string()
                        } else {
                            self.version_picker_mc_filter.clone()
                        };
                        let mc_w = auto_combo_width(ui, &mc_disp, 80.0, 145.0);
                        egui::ComboBox::from_id_salt("mod_vp_mc_filter_combo")
                            .selected_text(&mc_disp)
                            .width(mc_w)
                            .truncate()
                            .show_ui(ui, |ui| {
                                for gv in &all_mc_versions {
                                    let disp = if gv == "all" {
                                        "All Versions".to_string()
                                    } else {
                                        gv.clone()
                                    };
                                    if ui
                                        .selectable_value(
                                            &mut self.version_picker_mc_filter,
                                            gv.clone(),
                                            disp,
                                        )
                                        .changed()
                                    {
                                        filter_changed = true;
                                    }
                                }
                            });
                        ui.label("Loader:");
                        let loader_disp = display_loader_name(&self.version_picker_loader_filter);
                        let loader_w = auto_combo_width(ui, &loader_disp, 84.0, 130.0);
                        egui::ComboBox::from_id_salt("mod_vp_loader_filter_combo")
                            .selected_text(&loader_disp)
                            .width(loader_w)
                            .truncate()
                            .show_ui(ui, |ui| {
                                for l in ["all", "fabric", "forge", "neoforge", "quilt"] {
                                    if ui
                                        .selectable_value(
                                            &mut self.version_picker_loader_filter,
                                            l.to_string(),
                                            display_loader_name(l),
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
                        // Row 2: Responsive Auto-Sized Selected Version Dropdown (bounded within modal width)
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
                                let loaders = item
                                    .loaders
                                    .iter()
                                    .map(|l| display_loader_name(l))
                                    .collect::<Vec<_>>()
                                    .join(", ");
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
                            let max_combo_w = (ui.available_width() - 12.0).clamp(160.0, 560.0);
                            let combo_w = auto_combo_width(ui, &selected_label, 180.0, max_combo_w);
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
                                        let loaders = item
                                            .loaders
                                            .iter()
                                            .map(|l| display_loader_name(l))
                                            .collect::<Vec<_>>()
                                            .join(", ");
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
                                let inner_card_w = (ui.available_width() - 14.0).max(220.0);
                                for item in &filtered_items {
                                    let is_selected = item.id == self.version_picker_selected_id;
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
                                        .corner_radius(CornerRadius::same(10))
                                        .inner_margin(egui::Margin::symmetric(10, 8))
                                        .show(ui, |ui| {
                                            ui.set_width(inner_card_w);
                                            ui.horizontal(|ui| {
                                                let text_max_w = (inner_card_w - 104.0).max(140.0);
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
                                                        ui.add(
                                                            egui::Label::new(
                                                                RichText::new(&item.title).strong(),
                                                            )
                                                            .wrap(),
                                                        );
                                                    });
                                                    let gvs = item
                                                        .game_versions
                                                        .iter()
                                                        .take(4)
                                                        .cloned()
                                                        .collect::<Vec<_>>()
                                                        .join(", ");
                                                    let loaders = item
                                                        .loaders
                                                        .iter()
                                                        .map(|l| display_loader_name(l))
                                                        .collect::<Vec<_>>()
                                                        .join(", ");
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
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(meta)
                                                                .monospace()
                                                                .size(10.5)
                                                                .color(p.text_muted),
                                                        )
                                                        .wrap(),
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
                },
            );
            if !keep_open {
                open = false;
            }
            if dismiss_notice {
                self.notice_banner = None;
            }
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
                } else if self.version_picker_global_mode {
                    self.open_install_target_dialog(choice, ctx);
                } else {
                    self.start_direct_mod_file_install(choice, ctx);
                }
            } else if !self.version_picker_open {
                self.clear_remote_thumbs_if_idle();
            }
        }

        // 2. Rename Instance In-App Modal (Centered)
        if self.show_rename_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                let mut open = self.show_rename_modal;
                let notice = self.modal_notice_for("mcw_modal_rename_instance");
                let (keep_open, dismiss_notice) = render_in_app_modal(
                    ctx,
                    "mcw_modal_rename_instance",
                    &format!("Rename Instance - {}", inst.name),
                    [440.0, 200.0],
                    [360.0, 170.0],
                    p,
                    notice.as_ref(),
                    |ui| {
                        ui.label(format!("New name for '{}':", inst.name));
                        ui.text_edit_singleline(&mut self.rename_input);
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            let ren_btn = Button::new(
                                RichText::new(self.t("instance.rename"))
                                    .color(p.accent_text)
                                    .strong(),
                            )
                            .fill(p.accent);
                            if ui.add(ren_btn).clicked() {
                                let store = InstanceStore::new(&self.paths);
                                match store.rename_instance(&inst.instance_id, &self.rename_input) {
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
                            if ui.button(self.t("common.cancel")).clicked() {
                                open = false;
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
                self.show_rename_modal = open;
            }
        }

        // 2a. Edit Group & Tags In-App Modal (Centered, opened from Actions -> Edit Group & Tags...)
        if self.show_group_tags_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                let mut open = self.show_group_tags_modal;
                let notice = self.modal_notice_for("mcw_modal_edit_group_tags");
                let (keep_open, dismiss_notice) = render_in_app_modal(
                    ctx,
                    "mcw_modal_edit_group_tags",
                    &format!("Edit Group & Tags - {}", inst.name),
                    [480.0, 260.0],
                    [380.0, 220.0],
                    p,
                    notice.as_ref(),
                    |ui| {
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
                            if ui.button(self.t("common.cancel")).clicked() {
                                open = false;
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
                self.show_group_tags_modal = open;
            }
        }

        // 2b. Clone / Duplicate Instance In-App Modal (Centered)
        if self.show_clone_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                let mut open = self.show_clone_modal;
                let clone_title = self.t("workspace.clone.title").to_string();
                let include_saves_lbl = self.t("workspace.clone.include_saves").to_string();
                let clone_act_lbl = self.t("workspace.clone.action").to_string();
                let cancel_lbl = self.t("common.cancel").to_string();
                let notice = self.modal_notice_for("mcw_modal_clone_instance");
                let (keep_open, dismiss_notice) = render_in_app_modal(
                    ctx,
                    "mcw_modal_clone_instance",
                    &format!("{clone_title} - {}", inst.name),
                    [460.0, 235.0],
                    [360.0, 200.0],
                    p,
                    notice.as_ref(),
                    |ui| {
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
                    },
                );
                if !keep_open {
                    open = false;
                }
                if dismiss_notice {
                    self.notice_banner = None;
                }
                self.show_clone_modal = open;
            }
        }

        // 3. Delete Instance Confirmation In-App Modal (Centered, Data Safety Gate)
        if self.show_delete_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                let mut open = self.show_delete_modal;
                let notice = self.modal_notice_for("mcw_modal_delete_instance");
                let (keep_open, dismiss_notice) = render_in_app_modal(
                    ctx,
                    "mcw_modal_delete_instance",
                    &format!("Delete Instance - {}", inst.name),
                    [480.0, 225.0],
                    [380.0, 190.0],
                    p,
                    notice.as_ref(),
                    |ui| {
                        ui.label(
                            RichText::new(format!("Permanently delete '{}'?", inst.name))
                                .strong()
                                .color(p.danger),
                        );
                        ui.add(
                            egui::Label::new(
                                "This will permanently delete the instance folder including all saves, mods, and configs inside .minecraft/.",
                            )
                            .wrap(),
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
                            if ui.button(self.t("common.cancel")).clicked() {
                                open = false;
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
                self.show_delete_modal = open;
            }
        }

        // 4. Account Manager In-App Modal (Centered)
        if self.show_account_modal {
            let mut open = self.show_account_modal;
            let notice = self.modal_notice_for("mcw_modal_accounts");
            let (keep_open, dismiss_notice) = render_in_app_modal(
                ctx,
                "mcw_modal_accounts",
                "Accounts",
                [560.0, 480.0],
                [400.0, 340.0],
                p,
                notice.as_ref(),
                |ui| {
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
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "Browser (PKCE) always asks which Microsoft account to use.",
                                )
                                .size(11.5)
                                .color(p.text_muted),
                            )
                            .wrap(),
                        );
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
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
                                let cancel_btn =
                                    Button::new(RichText::new("Cancel").color(p.danger))
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
                            if self.ms_login_in_progress
                                && self.ms_login_mode == Some("device_code")
                            {
                                let cancel_btn =
                                    Button::new(RichText::new("Cancel").color(p.danger))
                                        .stroke(Stroke::new(1.0_f32, p.danger));
                                if ui.add(cancel_btn).clicked() {
                                    self.cancel_microsoft_login();
                                }
                            }
                        });

                        if self.ms_login_in_progress && self.ms_login_mode == Some("pkce") {
                            ui.add_space(4.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(
                                        "Waiting for Microsoft sign-in in your browser (listening on localhost:8400)...",
                                    )
                                    .size(11.5)
                                    .color(p.accent),
                                )
                                .wrap(),
                            );
                        }

                        if let Some(ref dc) = self.ms_device_code {
                            ui.add_space(6.0);
                            Frame::new()
                                .fill(p.elevated)
                                .stroke(Stroke::new(1.0_f32, p.accent))
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(egui::Margin::same(12))
                                .show(ui, |ui| {
                                    ui.label("Enter this code in your browser:");
                                    ui.horizontal_wrapped(|ui| {
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
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&dc.verification_uri)
                                                .monospace()
                                                .size(11.0)
                                                .color(p.text_muted),
                                        )
                                        .wrap(),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(
                                                "Tip: Microsoft Device Login reuses your active browser session. To pick a different account, paste the URL into an InPrivate/Incognito window or use Browser (PKCE).",
                                            )
                                            .size(11.0)
                                            .color(p.text_muted),
                                        )
                                        .wrap(),
                                    );
                                });
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        ui.label(RichText::new("Add Offline Account").strong());
                        ui.horizontal_wrapped(|ui| {
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
                        if ui.button(self.t("common.close")).clicked() {
                            open = false;
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
            if !open {
                if self.ms_login_in_progress {
                    self.cancel_microsoft_login();
                }
                self.show_account_modal = false;
            }
        }

        // 5. Restore Backup Confirmation In-App Modal (Centered)
        if let Some(backup_id) = self.pending_restore_backup_id.clone() {
            let mut open = true;
            let mut confirm_restore = false;
            let notice = self.modal_notice_for("mcw_modal_restore_backup");
            let (keep_open, dismiss_notice) = render_in_app_modal(
                ctx,
                "mcw_modal_restore_backup",
                &format!("Restore Backup - {backup_id}"),
                [480.0, 225.0],
                [380.0, 190.0],
                p,
                notice.as_ref(),
                |ui| {
                    ui.label(
                        RichText::new(format!("Restore backup '{backup_id}'?"))
                            .strong()
                            .color(p.warning),
                    );
                    ui.add(
                        egui::Label::new(
                            "Current .minecraft/ contents will be replaced using a safe staging & rollback swap (original data is preserved on rollback if restore fails).",
                        )
                        .wrap(),
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
                        if ui.button(self.t("common.cancel")).clicked() {
                            open = false;
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
            if !open {
                self.pending_restore_backup_id = None;
            }
            if confirm_restore {
                self.start_backup_restore(backup_id, ctx);
            }
        }

        // 5b. Delete World Confirmation In-App Modal (Centered, Data Safety Gate)
        if let Some((inst_id, folder_name)) = self.pending_delete_world.clone() {
            let mut open = true;
            let mut confirm_delete = false;
            let notice = self.modal_notice_for("mcw_modal_delete_world");
            let (keep_open, dismiss_notice) = render_in_app_modal(
                ctx,
                "mcw_modal_delete_world",
                &format!("Delete World - {folder_name}"),
                [460.0, 210.0],
                [360.0, 175.0],
                p,
                notice.as_ref(),
                |ui| {
                    ui.label(
                        RichText::new(format!("Permanently delete world '{folder_name}'?"))
                            .strong()
                            .color(p.danger),
                    );
                    ui.add(
                        egui::Label::new(
                            "This will permanently remove the world save folder from saves/.",
                        )
                        .wrap(),
                    );
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
                        if ui.button(self.t("common.cancel")).clicked() {
                            open = false;
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

        // 6. Launcher Settings In-App Centered Modal (moved to settings_modal.rs)
        self.render_launcher_settings_modal(ctx);

        // 7. Dedicated Per-Instance Manual Downloads Isolated OS Windows (CurseForge opt-out mods)
        let open_ids = self.open_manual_modals.clone();
        let open_manual_count = open_ids.len();
        let mut to_close: Vec<String> = Vec::new();
        let mut to_scan: Vec<String> = Vec::new();
        let mut launch_anyway_id: Option<String> = None;

        for (manual_idx, inst_id) in open_ids.into_iter().enumerate() {
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
            let win_title = self.lang.tr_fmt(
                "manual_downloads.window_title",
                &[("name", &inst_name), ("count", &items.len().to_string())],
            );
            let inst_hdr = self
                .lang
                .tr_fmt("manual_downloads.instance_label", &[("name", &inst_name)]);
            let disabled_warn = self.t("manual_downloads.author_disabled").to_string();
            let instructions_lbl = self.t("manual_downloads.instructions").to_string();
            let folder_lbl = self.t("manual_downloads.folder_label").to_string();
            let browse_lbl = self.t("instance_defaults.java.browse").to_string();
            let scan_lbl = self.t("manual_downloads.scan_now").to_string();
            let auto_scan_lbl = self.t("manual_downloads.auto_scan").to_string();
            let watching_lbl = self.t("manual_downloads.watching").to_string();
            let open_next_5_lbl = self.t("manual_downloads.open_next_5").to_string();
            let open_mods_lbl = self.t("manual_downloads.open_mods_folder").to_string();
            let all_imported_lbl = self.t("manual_downloads.all_imported").to_string();
            let open_page_lbl = self.t("manual_downloads.open_page").to_string();
            let close_lbl = self.t("common.close").to_string();
            let launch_anyway_lbl = self.t("manual_downloads.launch_anyway").to_string();
            let launch_anyway_hint = self.t("manual_downloads.launch_anyway_hint").to_string();
            let notice = if manual_idx + 1 == open_manual_count {
                self.modal_notice_for("mcw_modal_manual_downloads")
            } else {
                None
            };

            let (keep_open, dismiss_notice) = render_in_app_modal(
                ctx,
                &format!("mcw_modal_manual_dl_{inst_id}"),
                &win_title,
                [700.0, 560.0],
                [480.0, 380.0],
                p,
                notice.as_ref(),
                |ui| {
                    egui::TopBottomPanel::bottom(egui::Id::new((
                        "manual_downloads_footer",
                        &inst_id,
                    )))
                    .frame(
                        Frame::new()
                            .fill(p.canvas)
                            .inner_margin(egui::Margin::symmetric(0, 8)),
                    )
                    .show_inside(ui, |ui| {
                        ui.separator();
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui.button(&close_lbl).clicked() {
                                to_close.push(inst_id.clone());
                                win_open = false;
                            }
                            if !items.is_empty() {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .button(RichText::new(&launch_anyway_lbl).color(p.warning))
                                        .on_hover_text(&launch_anyway_hint)
                                        .clicked()
                                    {
                                        launch_anyway_id = Some(inst_id.clone());
                                    }
                                });
                            }
                        });
                    });

                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&inst_hdr).strong().size(13.5).color(p.accent));
                        ui.label(
                            RichText::new(format!("({inst_id})"))
                                .monospace()
                                .size(11.0)
                                .color(p.text_muted),
                        );
                    });
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(RichText::new(&disabled_warn).strong().color(p.warning))
                            .wrap(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(&instructions_lbl)
                                .size(11.5)
                                .color(p.text_muted),
                        )
                        .wrap(),
                    );
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label(&folder_lbl);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let scan_btn =
                                Button::new(RichText::new(&scan_lbl).color(p.accent_text).strong())
                                    .fill(p.accent);
                            if ui.add(scan_btn).clicked() {
                                to_scan.push(inst_id.clone());
                            }
                            if ui.button(&browse_lbl).clicked() {
                                if let Some(dir) = pick_folder_dialog(&folder_lbl) {
                                    self.manual_downloads_dir_input = dir.display().to_string();
                                    to_scan.push(inst_id.clone());
                                }
                            }
                            let edit_w = ui.available_width().max(120.0);
                            ui.add(
                                TextEdit::singleline(&mut self.manual_downloads_dir_input)
                                    .desired_width(edit_w),
                            );
                        });
                    });

                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut self.manual_auto_scan, &auto_scan_lbl);
                        if self.manual_auto_scan && !items.is_empty() {
                            ui.label(
                                RichText::new(format!("- {watching_lbl}"))
                                    .size(11.0)
                                    .color(p.accent),
                            );
                        }
                    });
                    if let Some(ref status) = status_text {
                        ui.add(
                            egui::Label::new(RichText::new(status).size(11.5).color(p.text_muted))
                                .wrap(),
                        );
                    }

                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(!items.is_empty(), Button::new(&open_next_5_lbl))
                            .clicked()
                        {
                            for item in items.iter().take(5) {
                                open_external(&item.website_url);
                            }
                        }
                        if ui.button(&open_mods_lbl).clicked() {
                            let mods_dir = self.paths.minecraft_dir(&inst_id).join("mods");
                            let _ = std::fs::create_dir_all(&mods_dir);
                            open_external(&mods_dir.display().to_string());
                        }
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    if items.is_empty() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&all_imported_lbl).strong().color(p.accent),
                            )
                            .wrap(),
                        );
                    } else {
                        ScrollArea::vertical()
                            .id_salt(("manual_downloads_modal_scroll", &inst_id))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let row_w = (ui.available_width() - 4.0).max(200.0);
                                for item in &items {
                                    Frame::new()
                                        .fill(p.elevated)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .corner_radius(CornerRadius::same(10))
                                        .inner_margin(egui::Margin::symmetric(10, 8))
                                        .show(ui, |ui| {
                                            ui.set_width(row_w);
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if ui.button(&open_page_lbl).clicked() {
                                                        open_external(&item.website_url);
                                                    }
                                                    ui.with_layout(
                                                        Layout::top_down(Align::LEFT),
                                                        |ui| {
                                                            ui.set_width(ui.available_width());
                                                            ui.add(
                                                                egui::Label::new(
                                                                    RichText::new(&item.file_name)
                                                                        .monospace()
                                                                        .strong(),
                                                                )
                                                                .wrap(),
                                                            );
                                                            ui.add(
                                                                egui::Label::new(
                                                                    RichText::new(format!(
                                                                        "Mod #{} - File #{}",
                                                                        item.mod_id, item.file_id
                                                                    ))
                                                                    .size(11.0)
                                                                    .color(p.text_muted),
                                                                )
                                                                .wrap(),
                                                            );
                                                        },
                                                    );
                                                },
                                            );
                                        });
                                    ui.add_space(4.0);
                                }
                            });
                    }
                },
            );
            if !keep_open {
                win_open = false;
            }
            if dismiss_notice {
                self.notice_banner = None;
            }

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

        self.render_install_target_dialog(ctx);
        self.render_duplicate_mod_modal(ctx);
        self.render_junk_cleanup_modal(ctx);
        self.render_storage_relocated_modal(ctx);
        self.render_screenshot_preview_modal(ctx);
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
            "{} ({})",
            self.modpack_dialog.pack_title, self.modpack_dialog.provider_label
        );

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
        let notice = self.modal_notice_for("mcw_modal_modpack_install");

        let (keep_open, dismiss_notice) = render_in_app_modal(
            ctx,
            "mcw_modal_modpack_install",
            &title,
            [600.0, 590.0],
            [440.0, 420.0],
            p,
            notice.as_ref(),
            |ui| {
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
                                RichText::new(format!("({} available)", filtered_options.len()))
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
                                RichText::new("No downloadable versions found.").color(p.warning),
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
                                    "(PC RAM: {} MB - Loader: {})",
                                    self.pc_ram_mb,
                                    display_loader_name(active_loader)
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
                                if self.modpack_dialog.max_ram_mb < self.modpack_dialog.min_ram_mb {
                                    self.modpack_dialog.max_ram_mb = self.modpack_dialog.min_ram_mb;
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
                                if self.modpack_dialog.min_ram_mb > self.modpack_dialog.max_ram_mb {
                                    self.modpack_dialog.min_ram_mb = self.modpack_dialog.max_ram_mb;
                                }
                            }
                        });

                        ui.add_space(8.0);

                        // 3b. JVM Preset (Default, Aikar, ZGC, Shenandoah)
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("JVM Preset:").strong().size(12.5));
                            let cur_preset_lbl = self
                                .t(self.modpack_dialog.jvm_preset.i18n_key())
                                .to_string();
                            let pw = auto_combo_width(ui, &cur_preset_lbl, 160.0, 260.0);
                            egui::ComboBox::from_id_salt("modpack_install_jvm_preset_combo")
                                .selected_text(&cur_preset_lbl)
                                .width(pw)
                                .truncate()
                                .show_ui(ui, |ui| {
                                    for preset in [
                                        JvmPresetId::Default,
                                        JvmPresetId::Aikar,
                                        JvmPresetId::Zgc,
                                        JvmPresetId::Shenandoah,
                                    ] {
                                        let lbl = self.t(preset.i18n_key()).to_string();
                                        ui.selectable_value(
                                            &mut self.modpack_dialog.jvm_preset,
                                            preset,
                                            lbl,
                                        );
                                    }
                                });
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
                                        .desired_width((ui.available_width() - 120.0).max(160.0)),
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

                        // 5. LAN Agent & Optional Modpack Files Picker
                        let lan_lbl = self.t("instance.create.enable_lan_agent").to_string();
                        ui.checkbox(&mut self.modpack_dialog.enable_lan_agent, &lan_lbl);
                        ui.add_space(6.0);

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
            },
        );
        if !keep_open {
            open = false;
        }
        if dismiss_notice {
            self.notice_banner = None;
        }

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
        } else if !self.modpack_dialog.open {
            self.clear_remote_thumbs_if_idle();
        }
    }

    /// Step-2 In-App Modal for Content Browser installs:
    /// pick an exact-compatible existing instance or create a new matching instance.
    pub(crate) fn render_install_target_dialog(&mut self, ctx: &egui::Context) {
        if !self.install_target_open {
            return;
        }
        let Some(choice) = self.install_target_choice.clone() else {
            self.install_target_open = false;
            return;
        };

        let p = self.palette;
        let mut open = self.install_target_open;
        let mut trigger_install = false;
        let mut go_back = false;
        let mut load_worlds_for: Option<String> = None;

        let title = format!("Install Destination - {}", self.install_target_title);

        let compat =
            self.exact_compatible_instances_for_choice(&choice, self.install_target_content_kind);
        let active_existing = compat
            .iter()
            .find(|i| i.instance_id == self.install_target_instance_id)
            .or_else(|| compat.first())
            .cloned();
        let active_existing_id = active_existing.as_ref().map(|i| i.instance_id.clone());

        let target_worlds: Vec<WorldInfo> = if !self.install_target_create_new
            && self.install_target_content_kind == Some(ContentKind::DataPack)
        {
            active_existing_id
                .as_ref()
                .and_then(|id| self.worlds_cache.get(id))
                .cloned()
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Available MC versions & Loaders for Create New Instance mode
        let mut mc_options: Vec<String> = choice
            .game_versions
            .iter()
            .filter(|v| !v.trim().is_empty())
            .cloned()
            .collect();
        if mc_options.is_empty() {
            mc_options = self
                .available_versions
                .iter()
                .filter(|(_, vt)| vt == "release")
                .take(40)
                .map(|(vid, _)| vid.clone())
                .collect();
        }
        if !mc_options.is_empty()
            && !mc_options
                .iter()
                .any(|v| v.eq_ignore_ascii_case(&self.install_target_new_mc_version))
        {
            self.install_target_new_mc_version = mc_options[0].clone();
        }

        let allowed_loaders: Vec<&'static str> = if self.install_target_content_kind.is_some() {
            vec!["vanilla", "fabric", "quilt", "forge", "neoforge"]
        } else {
            let matching: Vec<&'static str> = ["fabric", "quilt", "forge", "neoforge"]
                .into_iter()
                .filter(|l| choice.loaders.iter().any(|cl| cl.eq_ignore_ascii_case(l)))
                .collect();
            if matching.is_empty() {
                vec!["fabric", "quilt", "forge", "neoforge"]
            } else {
                matching
            }
        };
        if !allowed_loaders
            .iter()
            .any(|l| l.eq_ignore_ascii_case(&self.install_target_new_loader))
        {
            self.install_target_new_loader = allowed_loaders[0].to_string();
        }

        let can_install = if self.install_target_create_new {
            !self.install_target_new_name.trim().is_empty()
                && !self.install_target_new_mc_version.trim().is_empty()
        } else {
            active_existing_id.is_some()
        };

        let cancel_lbl = self.t("common.cancel").to_string();
        let badge = match choice.channel {
            ModpackChannel::Release => "[R]",
            ModpackChannel::Beta => "[B]",
            ModpackChannel::Alpha => "[A]",
        };
        let loaders_text = if choice.loaders.is_empty() {
            String::new()
        } else {
            choice
                .loaders
                .iter()
                .map(|l| display_loader_name(l))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let gvs_text = choice
            .game_versions
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let version_summary = if loaders_text.is_empty() {
            format!(
                "{badge} {} ({}) - MC {} - {}",
                choice.title,
                choice.version_number,
                gvs_text,
                format_bytes(choice.file_size)
            )
        } else {
            format!(
                "{badge} {} ({}) - {} - MC {} - {}",
                choice.title,
                choice.version_number,
                loaders_text,
                gvs_text,
                format_bytes(choice.file_size)
            )
        };

        let notice = self.modal_notice_for("mcw_modal_install_target");
        let (keep_open, dismiss_notice) = render_in_app_modal(
            ctx,
            "mcw_modal_install_target",
            &title,
            [620.0, 490.0],
            [460.0, 360.0],
            p,
            notice.as_ref(),
            |ui| {
                // Header
                ui.horizontal(|ui| {
                    self.render_thumbnail(ui, self.install_target_icon.as_deref(), 40.0);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.set_max_width((ui.available_width() - 8.0).max(160.0));
                        ui.label(
                            RichText::new(&self.install_target_title)
                                .strong()
                                .size(15.5)
                                .color(p.text),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(&version_summary)
                                    .monospace()
                                    .size(11.0)
                                    .color(p.accent),
                            )
                            .wrap(),
                        );
                    });
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                egui::TopBottomPanel::bottom("install_target_footer_panel")
                    .frame(
                        Frame::new()
                            .fill(p.canvas)
                            .inner_margin(egui::Margin::symmetric(0, 6)),
                    )
                    .show_inside(ui, |ui| {
                        ui.separator();
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let action_lbl = if self.install_target_create_new {
                                "Create & Install"
                            } else {
                                "Install to Instance"
                            };
                            let btn = Button::new(
                                RichText::new(action_lbl).color(p.accent_text).strong(),
                            )
                            .fill(p.accent);
                            if ui.add_enabled(can_install, btn).clicked() {
                                trigger_install = true;
                            }
                            if ui.button("Back").clicked() {
                                open = false;
                                go_back = true;
                            }
                            if ui.button(&cancel_lbl).clicked() {
                                open = false;
                            }
                        });
                    });

                ScrollArea::vertical()
                        .id_salt("install_target_body_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            Frame::new()
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(egui::Margin::same(12))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(RichText::new("Install Mode:").strong());
                                        let mode_lbl = if self.install_target_create_new {
                                            "Create New Instance"
                                        } else {
                                            "Existing Instance"
                                        };
                                        let mw = auto_combo_width(ui, mode_lbl, 160.0, 230.0);
                                        egui::ComboBox::from_id_salt("it_dest_mode_combo")
                                            .selected_text(mode_lbl)
                                            .width(mw)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut self.install_target_create_new,
                                                    false,
                                                    "Existing Instance",
                                                );
                                                ui.selectable_value(
                                                    &mut self.install_target_create_new,
                                                    true,
                                                    "Create New Instance",
                                                );
                                            });
                                    });

                                    ui.add_space(10.0);

                                    if !self.install_target_create_new {
                                        if compat.is_empty() {
                                            let msg = if self.install_target_content_kind.is_none() {
                                                "No existing instance matches this file's exact Minecraft version and mod loader."
                                            } else {
                                                "No existing instance matches this file's exact Minecraft version."
                                            };
                                            ui.label(RichText::new(msg).color(p.warning));
                                            ui.add_space(6.0);
                                            if ui.button("Switch to Create New Instance").clicked() {
                                                self.install_target_create_new = true;
                                            }
                                        } else {
                                            let cur_inst_lbl = active_existing
                                                .as_ref()
                                                .map(|i| {
                                                    format!(
                                                        "{} (MC {} - {})",
                                                        i.name,
                                                        i.version_id,
                                                        display_loader_name(&i.mod_loader.name)
                                                    )
                                                })
                                                .unwrap_or_else(|| "Select instance...".to_string());
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label("Target Instance:");
                                                let iw = auto_combo_width(
                                                    ui,
                                                    &cur_inst_lbl,
                                                    210.0,
                                                    380.0,
                                                );
                                                egui::ComboBox::from_id_salt(
                                                    "it_target_inst_combo",
                                                )
                                                .selected_text(&cur_inst_lbl)
                                                .width(iw)
                                                .truncate()
                                                .show_ui(ui, |ui| {
                                                    for inst in &compat {
                                                        let sel = active_existing_id.as_deref()
                                                            == Some(inst.instance_id.as_str());
                                                        let lbl = format!(
                                                            "{} (MC {} - {})",
                                                            inst.name,
                                                            inst.version_id,
                                                            display_loader_name(
                                                                &inst.mod_loader.name
                                                            )
                                                        );
                                                        if ui.selectable_label(sel, lbl).clicked()
                                                            && !sel
                                                        {
                                                            self.install_target_instance_id =
                                                                inst.instance_id.clone();
                                                            self.install_target_datapack_world
                                                                .clear();
                                                            if self.install_target_content_kind
                                                                == Some(ContentKind::DataPack)
                                                            {
                                                                load_worlds_for = Some(
                                                                    inst.instance_id.clone(),
                                                                );
                                                            }
                                                        }
                                                    }
                                                });
                                            });

                                            if self.install_target_content_kind
                                                == Some(ContentKind::DataPack)
                                                && active_existing_id.is_some()
                                            {
                                                ui.add_space(6.0);
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label("Data Pack Target:");
                                                    let w_lbl = if self
                                                        .install_target_datapack_world
                                                        .trim()
                                                        .is_empty()
                                                    {
                                                        "Global (.minecraft/datapacks)"
                                                            .to_string()
                                                    } else {
                                                        format!(
                                                            "World: {}",
                                                            self.install_target_datapack_world
                                                        )
                                                    };
                                                    let ww =
                                                        auto_combo_width(ui, &w_lbl, 200.0, 340.0);
                                                    egui::ComboBox::from_id_salt(
                                                        "it_datapack_world_combo",
                                                    )
                                                    .selected_text(&w_lbl)
                                                    .width(ww)
                                                    .truncate()
                                                    .show_ui(ui, |ui| {
                                                        ui.selectable_value(
                                                            &mut self
                                                                .install_target_datapack_world,
                                                            String::new(),
                                                            "Global (.minecraft/datapacks)",
                                                        );
                                                        for w in &target_worlds {
                                                            let lbl = format!(
                                                                "{} ({})",
                                                                w.display_name, w.folder_name
                                                            );
                                                            ui.selectable_value(
                                                                &mut self
                                                                    .install_target_datapack_world,
                                                                w.folder_name.clone(),
                                                                lbl,
                                                            );
                                                        }
                                                    });
                                                });
                                            }
                                        }
                                    } else {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label("Instance Name:");
                                            ui.add(
                                                TextEdit::singleline(
                                                    &mut self.install_target_new_name,
                                                )
                                                .hint_text("Instance name")
                                                .desired_width(250.0),
                                            );
                                        });
                                        ui.add_space(6.0);
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label("MC Version:");
                                            let mc_disp = if self
                                                .install_target_new_mc_version
                                                .trim()
                                                .is_empty()
                                            {
                                                "Select version...".to_string()
                                            } else {
                                                self.install_target_new_mc_version.clone()
                                            };
                                            let mw = auto_combo_width(ui, &mc_disp, 110.0, 175.0);
                                            egui::ComboBox::from_id_salt("it_new_mc_ver_combo")
                                                .selected_text(&mc_disp)
                                                .width(mw)
                                                .show_ui(ui, |ui| {
                                                    for gv in &mc_options {
                                                        ui.selectable_value(
                                                            &mut self.install_target_new_mc_version,
                                                            gv.clone(),
                                                            gv,
                                                        );
                                                    }
                                                });

                                            ui.label("Loader:");
                                            let l_disp = display_loader_name(
                                                &self.install_target_new_loader,
                                            );
                                            let lw = auto_combo_width(ui, &l_disp, 100.0, 155.0);
                                            egui::ComboBox::from_id_salt("it_new_loader_combo")
                                                .selected_text(&l_disp)
                                                .width(lw)
                                                .show_ui(ui, |ui| {
                                                    for &l in &allowed_loaders {
                                                        ui.selectable_value(
                                                            &mut self.install_target_new_loader,
                                                            l.to_string(),
                                                            display_loader_name(l),
                                                        );
                                                    }
                                                });
                                        });
                                        ui.add_space(6.0);
                                        let lan_lbl = self
                                            .t("instance.create.enable_lan_agent")
                                            .to_string();
                                        ui.checkbox(
                                            &mut self.install_target_enable_lan_agent,
                                            &lan_lbl,
                                        );
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(
                                                "Uses default Launcher Settings (RAM & Java)",
                                            )
                                            .size(11.0)
                                            .color(p.text_muted),
                                        );
                                    }
                                });
                        });
            },
        );
        if !keep_open {
            open = false;
        }
        if dismiss_notice {
            self.notice_banner = None;
        }

        self.install_target_open = open;
        if let Some(wid) = load_worlds_for {
            self.load_worlds_async(&wid, ctx, false);
        }
        if go_back {
            self.install_target_choice = None;
            self.version_picker_open = true;
            return;
        }
        if trigger_install {
            self.install_target_open = false;
            if let Some(choice_to_install) = self.install_target_choice.take() {
                let effective_id =
                    active_existing_id.unwrap_or_else(|| self.install_target_instance_id.clone());
                let dp_world = (!self.install_target_datapack_world.trim().is_empty())
                    .then(|| self.install_target_datapack_world.clone());
                self.install_content_choice_to_target(
                    choice_to_install,
                    self.install_target_create_new,
                    effective_id,
                    self.install_target_new_name.clone(),
                    self.install_target_new_mc_version.clone(),
                    self.install_target_new_loader.clone(),
                    dp_world,
                    ctx,
                );
            }
        } else if !self.install_target_open {
            self.install_target_choice = None;
            self.clear_remote_thumbs_if_idle();
        }
    }

    pub(crate) fn render_duplicate_mod_modal(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.pending_duplicate_mod_prompts.first().cloned() else {
            return;
        };
        let p = self.palette;
        let mut open = true;
        let mut chosen_action: Option<u8> = None;
        let notice = self.modal_notice_for("mcw_modal_duplicate_mod");

        let (keep_open, dismiss_notice) = render_in_app_modal(
            ctx,
            "mcw_modal_duplicate_mod",
            "Duplicate Mod Detected",
            [510.0, 290.0],
            [400.0, 240.0],
            p,
            notice.as_ref(),
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&prompt.match_reason)
                            .strong()
                            .size(13.5)
                            .color(p.warning),
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(format!("Target Instance: {}", prompt.instance_name))
                                .size(12.0)
                                .color(p.text),
                        );
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "Existing file: {}",
                                    prompt.existing_file_name
                                ))
                                .monospace()
                                .size(11.5)
                                .color(p.text_muted),
                            )
                            .wrap(),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "New file:      {}",
                                    prompt.incoming_file_name
                                ))
                                .monospace()
                                .size(11.5)
                                .color(p.accent),
                            )
                            .wrap(),
                        );
                    });
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    let overwrite_btn = Button::new(
                        RichText::new("Overwrite Existing")
                            .color(Color32::BLACK)
                            .strong(),
                    )
                    .fill(p.warning);
                    if ui.add(overwrite_btn).clicked() {
                        chosen_action = Some(1);
                    }
                    let keep_btn = Button::new(
                        RichText::new("Keep Both (Rename Copy)")
                            .color(p.accent_text)
                            .strong(),
                    )
                    .fill(p.accent);
                    if ui.add(keep_btn).clicked() {
                        chosen_action = Some(2);
                    }
                    if ui.button(self.t("common.cancel")).clicked() {
                        chosen_action = Some(0);
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

        if let Some(act) = chosen_action {
            self.confirm_duplicate_mod_action(act, ctx);
        } else if !open {
            self.confirm_duplicate_mod_action(0, ctx);
        }
    }

    pub(crate) fn render_junk_cleanup_modal(&mut self, ctx: &egui::Context) {
        if !self.show_junk_cleanup_modal {
            return;
        }
        let Some(report) = self.junk_scan_report.clone() else {
            return;
        };
        let p = self.palette;
        let mut open = self.show_junk_cleanup_modal;
        let mut clean_now = false;
        let mut snooze_later = false;
        let mut dont_show = !self.prefs.auto_cleanup_reminders;
        let mut selected_bytes = 0u64;
        if self.junk_clean_temp {
            selected_bytes += report.temp_staging.total_bytes;
        }
        if self.junk_clean_logs {
            selected_bytes += report.old_logs.total_bytes;
        }
        if self.junk_clean_caches {
            selected_bytes += report.disposable_and_unused.total_bytes;
        }

        let retention_days = self.prefs.junk_retention_days;
        let notice = self.modal_notice_for("mcw_modal_junk_cleanup");
        let (keep_open, dismiss_notice) = render_in_app_modal(
            ctx,
            "mcw_modal_junk_cleanup",
            "Junk Files & Cache Cleanup",
            [520.0, 390.0],
            [400.0, 300.0],
            p,
            notice.as_ref(),
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "Detected {} across temporary downloads, aged logs (>{} days), and expired caches. Worlds, saves, mods, configs, accounts, backups, and Java runtimes are never touched.",
                            format_bytes(report.total_bytes),
                            retention_days
                        ))
                        .size(12.0)
                        .color(p.text_muted),
                    )
                    .wrap(),
                );
                ui.add_space(10.0);

                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.checkbox(
                            &mut self.junk_clean_temp,
                            format!(
                                "Temp & incomplete downloads — {} item(s) ({})",
                                report.temp_staging.file_count,
                                format_bytes(report.temp_staging.total_bytes)
                            ),
                        );
                        ui.checkbox(
                            &mut self.junk_clean_logs,
                            format!(
                                "Old logs & crash reports — {} item(s) ({})",
                                report.old_logs.file_count,
                                format_bytes(report.old_logs.total_bytes)
                            ),
                        );
                        ui.checkbox(
                            &mut self.junk_clean_caches,
                            format!(
                                "Expired cache & unused versions — {} item(s) ({})",
                                report.disposable_and_unused.file_count,
                                format_bytes(report.disposable_and_unused.total_bytes)
                            ),
                        );
                    });

                ui.add_space(8.0);
                if ui
                    .checkbox(
                        &mut dont_show,
                        "Don't show automatic cleanup reminders (can re-enable in Settings → Storage)",
                    )
                    .changed()
                {
                    self.prefs.auto_cleanup_reminders = !dont_show;
                    self.save_prefs();
                }

                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    let can_clean = selected_bytes > 0
                        && !self.junk_cleaning
                        && (self.junk_clean_temp || self.junk_clean_logs || self.junk_clean_caches);
                    let clean_lbl = if self.junk_cleaning {
                        "Cleaning...".to_string()
                    } else {
                        format!("Clean Selected ({})", format_bytes(selected_bytes))
                    };
                    let clean_btn =
                        Button::new(RichText::new(clean_lbl).color(p.accent_text).strong())
                            .fill(p.accent);
                    if ui.add_enabled(can_clean, clean_btn).clicked() {
                        clean_now = true;
                    }
                    if ui
                        .add_enabled(!self.junk_cleaning, Button::new("Later (Remind in 3 days)"))
                        .clicked()
                    {
                        snooze_later = true;
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
        if clean_now {
            self.start_junk_clean(
                self.junk_clean_temp,
                self.junk_clean_logs,
                self.junk_clean_caches,
                ctx,
            );
        } else if snooze_later || !open {
            self.prefs.junk_reminder_snooze_until = unix_now_secs().saturating_add(3 * 86_400);
            self.save_prefs();
            self.show_junk_cleanup_modal = false;
        }
    }

    pub(crate) fn render_storage_relocated_modal(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.relocated_storage_prompt.clone() else {
            return;
        };
        let p = self.palette;
        let notice = self.modal_notice_for("mcw_modal_storage_relocated");
        let elapsed = prompt.opened_at.elapsed().as_secs_f32();
        let remaining_secs = (5.0 - elapsed).ceil().max(0.0) as u32;
        if remaining_secs > 0 {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        let buttons_unlocked = remaining_secs == 0 && !self.relocating_storage_busy;

        let mut action: Option<u8> = None; // 0 = Backup&Move, 1 = DirectMove, 2 = Cancel/Manual, 3 = Empty OK
        let (_keep_open, dismiss_notice) = render_in_app_modal(
            ctx,
            "mcw_modal_storage_relocated",
            "Install Location Not Writable (Safe Storage Active)",
            [620.0, 410.0],
            [460.0, 310.0],
            p,
            notice.as_ref(),
            |ui| {
                ui.label(
                    RichText::new(
                        "The launch folder does not allow writing files (Windows permission / C: protection, os error 5).",
                    )
                    .color(p.text),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("Previous folder: {}", prompt.old_root.display()))
                        .monospace()
                        .size(11.0)
                        .color(p.text_muted),
                );
                ui.label(
                    RichText::new(format!("New safe folder: {}", prompt.new_root.display()))
                        .monospace()
                        .size(11.0)
                        .color(p.accent),
                );
                ui.add_space(8.0);
                if prompt.has_user_data {
                    ui.label(
                        RichText::new(
                            "Existing instances, saves/worlds, accounts, and config were found in the previous folder. Choose how to migrate your worlds and data:",
                        )
                        .color(p.warning),
                    );
                } else {
                    ui.label(
                        RichText::new(
                            "No existing data was found in the previous folder. Future downloads and instances will be stored in the new safe folder.",
                        )
                        .color(p.text_muted),
                    );
                }
                if let Some(ref err) = prompt.error {
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("Error: {err}")).color(p.danger));
                }
                if self.relocating_storage_busy {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Backing up / moving data and worlds...").color(p.accent),
                        );
                    });
                } else if remaining_secs > 0 {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!(
                            "Actions unlock in {remaining_secs}s so you can verify paths safely..."
                        ))
                        .size(11.5)
                        .color(p.warning),
                    );
                }

                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    if prompt.has_user_data {
                        let rec_btn = Button::new(
                            RichText::new("Backup & Move (Recommended)")
                                .strong()
                                .color(p.accent_text),
                        )
                        .fill(p.accent);
                        if ui.add_enabled(buttons_unlocked, rec_btn).clicked() {
                            action = Some(0);
                        }
                        if ui
                            .add_enabled(
                                buttons_unlocked,
                                Button::new("Move Directly (Faster, risky)"),
                            )
                            .clicked()
                        {
                            action = Some(1);
                        }
                        if ui
                            .add_enabled(buttons_unlocked, Button::new("Cancel (Move Manually)"))
                            .clicked()
                        {
                            action = Some(2);
                        }
                        if ui.button("Open Old Folder").clicked() {
                            open_external(&prompt.old_root.display().to_string());
                        }
                        if ui.button("Open New Folder").clicked() {
                            open_external(&prompt.new_root.display().to_string());
                        }
                    } else {
                        let ok_btn = Button::new(RichText::new("OK").strong().color(p.accent_text))
                            .fill(p.accent);
                        if ui.add_enabled(buttons_unlocked, ok_btn).clicked() {
                            action = Some(3);
                        }
                    }
                });
            },
        );
        if dismiss_notice {
            self.notice_banner = None;
        }
        match action {
            Some(0) => self.start_desktop_storage_relocation(true, ctx),
            Some(1) => self.start_desktop_storage_relocation(false, ctx),
            Some(2) => {
                open_external(&prompt.old_root.display().to_string());
                open_external(&prompt.new_root.display().to_string());
                self.relocated_storage_prompt = None;
                self.notice_banner = Some((
                    format!(
                        "Using safe folder {}. Move your instances/saves manually if needed.",
                        prompt.new_root.display()
                    ),
                    false,
                ));
            }
            Some(3) => {
                self.relocated_storage_prompt = None;
            }
            _ => {}
        }
    }

    pub(crate) fn render_screenshot_preview_modal(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot_preview_path.clone() else {
            return;
        };
        if !path.is_file() {
            self.screenshot_preview_path = None;
            self.screenshot_preview_texture = None;
            return;
        }
        let needs_load = self
            .screenshot_preview_texture
            .as_ref()
            .is_none_or(|(p, _)| p != &path);
        if needs_load {
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let rgba = img.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    let color_img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                    let tex = ctx.load_texture(
                        format!("mcw_shot_preview_{}", path.display()),
                        color_img,
                        egui::TextureOptions::LINEAR,
                    );
                    self.screenshot_preview_texture = Some((path.clone(), tex));
                }
            }
        }

        let p = self.palette;
        let notice = self.modal_notice_for("mcw_modal_screenshot_preview");
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Screenshot")
            .to_string();
        let mut close = false;
        let mut rename_clicked = false;
        let mut delete_clicked = false;

        let (keep_open, dismiss_notice) = render_in_app_modal(
            ctx,
            "mcw_modal_screenshot_preview",
            &format!("Screenshot Preview - {file_name}"),
            [780.0, 580.0],
            [520.0, 400.0],
            p,
            notice.as_ref(),
            |ui| {
                if let Some((_, ref tex)) = self.screenshot_preview_texture {
                    let max_size = Vec2::new(ui.available_width().max(300.0), 390.0);
                    ui.centered_and_justified(|ui| {
                        ui.add(
                            egui::Image::new(tex)
                                .fit_to_exact_size(max_size)
                                .corner_radius(CornerRadius::same(8)),
                        );
                    });
                } else {
                    ui.label(RichText::new("Unable to decode image preview.").color(p.warning));
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Rename:");
                    let enter = ui
                        .add(
                            TextEdit::singleline(&mut self.screenshot_preview_rename_input)
                                .desired_width(220.0),
                        )
                        .lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("Apply Name").clicked() || enter {
                        rename_clicked = true;
                    }
                    if ui.button("Open File").clicked() {
                        open_external(&path.display().to_string());
                    }
                    if ui.button("Copy Path").clicked() {
                        ctx.copy_text(path.display().to_string());
                        self.notice_banner = Some(("Copied screenshot path.".to_string(), false));
                    }
                    if ui.button(RichText::new("Delete").color(p.danger)).clicked() {
                        delete_clicked = true;
                    }
                    if ui.button(self.t("common.close")).clicked() {
                        close = true;
                    }
                });
            },
        );
        if dismiss_notice {
            self.notice_banner = None;
        }
        if !keep_open || close {
            self.screenshot_preview_path = None;
            self.screenshot_preview_texture = None;
            return;
        }
        if rename_clicked {
            let candidate = self.screenshot_preview_rename_input.clone();
            match ScreenshotManager::rename_screenshot(&path, &candidate) {
                Ok(new_path) => {
                    self.screenshot_preview_path = Some(new_path.clone());
                    self.screenshot_preview_texture = None;
                    if let Some(inst_id) = self.selected_instance_id.clone() {
                        self.screenshots_cache.remove(&inst_id);
                        self.load_screenshots_async(&inst_id, ctx, true);
                    }
                    self.notice_banner = Some((
                        format!(
                            "Renamed screenshot to {}.",
                            new_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("file")
                        ),
                        false,
                    ));
                }
                Err(e) => {
                    self.notice_banner = Some((format!("Rename failed: {e}"), true));
                }
            }
        } else if delete_clicked {
            match ScreenshotManager::delete_screenshot(&path) {
                Ok(()) => {
                    self.screenshot_preview_path = None;
                    self.screenshot_preview_texture = None;
                    if let Some(inst_id) = self.selected_instance_id.clone() {
                        self.screenshots_cache.remove(&inst_id);
                        self.load_screenshots_async(&inst_id, ctx, true);
                    }
                    self.notice_banner = Some(("Deleted screenshot.".to_string(), false));
                }
                Err(e) => {
                    self.notice_banner = Some((format!("Delete failed: {e}"), true));
                }
            }
        }
    }
}
