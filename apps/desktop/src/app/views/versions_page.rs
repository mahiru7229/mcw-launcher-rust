use crate::app::platform::*;
use crate::app::*;

impl McwApp {
    pub(crate) fn render_versions_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.palette;

        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Versions").size(28.0).strong().color(p.text));
                ui.label(
                    RichText::new(
                        "Inspect cached Minecraft version JARs and browse official Mojang releases.",
                    )
                    .size(12.5)
                    .color(p.text_muted),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Import Archive...").clicked() {
                    self.create_tab = CreateInstanceTab::ImportArchive;
                    self.show_create_modal = true;
                }
                if ui
                    .add_enabled(!self.versions_loading && !self.cached_versions_loading, Button::new("Refresh"))
                    .clicked()
                {
                    self.fetch_versions(ctx);
                    self.load_cached_versions_async(ctx);
                }
                let snap_lbl = self.t("instance.create.show_non_release").to_string();
                if ui
                    .checkbox(&mut self.create_show_snapshots, snap_lbl)
                    .changed()
                {
                    self.versions_type_filter = if self.create_show_snapshots {
                        "all".to_string()
                    } else {
                        "release".to_string()
                    };
                }
            });
        });

        ui.add_space(12.0);

        let mut used_by: HashMap<String, Vec<String>> = HashMap::new();
        for inst in &self.instances {
            used_by
                .entry(inst.version_id.clone())
                .or_default()
                .push(inst.name.clone());
        }
        let cached_set: HashSet<String> = self
            .cached_versions
            .iter()
            .map(|c| c.version_id.clone())
            .collect();

        ScrollArea::vertical()
            .id_salt("versions_page_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Card 1: Cached versions on disk (cache/versions/)
                let total_cached_bytes: u64 = self.cached_versions.iter().map(|v| v.size_bytes).sum();
                let unused_ids: Vec<String> = self
                    .cached_versions
                    .iter()
                    .filter(|v| !used_by.contains_key(&v.version_id))
                    .map(|v| v.version_id.clone())
                    .collect();

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
                                    "Cached Versions ({}) · {}",
                                    self.cached_versions.len(),
                                    format_bytes(total_cached_bytes)
                                ))
                                .strong()
                                .size(15.5)
                                .color(p.text),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("Open cache/versions/").clicked() {
                                    let dir = self.paths.cache_root.join("versions");
                                    let _ = std::fs::create_dir_all(&dir);
                                    open_external(&dir.display().to_string());
                                }
                                let can_prune = !unused_ids.is_empty();
                                if ui
                                    .add_enabled(
                                        can_prune,
                                        Button::new(format!("Prune Unused ({})", unused_ids.len())),
                                    )
                                    .clicked()
                                {
                                    let mut reclaimed = 0u64;
                                    let mut removed = 0usize;
                                    for id in &unused_ids {
                                        if let Ok(bytes) =
                                            StorageCleanupService::delete_cached_version(&self.paths, id)
                                        {
                                            reclaimed += bytes;
                                            removed += 1;
                                        }
                                    }
                                    self.notice_banner = Some((
                                        format!(
                                            "Removed {removed} unused cached version(s), reclaimed {}.",
                                            format_bytes(reclaimed)
                                        ),
                                        false,
                                    ));
                                    self.load_cached_versions_async(ctx);
                                }
                            });
                        });
                        ui.add_space(8.0);

                        let mut delete_id: Option<String> = None;
                        let mut create_from_ver: Option<String> = None;

                        if self.cached_versions_loading && self.cached_versions.is_empty() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    RichText::new("Scanning cache/versions/...").color(p.text_muted),
                                );
                            });
                        } else if self.cached_versions.is_empty() {
                            ui.label(
                                RichText::new(
                                    "No cached versions found yet. Launching or creating an instance downloads its version metadata & JAR here.",
                                )
                                .color(p.text_muted),
                            );
                        } else {
                            for ver in &self.cached_versions {
                                let refs = used_by.get(&ver.version_id);
                                Frame::new()
                                    .fill(p.elevated)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .corner_radius(CornerRadius::same(10))
                                    .inner_margin(egui::Margin::symmetric(12, 8))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label(
                                                        RichText::new(&ver.version_id)
                                                            .monospace()
                                                            .strong()
                                                            .color(p.text),
                                                    );
                                                    let jar_lbl = if ver.has_jar {
                                                        "JAR ready"
                                                    } else {
                                                        "Metadata only"
                                                    };
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "· {jar_lbl} · {}",
                                                            format_bytes(ver.size_bytes)
                                                        ))
                                                        .size(11.5)
                                                        .color(p.text_muted),
                                                    );
                                                });
                                                if let Some(names) = refs {
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "Used by: {}",
                                                            names.join(", ")
                                                        ))
                                                        .size(11.0)
                                                        .color(p.accent),
                                                    );
                                                } else {
                                                    ui.label(
                                                        RichText::new("Unused by current instances")
                                                            .size(11.0)
                                                            .color(p.text_muted),
                                                    );
                                                }
                                            });
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    let can_delete = refs.is_none();
                                                    let del_resp = ui.add_enabled(
                                                        can_delete,
                                                        Button::new(
                                                            RichText::new("Delete").color(if can_delete {
                                                                p.danger
                                                            } else {
                                                                p.text_muted
                                                            }),
                                                        ),
                                                    );
                                                    if !can_delete {
                                                        del_resp.on_disabled_hover_text(
                                                            "Still referenced by one or more instances",
                                                        );
                                                    } else if del_resp.clicked() {
                                                        delete_id = Some(ver.version_id.clone());
                                                    }
                                                    if ui.button("New Instance").clicked() {
                                                        create_from_ver =
                                                            Some(ver.version_id.clone());
                                                    }
                                                },
                                            );
                                        });
                                    });
                                ui.add_space(6.0);
                            }
                        }

                        if let Some(id) = delete_id {
                            match StorageCleanupService::delete_cached_version(&self.paths, &id) {
                                Ok(bytes) => {
                                    self.notice_banner = Some((
                                        format!(
                                            "Deleted cached version '{id}' ({}).",
                                            format_bytes(bytes)
                                        ),
                                        false,
                                    ));
                                    self.load_cached_versions_async(ctx);
                                }
                                Err(e) => {
                                    self.notice_banner = Some((
                                        format!("Failed to delete version '{id}': {e}"),
                                        true,
                                    ));
                                }
                            }
                        }
                        if let Some(v) = create_from_ver {
                            self.open_create_instance_dialog();
                            self.create_version = v;
                        }
                    });

                ui.add_space(12.0);

                // Card 2: Mojang Manifest Versions Browser
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
                                    "Available Minecraft Versions ({})",
                                    self.available_versions.len()
                                ))
                                .strong()
                                .size(15.5)
                                .color(p.text),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let cur_type = match self.versions_type_filter.as_str() {
                                    "snapshot" => "Snapshots",
                                    "all" => "All Types",
                                    _ => "Releases",
                                };
                                let w = auto_combo_width(ui, cur_type, 110.0, 160.0);
                                egui::ComboBox::from_id_salt("versions_type_filter_combo")
                                    .selected_text(cur_type)
                                    .width(w)
                                    .truncate()
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut self.versions_type_filter,
                                            "release".to_string(),
                                            "Releases",
                                        );
                                        ui.selectable_value(
                                            &mut self.versions_type_filter,
                                            "snapshot".to_string(),
                                            "Snapshots",
                                        );
                                        ui.selectable_value(
                                            &mut self.versions_type_filter,
                                            "all".to_string(),
                                            "All Types",
                                        );
                                    });
                                ui.add(
                                    TextEdit::singleline(&mut self.versions_search)
                                        .hint_text("Search version (e.g. 1.21, 1.20.1, 25w)...")
                                        .desired_width(220.0),
                                );
                            });
                        });
                        ui.add_space(8.0);

                        if self.versions_loading && self.available_versions.is_empty() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    RichText::new("Fetching Mojang version manifest...")
                                        .color(p.text_muted),
                                );
                            });
                            return;
                        }

                        let needle = self.versions_search.trim().to_ascii_lowercase();
                        let type_filter = self.versions_type_filter.clone();
                        let matching: Vec<(String, String)> = self
                            .available_versions
                            .iter()
                            .filter(|(id, kind)| {
                                if type_filter != "all" && kind != &type_filter {
                                    return false;
                                }
                                needle.is_empty() || id.to_ascii_lowercase().contains(&needle)
                            })
                            .take(150)
                            .cloned()
                            .collect();

                        let mut pick_ver: Option<String> = None;
                        ScrollArea::vertical()
                            .id_salt("versions_manifest_scroll")
                            .max_height(320.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if matching.is_empty() {
                                    ui.label(
                                        RichText::new("No matching Minecraft versions.")
                                            .color(p.text_muted),
                                    );
                                } else {
                                    for (ver_id, ver_type) in &matching {
                                        Frame::new()
                                            .fill(p.elevated)
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .corner_radius(CornerRadius::same(8))
                                            .inner_margin(egui::Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        RichText::new(ver_id)
                                                            .monospace()
                                                            .strong()
                                                            .color(p.text),
                                                    );
                                                    ui.label(
                                                        RichText::new(ver_type)
                                                            .size(11.0)
                                                            .color(p.text_muted),
                                                    );
                                                    if cached_set.contains(ver_id) {
                                                        render_badge_pill(
                                                            ui, "Cached", true, true, p,
                                                        );
                                                    }
                                                    ui.with_layout(
                                                        Layout::right_to_left(Align::Center),
                                                        |ui| {
                                                            if ui.button("Create Instance").clicked()
                                                            {
                                                                pick_ver = Some(ver_id.clone());
                                                            }
                                                        },
                                                    );
                                                });
                                            });
                                        ui.add_space(4.0);
                                    }
                                }
                            });
                        if let Some(v) = pick_ver {
                            self.open_create_instance_dialog();
                            self.create_version = v;
                        }
                    });
            });
    }
}
