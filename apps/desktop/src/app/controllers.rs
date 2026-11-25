use super::platform::*;
use super::*;
use std::fs;

impl McwApp {
    pub(crate) fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.lang.tr(key)
    }

    pub(crate) fn save_prefs(&mut self) {
        if self.prefs.theme.trim().is_empty() {
            self.prefs.theme = match self.theme_mode {
                ThemeMode::Dark => "dark".to_string(),
                ThemeMode::Light => "light".to_string(),
            };
        }
        self.prefs.motion_mode = self.motion_mode.as_str().to_string();
        self.prefs.language = self.lang.current_locale().to_string();
        self.prefs.save(&self.paths);
    }

    pub(crate) fn reload_instances(&mut self) {
        let store = InstanceStore::new(&self.paths);
        self.instances = store.list_instances().unwrap_or_default();
        self.security_assessments.clear();
        if let Some(ref sel) = self.selected_instance_id {
            if !self.instances.iter().any(|i| &i.instance_id == sel) {
                self.selected_instance_id = None;
            }
        }
        if self.selected_instance_id.is_none() {
            self.selected_instance_id = self.instances.first().map(|i| i.instance_id.clone());
        }
        self.refresh_pending_manual_counts();
        self.sync_selected_from_cache();
    }

    pub(crate) fn assess_instance_security(
        &mut self,
        inst: &Instance,
    ) -> InstanceSecurityAssessment {
        if let Some(cached) = self.security_assessments.get(&inst.instance_id) {
            return cached.clone();
        }
        let assessment = compute_local_instance_security(
            &self.paths,
            inst,
            self.prefs.mitigate_minecraft2shell,
            &self.resolved_security_policy,
        );
        self.security_assessments
            .insert(inst.instance_id.clone(), assessment.clone());
        assessment
    }

    pub(crate) fn start_refresh_security_policy(&mut self, ctx: &egui::Context) {
        if self.security_policy_refreshing {
            return;
        }
        self.security_policy_refreshing = true;
        let paths = self.paths.clone();
        let client = self.download_client.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.runtime.spawn(async move {
            let policy = SecurityPolicyService::refresh_from_remote(
                &paths,
                &client,
                REMOTE_SECURITY_POLICY_URL,
            )
            .await;
            let _ = tx.send(AppEvent::SecurityPolicyRefreshed(policy));
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_sync_security_libraries(&mut self, inst: &Instance, ctx: &egui::Context) {
        if self.security_sync_busy {
            return;
        }
        self.security_sync_busy = true;
        let paths = self.paths.clone();
        let client = self.download_client.clone();
        let inst = inst.clone();
        let mitigate = self.prefs.mitigate_minecraft2shell;
        let policy = self.resolved_security_policy.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.runtime.spawn(async move {
            let outcome = match MojangLibrarySync::sync_freetype_for_version(
                &paths,
                &client,
                &inst.version_id,
                &policy,
            )
            .await
            {
                Ok(out) => out,
                Err(err) => LibrarySyncOutcome::RolledBack {
                    version_id: inst.version_id.clone(),
                    reason: format!("Library sync failed: {err}"),
                },
            };
            let assessment = Some(compute_local_instance_security(
                &paths, &inst, mitigate, &policy,
            ));
            let _ = tx.send(AppEvent::SecurityLibrarySyncFinished {
                instance_id: inst.instance_id,
                outcome,
                assessment,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn refresh_pending_manual_counts(&mut self) {
        self.pending_manual_counts.clear();
        for inst in &self.instances {
            let pending =
                ModpackInstaller::load_pending_manual_downloads(&self.paths, &inst.instance_id);
            if !pending.is_empty() {
                self.pending_manual_counts
                    .insert(inst.instance_id.clone(), pending.len());
            }
        }
    }

    pub(crate) fn open_manual_downloads_for(&mut self, instance_id: &str, _ctx: &egui::Context) {
        let pending = ModpackInstaller::load_pending_manual_downloads(&self.paths, instance_id);
        if pending.is_empty() {
            self.pending_manual_counts.remove(instance_id);
            self.manual_downloads_cache.remove(instance_id);
            self.open_manual_modals.retain(|id| id != instance_id);
            return;
        }
        self.pending_manual_counts
            .insert(instance_id.to_string(), pending.len());
        self.manual_downloads_cache
            .insert(instance_id.to_string(), pending);
        self.manual_scan_status.remove(instance_id);
        self.last_manual_scan_at = None;
        if !self.open_manual_modals.iter().any(|id| id == instance_id) {
            self.open_manual_modals.push(instance_id.to_string());
        }
    }

    pub(crate) fn run_manual_downloads_scan(&mut self, inst_id: &str, manual_trigger: bool) {
        let current_items = self
            .manual_downloads_cache
            .get(inst_id)
            .cloned()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                ModpackInstaller::load_pending_manual_downloads(&self.paths, inst_id)
            });
        if current_items.is_empty() {
            self.pending_manual_counts.remove(inst_id);
            self.manual_downloads_cache.remove(inst_id);
            self.open_manual_modals.retain(|id| id != inst_id);
            return;
        }

        let dl_dir = PathBuf::from(self.manual_downloads_dir_input.trim());
        self.last_manual_scan_at = Some(Instant::now());

        match ModpackInstaller::scan_and_import_manual_downloads(
            &self.paths,
            inst_id,
            &current_items,
            &dl_dir,
        ) {
            Ok((resolved, remaining)) => {
                if remaining.is_empty() {
                    self.pending_manual_counts.remove(inst_id);
                    self.manual_downloads_cache.remove(inst_id);
                    self.open_manual_modals.retain(|id| id != inst_id);
                } else {
                    self.pending_manual_counts
                        .insert(inst_id.to_string(), remaining.len());
                    self.manual_downloads_cache
                        .insert(inst_id.to_string(), remaining.clone());
                }

                if resolved > 0 {
                    self.mods_cache.remove(inst_id);
                    if self.selected_instance_id.as_deref() == Some(inst_id) {
                        self.sync_selected_from_cache();
                    }
                    let msg = if remaining.is_empty() {
                        format!(
                            "Imported {resolved} mod(s) for '{inst_id}'. All manual downloads complete - modpack is ready!"
                        )
                    } else {
                        format!(
                            "Imported {resolved} mod(s) ({} remaining).",
                            remaining.len()
                        )
                    };
                    self.manual_scan_status
                        .insert(inst_id.to_string(), msg.clone());
                    self.notice_banner = Some((msg, false));
                } else if manual_trigger {
                    let msg = format!(
                        "No new matching .jar files found in {} ({} still missing).",
                        dl_dir.display(),
                        remaining.len()
                    );
                    self.manual_scan_status
                        .insert(inst_id.to_string(), msg.clone());
                    self.notice_banner = Some((msg, false));
                }
            }
            Err(e) => {
                let msg = format!("Scan Downloads failed: {e}");
                self.manual_scan_status
                    .insert(inst_id.to_string(), msg.clone());
                if manual_trigger {
                    self.notice_banner = Some((msg, true));
                }
            }
        }
    }

    /// Lightweight, instantaneous instance switch: NEVER scans `mods/` JARs or `backups/` on disk.
    /// Reads only cached data (and tiny `settings.json` once per instance).
    pub(crate) fn sync_selected_from_cache(&mut self) {
        let Some(ref id) = self.selected_instance_id else {
            self.selected_settings = None;
            self.selected_mods.clear();
            self.selected_backups.clear();
            self.selected_worlds.clear();
            self.selected_screenshots.clear();
            self.selected_content_packs.clear();
            self.selected_servers.clear();
            return;
        };
        if let Some(cached) = self.settings_cache.get(id) {
            self.selected_settings = Some(cached.clone());
        } else {
            let store = InstanceStore::new(&self.paths);
            if let Ok((_, s)) = store.get_instance(id) {
                self.settings_cache.insert(id.clone(), s.clone());
                self.selected_settings = Some(s);
            } else {
                self.selected_settings = None;
            }
        }
        if let Some(inst) = self.instances.iter().find(|i| &i.instance_id == id) {
            if inst.mod_loader.is_vanilla()
                && matches!(self.active_tab, DetailTab::Mods | DetailTab::Shaders)
            {
                self.active_tab = DetailTab::Worlds;
            }
            self.edit_runtime_version = inst.version_id.clone();
            self.edit_runtime_loader = inst.mod_loader.name.to_ascii_lowercase();
            self.edit_runtime_show_snapshots = inst.mod_loader.is_vanilla()
                && self
                    .available_versions
                    .iter()
                    .any(|(vid, vtype)| vid == &inst.version_id && vtype != "release");
            self.edit_group_input = inst.group.clone();
            self.edit_tags_input = inst.tags.join(", ");
        }
        self.selected_mods = self.mods_cache.get(id).cloned().unwrap_or_default();
        self.selected_backups = self.backups_cache.get(id).cloned().unwrap_or_default();
        self.selected_worlds = self.worlds_cache.get(id).cloned().unwrap_or_default();
        self.selected_screenshots = self.screenshots_cache.get(id).cloned().unwrap_or_default();
        self.selected_servers = self.servers_cache.get(id).cloned().unwrap_or_default();
        self.selected_content_packs = self
            .active_tab
            .content_kind()
            .and_then(|k| self.content_packs_cache.get(&(id.clone(), k)))
            .cloned()
            .unwrap_or_default();
        if let Some(ref s) = self.selected_settings {
            self.edit_jvm_args_text = s.jvm_arguments.join("\n");
            self.edit_game_args_text = s.game_arguments.join("\n");
            self.edit_env_vars_text = s
                .env_vars
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("\n");
        } else {
            self.edit_jvm_args_text.clear();
            self.edit_game_args_text.clear();
            self.edit_env_vars_text.clear();
        }
        self.mod_update_candidates.clear();
        self.selected_mod_files.clear();
        self.mod_search_query.clear();
        self.mod_status_filter = "all".to_string();
        if let Some(ref inst_id) = self.selected_instance_id {
            let notes_path = self.paths.instance_dir(inst_id).join("notes.txt");
            let notes = std::fs::read_to_string(&notes_path).unwrap_or_default();
            self.edit_instance_notes_text = notes.clone();
            self.instance_notes_baseline = notes;
        } else {
            self.edit_instance_notes_text.clear();
            self.instance_notes_baseline.clear();
        }
        self.screenshot_thumbs.clear();
        self.screenshot_thumbs_loading.clear();
        self.screenshot_thumb_lru.clear();
        self.world_icon_textures.clear();
        self.pack_icon_textures.clear();
        self.pack_update_candidates.clear();
        self.pack_updates_kind = None;
        self.checking_pack_updates = false;
        self.screenshot_preview_path = None;
        self.screenshot_preview_texture = None;
        self.instance_settings_dirty = false;
        self.refresh_mod_compat_report();
    }

    pub(crate) fn ensure_active_tab_loaded(&mut self, ctx: &egui::Context) {
        let Some(id) = self.selected_instance_id.clone() else {
            return;
        };
        if self
            .instances
            .iter()
            .find(|i| i.instance_id == id)
            .map(|i| i.mod_loader.is_vanilla())
            .unwrap_or(false)
            && matches!(self.active_tab, DetailTab::Mods | DetailTab::Shaders)
        {
            self.active_tab = DetailTab::Worlds;
        }
        match self.active_tab {
            DetailTab::Mods => self.load_mods_async(&id, ctx, false),
            DetailTab::Servers => self.load_servers_async(&id, ctx, false),
            DetailTab::ResourcePacks => {
                self.load_content_packs_async(&id, ContentKind::ResourcePack, ctx, false)
            }
            DetailTab::Shaders => {
                self.load_content_packs_async(&id, ContentKind::ShaderPack, ctx, false)
            }
            DetailTab::Worlds => self.load_worlds_async(&id, ctx, false),
            DetailTab::Screenshots => self.load_screenshots_async(&id, ctx, false),
            DetailTab::Backups => self.load_backups_async(&id, ctx, false),
            _ => {}
        }
    }

    pub(crate) fn load_content_packs_async(
        &mut self,
        instance_id: &str,
        kind: ContentKind,
        ctx: &egui::Context,
        force: bool,
    ) {
        let key = (instance_id.to_string(), kind);
        if !force {
            if let Some(cached) = self.content_packs_cache.get(&key) {
                if self.selected_instance_id.as_deref() == Some(instance_id)
                    && self.active_tab.content_kind() == Some(kind)
                {
                    self.selected_content_packs = cached.clone();
                }
                return;
            }
        }
        if !self.loading_content_packs.insert(key) {
            return;
        }
        let mc_dir = self.paths.minecraft_dir(instance_id);
        let id = instance_id.to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let packs =
                tokio::task::spawn_blocking(move || ContentPackManager::list(&mc_dir, kind))
                    .await
                    .unwrap_or_default();
            let _ = tx.send(AppEvent::ContentPacksLoaded {
                instance_id: id,
                kind,
                packs,
            });
            ctx.request_repaint();
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_content_pack_install(
        &mut self,
        kind: ContentKind,
        provider_is_cf: bool,
        item_key: String,
        display_name: String,
        cf_mod_id: Option<u64>,
        datapack_world: Option<String>,
        ctx: &egui::Context,
    ) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        if !self.installing_content_packs.insert(item_key.clone()) {
            return;
        }
        let kind_label = match kind {
            ContentKind::ResourcePack => "Resource Pack",
            ContentKind::ShaderPack => "Shader Pack",
            ContentKind::DataPack => "Data Pack",
        };
        let task_id = self.begin_task(
            format!("{kind_label}: {display_name}"),
            format!("Downloading into {}...", inst.name),
        );
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let target_dir = if kind == ContentKind::DataPack {
            match datapack_world
                .as_deref()
                .map(str::trim)
                .filter(|w| !w.is_empty())
            {
                Some(world) => mc_dir.join("saves").join(world).join("datapacks"),
                None => mc_dir.join(kind.folder_name()),
            }
        } else {
            mc_dir.join(kind.folder_name())
        };
        let api_key = self.resolved_cf_api_key();
        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let slug = item_key.clone();
        let downloader = self.download_client.clone();

        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let path = if provider_is_cf {
                    let client =
                        CurseForgeClient::new(api_key, cache).map_err(|e| e.to_string())?;
                    client
                        .install_content_pack(
                            cf_mod_id.unwrap_or(0),
                            Some(&slug),
                            &inst.version_id,
                            &target_dir,
                            &downloader,
                        )
                        .await
                        .map_err(|e| e.to_string())?
                } else {
                    let mr = ModrinthClient::new(cache).map_err(|e| e.to_string())?;
                    mr.install_content_pack(&slug, &inst.version_id, &target_dir, &downloader)
                        .await
                        .map_err(|e| e.to_string())?
                };
                Ok::<String, String>(
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("pack.zip")
                        .to_string(),
                )
            }
            .await;

            let _ = tx.send(AppEvent::ContentPackInstallFinished {
                task_id,
                item_key,
                instance_id: inst.instance_id,
                kind,
                result: res,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn load_worlds_async(
        &mut self,
        instance_id: &str,
        ctx: &egui::Context,
        force: bool,
    ) {
        if !force {
            if let Some(cached) = self.worlds_cache.get(instance_id) {
                if self.selected_instance_id.as_deref() == Some(instance_id) {
                    self.selected_worlds = cached.clone();
                }
                return;
            }
        }
        if !self.loading_worlds.insert(instance_id.to_string()) {
            return;
        }
        let mc_dir = self.paths.minecraft_dir(instance_id);
        let id = instance_id.to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let worlds = tokio::task::spawn_blocking(move || WorldManager::list_worlds(&mc_dir))
                .await
                .unwrap_or_default();
            let _ = tx.send(AppEvent::WorldsLoaded {
                instance_id: id,
                worlds,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn load_screenshots_async(
        &mut self,
        instance_id: &str,
        ctx: &egui::Context,
        force: bool,
    ) {
        if !force {
            if let Some(cached) = self.screenshots_cache.get(instance_id) {
                if self.selected_instance_id.as_deref() == Some(instance_id) {
                    self.selected_screenshots = cached.clone();
                }
                return;
            }
        }
        if !self.loading_screenshots.insert(instance_id.to_string()) {
            return;
        }
        let mc_dir = self.paths.minecraft_dir(instance_id);
        let id = instance_id.to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let screenshots =
                tokio::task::spawn_blocking(move || ScreenshotManager::list_screenshots(&mc_dir))
                    .await
                    .unwrap_or_default();
            let _ = tx.send(AppEvent::ScreenshotsLoaded {
                instance_id: id,
                screenshots,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_mclogs_upload(&mut self, content: String, ctx: &egui::Context) {
        if self.uploading_mclogs {
            return;
        }
        self.uploading_mclogs = true;
        self.last_mclogs_url = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = McLogsClient::upload(&content)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::McLogsUploaded(res));
            ctx.request_repaint();
        });
    }

    pub(crate) fn load_mods_async(&mut self, instance_id: &str, ctx: &egui::Context, force: bool) {
        if !force {
            if let Some(cached) = self.mods_cache.get(instance_id) {
                if self.selected_instance_id.as_deref() == Some(instance_id) {
                    self.selected_mods = cached.clone();
                }
                return;
            }
        }
        if !self.loading_mods.insert(instance_id.to_string()) {
            return;
        }
        let mods_dir = self.paths.minecraft_dir(instance_id).join("mods");
        let mods_cache_file = self
            .paths
            .cache_root
            .join("mod_metadata")
            .join(format!("{instance_id}.json"));
        let id = instance_id.to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let mods = tokio::task::spawn_blocking(move || {
                ModScanner::scan_dir_cached(&mods_dir, &mods_cache_file)
            })
            .await
            .unwrap_or_default();
            let _ = tx.send(AppEvent::ModsLoaded {
                instance_id: id,
                mods,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn load_backups_async(
        &mut self,
        instance_id: &str,
        ctx: &egui::Context,
        force: bool,
    ) {
        if !force {
            if let Some(cached) = self.backups_cache.get(instance_id) {
                if self.selected_instance_id.as_deref() == Some(instance_id) {
                    self.selected_backups = cached.clone();
                }
                return;
            }
        }
        if !self.loading_backups.insert(instance_id.to_string()) {
            return;
        }
        let paths = self.paths.clone();
        let id = instance_id.to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let id_inner = id.clone();
            let backups = tokio::task::spawn_blocking(move || {
                BackupManager::new(&paths)
                    .list_backups(&id_inner)
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            let _ = tx.send(AppEvent::BackupsLoaded {
                instance_id: id,
                backups,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn reload_accounts(&mut self) {
        let store = AccountStore::new(&self.paths);
        self.accounts = store.list().unwrap_or_default();
        self.account_avatars_loading.clear();
    }

    pub(crate) fn reload_java(&mut self) {
        self.java_installations = JavaDiscovery::discover(Some(&self.paths.runtimes_root));
    }

    pub(crate) fn active_account(&self) -> Option<&StoredAccount> {
        self.accounts
            .iter()
            .find(|a| a.active)
            .or_else(|| self.accounts.first())
    }

    pub(crate) fn account_for_instance(
        &self,
        settings: &InstanceSettings,
    ) -> Option<StoredAccount> {
        resolve_account_for_instance(
            &self.accounts,
            self.active_account(),
            settings.preferred_account_uuid.as_deref(),
        )
        .cloned()
    }

    pub(crate) fn switch_active_account(&mut self, uuid: &str) {
        if let Some(acc) = self.accounts.iter().find(|a| a.uuid == uuid).cloned() {
            let username = acc.username.clone();
            let _ = AccountStore::new(&self.paths).upsert(acc, true);
            self.reload_accounts();
            self.push_global_log(format!("[AUTH] Switched active account to '{username}'."));
        }
    }

    pub(crate) fn selected_instance(&self) -> Option<&Instance> {
        let id = self.selected_instance_id.as_ref()?;
        self.instances.iter().find(|i| &i.instance_id == id)
    }

    pub(crate) fn is_instance_busy(&self, instance_id: &str) -> bool {
        !matches!(
            self.launch_states.get(instance_id),
            None | Some(LaunchState::Idle)
        )
    }

    pub(crate) fn begin_task(
        &mut self,
        label: impl Into<String>,
        message: impl Into<String>,
    ) -> u64 {
        self.begin_task_with_control(label, message).0
    }

    pub(crate) fn begin_task_with_control(
        &mut self,
        label: impl Into<String>,
        message: impl Into<String>,
    ) -> (u64, Arc<TaskControl>) {
        let id = self.next_task_id;
        self.next_task_id += 1;
        let label_str = label.into();
        let msg_str = message.into();
        let control = TaskControl::new();
        self.active_tasks.push(ActiveTask::new(
            id,
            label_str.clone(),
            msg_str.clone(),
            Arc::clone(&control),
        ));
        if !msg_str.trim().is_empty() {
            self.last_task_log_msg.insert(id, msg_str.clone());
        }
        self.push_global_log(format!("[TASK] Started #{id} '{label_str}': {msg_str}"));
        (id, control)
    }

    pub(crate) fn finish_task(&mut self, task_id: u64) {
        if let Some(pos) = self.active_tasks.iter().position(|t| t.id == task_id) {
            let task = self.active_tasks.remove(pos);
            self.push_global_log(format!("[TASK] Finished #{task_id} '{}'.", task.label));
        }
        self.last_task_log_msg.remove(&task_id);
        if self.active_tasks.is_empty() {
            mcw_net::flush_persistent_verification_cache();
        }
    }

    pub(crate) fn latest_mc_version(&self, include_snapshots: bool) -> String {
        if include_snapshots {
            self.available_versions
                .first()
                .map(|(id, _)| id.clone())
                .unwrap_or_default()
        } else {
            self.available_versions
                .iter()
                .find(|(_, t)| t == "release")
                .or_else(|| self.available_versions.first())
                .map(|(id, _)| id.clone())
                .unwrap_or_default()
        }
    }

    pub(crate) fn fetch_versions(&mut self, ctx: &egui::Context) {
        if self.versions_loading {
            return;
        }
        self.versions_loading = true;
        self.push_global_log("[INFO] Loading Minecraft version manifest...");
        let paths = self.paths.clone();
        let client = self.download_client.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let svc = VersionManifestService::new(&paths, &client);
                let manifest = svc.get_manifest(false).await.map_err(|e| e.to_string())?;
                let versions: Vec<(String, String)> = manifest
                    .versions
                    .into_iter()
                    .map(|v| (v.id, v.version_type))
                    .collect();
                Ok::<Vec<(String, String)>, String>(versions)
            }
            .await;
            let _ = tx.send(AppEvent::VersionsLoaded(res));
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_launch(&mut self, ctx: &egui::Context) {
        self.start_launch_impl(None, None, ctx);
    }

    pub(crate) fn start_launch_with_world(
        &mut self,
        quick_play_world: Option<String>,
        ctx: &egui::Context,
    ) {
        self.start_launch_impl(quick_play_world, None, ctx);
    }

    pub(crate) fn start_launch_impl(
        &mut self,
        quick_play_world: Option<String>,
        quick_play_server: Option<String>,
        ctx: &egui::Context,
    ) {
        self.start_launch_with_flags(quick_play_world, quick_play_server, false, false, ctx);
    }

    pub(crate) fn launch_with_preflight_bypass(
        &mut self,
        instance_id: String,
        quick_play_world: Option<String>,
        quick_play_server: Option<String>,
        ctx: &egui::Context,
    ) {
        if self.selected_instance_id.as_deref() != Some(instance_id.as_str()) {
            self.selected_instance_id = Some(instance_id);
            self.sync_selected_from_cache();
        }
        self.start_launch_with_flags(quick_play_world, quick_play_server, true, false, ctx);
    }

    pub(crate) fn launch_with_security_rp_bypass(
        &mut self,
        instance_id: String,
        quick_play_world: Option<String>,
        quick_play_server: Option<String>,
        ctx: &egui::Context,
    ) {
        if self.selected_instance_id.as_deref() != Some(instance_id.as_str()) {
            self.selected_instance_id = Some(instance_id);
            self.sync_selected_from_cache();
        }
        self.start_launch_with_flags(quick_play_world, quick_play_server, false, true, ctx);
    }

    pub(crate) fn start_launch_for_instance(
        &mut self,
        quick_play_world: Option<String>,
        quick_play_server: Option<String>,
        bypass_preflight: bool,
        ctx: &egui::Context,
    ) {
        self.start_launch_with_flags(
            quick_play_world,
            quick_play_server,
            bypass_preflight,
            false,
            ctx,
        );
    }

    fn start_launch_with_flags(
        &mut self,
        quick_play_world: Option<String>,
        quick_play_server: Option<String>,
        bypass_preflight: bool,
        bypass_security_rp: bool,
        ctx: &egui::Context,
    ) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        let Some(settings) = self.selected_settings.clone() else {
            return;
        };
        let Some(account) = self.account_for_instance(&settings) else {
            self.notice_banner = Some((self.t("accounts.auth_required_notice").to_string(), true));
            self.open_accounts_window(ctx);
            return;
        };

        let instance_id = inst.instance_id.clone();
        if self.is_instance_busy(&instance_id) {
            return;
        }

        let missing_manual = self
            .pending_manual_counts
            .get(&instance_id)
            .copied()
            .unwrap_or(0);
        if missing_manual > 0 {
            let cf_policy = ManagedContentPolicy::resolve_for_provider(
                &settings,
                &self.prefs.modrinth_failure_policy,
                &self.prefs.curseforge_failure_policy,
                &self.prefs.forge_preflight_failure_policy,
                "curseforge",
            );
            if cf_policy.is_blocking() {
                self.notice_banner = Some((
                    format!(
                        "{missing_manual} CurseForge manual download(s) required before launch (policy: block)."
                    ),
                    true,
                ));
                self.open_manual_downloads_for(&instance_id, ctx);
                return;
            } else if cf_policy.warns() {
                self.push_global_log(format!(
                    "[WARN] Launching '{instance_id}' with {missing_manual} missing CurseForge manual file(s) (policy: warn)."
                ));
            }
        }

        // Lesson #8: Clear previous launch errors & logs for the new run.
        self.notice_banner = None;
        self.instance_logs.insert(instance_id.clone(), Vec::new());
        self.last_launch_log_msg.remove(&instance_id);
        self.push_global_log(format!(
            "[LAUNCH] Starting launch pipeline for '{}' (MC {}, loader: {}).",
            inst.name, inst.version_id, inst.mod_loader.name
        ));
        let (launch_task_id, task_ctrl) =
            self.begin_task_with_control(format!("Launch {}", inst.name), "Preparing launch...");
        self.launch_task_ids
            .insert(instance_id.clone(), launch_task_id);
        self.launch_states.insert(
            instance_id.clone(),
            LaunchState::Preparing(ProgressEvent {
                stage: mcw_models::progress::ProgressStage::FetchingManifest,
                message: "Preparing launch...".to_string(),
                current: 0,
                total: 1,
            }),
        );

        let paths = self.paths.clone();
        let prefer_dedicated_gpu = self.prefs.prefer_dedicated_gpu;
        let mitigate_minecraft2shell = self.prefs.mitigate_minecraft2shell;
        let warn_auto_accept_server_rp = self.prefs.warn_auto_accept_server_resource_packs;
        let forge_policy_str = self.prefs.forge_preflight_failure_policy.clone();
        let is_official_pack =
            detect_installed_pack(&self.paths.instance_dir(&inst.instance_id)).is_some();
        let launch_check_disabled = settings.is_launch_checking_disabled(is_official_pack);
        let always_allow_preflight =
            bypass_preflight || settings.forge_preflight_always_allow || launch_check_disabled;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let client = self.download_client.with_task_control(task_ctrl);

        self.runtime.spawn(async move {
            let result = async {
                let lock = InstanceRunLock::acquire(&paths, &inst.instance_id)
                    .map_err(|e| e.to_string())?;

                let (auth, source) = match account.account_type {
                    AccountSource::Offline => (
                        OfflineAuth::authenticate(&account.username).map_err(|e| e.to_string())?,
                        AccountSource::Offline,
                    ),
                    AccountSource::Microsoft => {
                        let acc_store = AccountStore::new(&paths);
                        let rt = acc_store
                            .load_refresh_token(&account.uuid)
                            .map_err(|e| format!("Account token unavailable or expired: {e}"))?;
                        let ms = MicrosoftAuthClient::new().map_err(|e| e.to_string())?;
                        let refreshed = ms.refresh_microsoft_token(&rt).await.map_err(|e| {
                            format!("Microsoft token expired or refresh failed: {e}")
                        })?;
                        let login = ms.complete_minecraft_chain(refreshed).await.map_err(|e| {
                            format!("Minecraft authentication failed (account/token): {e}")
                        })?;
                        if let Some(ref new_rt) = login.refresh_token {
                            let _ = acc_store.store_refresh_token(&account.uuid, new_rt);
                        }
                        (login.authentication, AccountSource::Microsoft)
                    }
                };

                let pipeline = VanillaLaunchPipeline::new(&paths, &client);
                let progress_sink = Arc::new(ChannelProgressSink {
                    instance_id: inst.instance_id.clone(),
                    tx: tx.clone(),
                    ctx: ctx.clone(),
                });

                let mut prepared = pipeline
                    .prepare(&inst, &settings, &auth, source, progress_sink)
                    .await
                    .map_err(|e| e.to_string())?;

                // Pre-launch dependency & compatibility check across modloaders:
                // Official modpacks (Modrinth/CurseForge/FTB/ATLauncher) and .mcwpack packs
                // with `disable_launch_checking` silently log findings to launcher logs without dialogs.
                let forge_policy = mcw_models::FailurePolicy::parse_global(&forge_policy_str);
                let preflight =
                    ForgePreflightManager::scan(&paths, &inst, &prepared.metadata, false);
                if launch_check_disabled || always_allow_preflight {
                    let mut silent_notes: Vec<String> = preflight
                        .issues
                        .iter()
                        .map(|i| {
                            format!(
                                "[{}] {}: {}",
                                i.severity.to_ascii_uppercase(),
                                i.code,
                                i.message
                            )
                        })
                        .collect();
                    let loader_low = inst.mod_loader.name.trim().to_ascii_lowercase();
                    if !matches!(loader_low.as_str(), "vanilla" | "forge" | "neoforge") {
                        let mods_dir = paths.minecraft_dir(&inst.instance_id).join("mods");
                        if mods_dir.is_dir() {
                            let mods = ModManager::new(&mods_dir).list();
                            let compat =
                                ModCompatibilityAnalyzer::analyze(&inst.mod_loader.name, &mods);
                            for issue in compat.issues {
                                let sev = match issue.severity {
                                    ModIssueSeverity::Error => "ERROR",
                                    ModIssueSeverity::Warning => "WARNING",
                                    ModIssueSeverity::Info => "INFO",
                                };
                                silent_notes
                                    .push(format!("[{sev}] {}: {}", issue.code, issue.message));
                            }
                        }
                    }
                    if launch_check_disabled && silent_notes.is_empty() {
                        silent_notes.push(format!(
                            "Dependency check dialog disabled for modpack ({} loader).",
                            inst.mod_loader.name
                        ));
                    }
                    for note in silent_notes {
                        let _ = tx.send(AppEvent::LogReceived {
                            instance_id: inst.instance_id.clone(),
                            line: LogLine {
                                stream: LogStream::Stdout,
                                text: format!("[LaunchCheck] {note}"),
                            },
                        });
                    }
                } else {
                    let wants_ask = forge_policy_str.trim().eq_ignore_ascii_case("ask")
                        || (forge_policy == mcw_models::FailurePolicy::Warn
                            && preflight.error_count() > 0);
                    if wants_ask && !preflight.issues.is_empty() {
                        let issue_lines: Vec<String> = preflight
                            .issues
                            .iter()
                            .map(|i| {
                                format!(
                                    "[{}] {}: {}",
                                    i.severity.to_ascii_uppercase(),
                                    i.code,
                                    i.message
                                )
                            })
                            .collect();
                        drop(lock);
                        let _ = tx.send(AppEvent::PreflightConfirmationRequired {
                            instance_id: inst.instance_id.clone(),
                            quick_play_world: quick_play_world.clone(),
                            quick_play_server: quick_play_server.clone(),
                            issues: issue_lines,
                        });
                        ctx.request_repaint();
                        return Ok(None);
                    }
                    let preflight_notes = ForgePreflightManager::enforce(&preflight, forge_policy)
                        .map_err(|e| e.to_string())?;
                    for note in preflight_notes {
                        let _ = tx.send(AppEvent::LogReceived {
                            instance_id: inst.instance_id.clone(),
                            line: LogLine {
                                stream: LogStream::Stderr,
                                text: format!("[ForgePreflight] {note}"),
                            },
                        });
                    }
                }

                let base_sync_id = prepared
                    .metadata
                    .inherits_from
                    .clone()
                    .unwrap_or_else(|| prepared.metadata.id.clone());
                let active_policy = SecurityPolicyService::load_cached(&paths);
                if let Ok(sync_outcome) = MojangLibrarySync::sync_freetype_for_version(
                    &paths,
                    &client,
                    &base_sync_id,
                    &active_policy,
                )
                .await
                {
                    let sync_msg = match &sync_outcome {
                        LibrarySyncOutcome::AlreadyUpToDate { description, .. } => {
                            format!("[Security][Sync] {description}")
                        }
                        LibrarySyncOutcome::UpdatedPatchedFreetype { description, .. } => {
                            if let Ok(raw) = fs::read_to_string(paths.version_json(&base_sync_id)) {
                                if let Ok(fresh_base) =
                                    serde_json::from_str::<mcw_models::minecraft::VersionMetadata>(&raw)
                                {
                                    let patched_ft: Vec<_> = fresh_base
                                        .libraries
                                        .into_iter()
                                        .filter(|l| {
                                            l.name
                                                .to_ascii_lowercase()
                                                .contains("org.lwjgl:lwjgl-freetype")
                                        })
                                        .collect();
                                    if !patched_ft.is_empty()
                                        && prepared.metadata.inherits_from.is_none()
                                    {
                                        prepared.metadata.libraries.retain(|l| {
                                            !l.name
                                                .to_ascii_lowercase()
                                                .contains("org.lwjgl:lwjgl-freetype")
                                        });
                                        prepared.metadata.libraries.extend(patched_ft);
                                    }
                                }
                            }
                            format!("[Security][Sync] {description}")
                        }
                        LibrarySyncOutcome::NoPatchAvailable { description, .. } => {
                            format!("[Security][Sync] {description}")
                        }
                        LibrarySyncOutcome::OfflineSkipped { description, .. } => {
                            format!("[Security][Sync] {description}")
                        }
                        LibrarySyncOutcome::RolledBack { reason, .. } => {
                            format!("[Security][Sync][WARN] {reason}")
                        }
                    };
                    let _ = tx.send(AppEvent::LogReceived {
                        instance_id: inst.instance_id.clone(),
                        line: LogLine {
                            stream: if matches!(sync_outcome, LibrarySyncOutcome::RolledBack { .. })
                            {
                                LogStream::Stderr
                            } else {
                                LogStream::Stdout
                            },
                            text: sync_msg,
                        },
                    });
                }

                let preflight_assessment = SecurityManager::assess_instance(
                    &paths,
                    &prepared.metadata,
                    &inst.instance_id,
                    mitigate_minecraft2shell,
                    prepared.java.major,
                    &active_policy,
                );
                let _ = tx.send(AppEvent::LogReceived {
                    instance_id: inst.instance_id.clone(),
                    line: LogLine {
                        stream: if preflight_assessment.status == SecurityStatus::Unprotected {
                            LogStream::Stderr
                        } else {
                            LogStream::Stdout
                        },
                        text: format!(
                            "[Security] {} — {}",
                            preflight_assessment.status.display_title(),
                            preflight_assessment.description
                        ),
                    },
                });

                let uses_freetype =
                    !matches!(preflight_assessment.freetype_status, FreetypeLibraryStatus::NotUsed);
                if uses_freetype && warn_auto_accept_server_rp && !bypass_security_rp {
                    let mc_dir = paths.minecraft_dir(&inst.instance_id);
                    let trusted: HashSet<String> = settings
                        .trusted_resource_pack_servers
                        .iter()
                        .map(|s| s.trim().to_ascii_lowercase())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let risky_servers: Vec<(usize, String, String)> =
                        ServerManager::list_servers(&mc_dir)
                            .into_iter()
                            .enumerate()
                            .filter_map(|(idx, srv)| {
                                if srv.accept_textures == Some(1) {
                                    let norm_ip = srv.ip.trim().to_ascii_lowercase();
                                    if !trusted.contains(&norm_ip) {
                                        return Some((idx, srv.name, srv.ip));
                                    }
                                }
                                None
                            })
                            .collect();
                    if !risky_servers.is_empty() {
                        drop(lock);
                        let _ = tx.send(AppEvent::ServerRpSecurityConfirmationRequired {
                            instance_id: inst.instance_id.clone(),
                            quick_play_world: quick_play_world.clone(),
                            quick_play_server: quick_play_server.clone(),
                            risky_servers,
                            mitigation_enabled: mitigate_minecraft2shell,
                            assessment_headline: preflight_assessment.headline.clone(),
                            assessment_description: preflight_assessment.description.clone(),
                        });
                        ctx.request_repaint();
                        return Ok(None);
                    }
                }

                let attached_security_agent = preflight_assessment.should_attach_agent;
                prepared.spec.jvm_args =
                    SecurityAgentManager::sanitize_user_jvm_args(&prepared.spec.jvm_args);
                if attached_security_agent {
                    let sec_args = SecurityAgentManager::build_jvm_args(
                        &paths.cache_root.join("security_agent"),
                        &paths.instance_dir(&inst.instance_id),
                        prepared.java.major,
                    )
                    .map_err(|e| e.to_string())?;
                    let mut combined = sec_args;
                    combined.extend(prepared.spec.jvm_args);
                    prepared.spec.jvm_args = combined;
                } else {
                    SecurityAgentManager::reset_status_file(&paths.instance_dir(&inst.instance_id));
                }

                prepared.spec.jvm_args =
                    LanAgentManager::sanitize_user_jvm_args(&prepared.spec.jvm_args);
                if LanAgentManager::is_enabled(
                    settings.offline_multiplayer_enabled,
                    &settings.lan_auth_mode,
                ) {
                    let resolution =
                        LanAgentTargetResolver::resolve(&paths, &client, &prepared.metadata, &inst)
                            .await;
                    for warn in &resolution.warnings {
                        let _ = tx.send(AppEvent::LogReceived {
                            instance_id: inst.instance_id.clone(),
                            line: LogLine {
                                stream: LogStream::Stdout,
                                text: format!("[LAN Agent Resolver] {warn}"),
                            },
                        });
                    }
                    let extra_targets = resolution.encoded_targets();
                    let agent_args = LanAgentManager::build_jvm_args(
                        &paths.cache_root.join("lan_agent"),
                        &paths.instance_dir(&inst.instance_id),
                        &inst.mod_loader.name,
                        &extra_targets,
                    )
                    .map_err(|e| e.to_string())?;
                    let mut combined = agent_args;
                    combined.extend(prepared.spec.jvm_args);
                    prepared.spec.jvm_args = combined;
                }

                if let Some(world_folder) = quick_play_world {
                    if is_quick_play_supported(&inst.version_id) {
                        prepared
                            .spec
                            .game_args
                            .push("--quickPlaySingleplayer".to_string());
                        prepared.spec.game_args.push(world_folder);
                    }
                } else if let Some(server_addr) = quick_play_server {
                    if is_quick_play_supported(&inst.version_id) {
                        prepared
                            .spec
                            .game_args
                            .push("--quickPlayMultiplayer".to_string());
                        prepared.spec.game_args.push(server_addr);
                    }
                }

                if !settings.pre_launch_command.trim().is_empty() {
                    let pre_cmd = settings.pre_launch_command.clone();
                    let pre_cwd = prepared.spec.working_dir.clone();
                    let pre_env = prepared.spec.env_vars.clone();
                    let _ = tx.send(AppEvent::LogReceived {
                        instance_id: inst.instance_id.clone(),
                        line: LogLine {
                            stream: LogStream::Stdout,
                            text: format!("[Pre-launch] Running: {}", pre_cmd.trim()),
                        },
                    });
                    tokio::task::spawn_blocking(move || {
                        run_lifecycle_hook(&pre_cmd, &pre_cwd, &pre_env, "Pre-launch")
                    })
                    .await
                    .map_err(|e| format!("Pre-launch hook task error: {e}"))??;
                }

                let log_path = paths
                    .logs_root
                    .join(format!("{}-latest.log", inst.instance_id));
                let log_sink = Arc::new(ChannelLogSink {
                    instance_id: inst.instance_id.clone(),
                    tx: tx.clone(),
                    ctx: ctx.clone(),
                });
                let supervised = SupervisedProcess::spawn_with_gpu_preference(
                    &prepared.spec,
                    Some(&log_path),
                    log_sink,
                    prefer_dedicated_gpu,
                )
                .map_err(|e| e.to_string())?;

                let pid = supervised.pid();
                let _ = lock.mark_running(pid);
                let control = supervised.control();
                let _ = tx.send(AppEvent::LaunchStarted {
                    instance_id: inst.instance_id.clone(),
                    pid,
                    control,
                });
                ctx.request_repaint();

                let report = supervised.wait().await.map_err(|e| e.to_string())?;

                if attached_security_agent {
                    if let Some(rt) =
                        SecurityAgentManager::read_runtime_status(&paths.instance_dir(&inst.instance_id))
                    {
                        let rt_msg = match rt.state {
                            SecurityAgentRuntimeState::ActiveVerified => format!(
                                "[Security] Runtime verification passed: ACTIVE_VERIFIED ({} FreeType method(s) patched -> FT_LOAD_NO_HINTING).",
                                rt.patched_methods
                            ),
                            SecurityAgentRuntimeState::Initialized => "[Security] Agent initialized (FreeType class was not loaded during this session).".to_string(),
                            SecurityAgentRuntimeState::Failed | SecurityAgentRuntimeState::Incompatible => format!(
                                "[Security][WARN] Runtime protection verification FAILED ({:?}): {}",
                                rt.state, rt.detail
                            ),
                            other => format!("[Security] Agent status: {other:?} ({})", rt.detail),
                        };
                        let _ = tx.send(AppEvent::LogReceived {
                            instance_id: inst.instance_id.clone(),
                            line: LogLine {
                                stream: if matches!(
                                    rt.state,
                                    SecurityAgentRuntimeState::Failed
                                        | SecurityAgentRuntimeState::Incompatible
                                ) {
                                    LogStream::Stderr
                                } else {
                                    LogStream::Stdout
                                },
                                text: rt_msg,
                            },
                        });
                    }
                }

                if !settings.post_exit_command.trim().is_empty() {
                    let post_cmd = settings.post_exit_command.clone();
                    let post_cwd = prepared.spec.working_dir.clone();
                    let mut post_env = prepared.spec.env_vars.clone();
                    post_env.push((
                        "INST_EXITCODE".to_string(),
                        report.exit_code.map(|c| c.to_string()).unwrap_or_default(),
                    ));
                    if let Ok(Err(warn_msg)) = tokio::task::spawn_blocking(move || {
                        run_lifecycle_hook(&post_cmd, &post_cwd, &post_env, "Post-exit")
                    })
                    .await
                    {
                        let _ = tx.send(AppEvent::LogReceived {
                            instance_id: inst.instance_id.clone(),
                            line: LogLine {
                                stream: LogStream::Stderr,
                                text: format!("[Post-exit] Warning: {warn_msg}"),
                            },
                        });
                    }
                }

                // Persist playtime & exit status
                let store = InstanceStore::new(&paths);
                if let Ok(mut updated) = store.load_instance(&inst.instance_id) {
                    updated.total_playtime_seconds = updated
                        .total_playtime_seconds
                        .saturating_add(report.duration_secs);
                    updated.last_exit_code = report.exit_code;
                    updated.last_launch_crashed =
                        report.classification == ExitClassification::Crashed;
                    updated.last_launch_state = match report.classification {
                        ExitClassification::Crashed => "crashed".to_string(),
                        ExitClassification::Normal | ExitClassification::StoppedByUser => {
                            "finished".to_string()
                        }
                    };
                    let _ = store.save_instance(&updated);
                }

                drop(lock);
                Ok::<_, String>(Some(report))
            }
            .await;

            let launch_report = match result {
                Ok(Some(rep)) => Ok(rep),
                Ok(None) => return,
                Err(err) => Err(err),
            };
            let _ = tx.send(AppEvent::LaunchFinished {
                instance_id: inst.instance_id,
                report: launch_report,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn cancel_microsoft_login(&mut self) {
        if let Some(handle) = self.ms_login_task.take() {
            handle.abort();
        }
        self.ms_login_in_progress = false;
        self.ms_login_mode = None;
        self.ms_device_code = None;
        self.notice_banner = Some(("Microsoft sign-in cancelled.".to_string(), false));
    }

    pub(crate) fn start_microsoft_login(&mut self, browser_pkce: bool, ctx: &egui::Context) {
        if self.ms_login_in_progress {
            return;
        }
        if self.connectivity_status == ConnectivityStatus::Offline {
            self.notice_banner = Some((
                "Offline mode: connect to the internet before signing in with Microsoft."
                    .to_string(),
                true,
            ));
            self.push_global_log("[AUTH] Microsoft login skipped while offline.".to_string());
            return;
        }
        self.ms_login_in_progress = true;
        self.ms_login_mode = Some(if browser_pkce { "pkce" } else { "device_code" });
        self.ms_device_code = None;
        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        let handle = self.runtime.spawn(async move {
            let res = async {
                let client = MicrosoftAuthClient::new().map_err(|e| e.to_string())?;
                let oauth = if browser_pkce {
                    client
                        .authenticate_browser_pkce(Duration::from_secs(180), |url| {
                            open_external(url);
                        })
                        .await
                        .map_err(|e| e.to_string())?
                } else {
                    let dc = client
                        .request_device_code()
                        .await
                        .map_err(|e| e.to_string())?;
                    open_external(&dc.verification_uri);
                    let _ = tx.send(AppEvent::DeviceCodeReady(dc.clone()));
                    ctx.request_repaint();
                    client
                        .poll_device_code(&dc.device_code, dc.interval, dc.expires_in)
                        .await
                        .map_err(|e| e.to_string())?
                };

                let login = client
                    .complete_minecraft_chain(oauth)
                    .await
                    .map_err(|e| e.to_string())?;
                let uuid = login.authentication.uuid.clone();
                let store = AccountStore::new(&paths);
                if let Some(ref rt) = login.refresh_token {
                    store
                        .store_refresh_token(&uuid, rt)
                        .map_err(|e| e.to_string())?;
                }
                let stored = StoredAccount {
                    uuid,
                    username: login.profile.name.clone(),
                    account_type: AccountSource::Microsoft,
                    skin_url: login.profile.active_skin_url(),
                    active: true,
                };
                store.upsert(stored, true).map_err(|e| e.to_string())
            }
            .await;

            let _ = tx.send(AppEvent::MicrosoftLoginFinished(res));
            ctx.request_repaint();
        });
        self.ms_login_task = Some(handle);
    }

    pub(crate) fn ensure_thumbnail(&mut self, url: &str, ctx: &egui::Context) {
        let u = url.trim();
        if u.is_empty() {
            return;
        }
        if self.thumb_textures.contains_key(u) {
            self.thumb_lru_order.retain(|k| k != u);
            self.thumb_lru_order.push(u.to_string());
            return;
        }
        if !self.loading_thumbs.insert(u.to_string()) {
            return;
        }
        let url_owned = u.to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let ok_bytes = reqwest::Client::builder()
                .user_agent(mcw_models::user_agent())
                .timeout(Duration::from_secs(10))
                .build()
                .ok();
            if let Some(http) = ok_bytes {
                if let Ok(resp) = http.get(&url_owned).send().await {
                    if resp.status().is_success() {
                        if let Ok(bytes) = resp.bytes().await {
                            let decoded = tokio::task::spawn_blocking(move || {
                                let img = image::load_from_memory(&bytes).ok()?;
                                let thumb = img.resize_to_fill(
                                    48,
                                    48,
                                    image::imageops::FilterType::Triangle,
                                );
                                let rgba = thumb.to_rgba8();
                                let (w, h) = rgba.dimensions();
                                Some(([w as usize, h as usize], rgba.into_raw()))
                            })
                            .await
                            .ok()
                            .flatten();
                            if let Some((size, rgba)) = decoded {
                                let _ = tx.send(AppEvent::ThumbnailLoaded {
                                    url: url_owned,
                                    size,
                                    rgba,
                                });
                                ctx.request_repaint();
                            }
                        }
                    }
                }
            }
        });
    }

    pub(crate) fn render_thumbnail(&self, ui: &mut egui::Ui, url: Option<&str>, size: f32) {
        let p = self.palette;
        if let Some(u) = url {
            if let Some(tex) = self.thumb_textures.get(u) {
                ui.add(
                    egui::Image::new((tex.id(), Vec2::splat(size)))
                        .corner_radius(CornerRadius::same(8)),
                );
                return;
            }
        }
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
        ui.painter().rect(
            rect,
            CornerRadius::same(8),
            p.elevated,
            Stroke::new(1.0_f32, p.border),
            egui::StrokeKind::Inside,
        );
    }

    pub(crate) fn clear_remote_thumbs_if_idle(&mut self) {
        if self.active_page != AppPage::Modpacks
            && self.instance_inline_browser.is_none()
            && !self.modpack_dialog.open
            && !self.version_picker_open
            && !self.install_target_open
        {
            self.thumb_textures.clear();
            self.thumb_lru_order.clear();
            self.loading_thumbs.clear();
        }
    }

    fn setup_version_picker_target_defaults(
        &mut self,
        _title: &str,
        content_kind: Option<ContentKind>,
        global_mode: bool,
        _ctx: &egui::Context,
    ) {
        self.version_picker_global_mode = global_mode;
        self.version_picker_content_kind = content_kind;
    }

    pub(crate) fn exact_compatible_instances_for_choice(
        &self,
        choice: &ModVersionChoice,
        content_kind: Option<ContentKind>,
    ) -> Vec<Instance> {
        self.instances
            .iter()
            .filter(|inst| {
                let ver_ok = choice.game_versions.is_empty()
                    || choice
                        .game_versions
                        .iter()
                        .any(|gv| gv.eq_ignore_ascii_case(&inst.version_id));
                if !ver_ok {
                    return false;
                }
                if content_kind.is_none() {
                    if inst.mod_loader.is_vanilla() {
                        return false;
                    }
                    choice.loaders.is_empty()
                        || choice
                            .loaders
                            .iter()
                            .any(|l| l.eq_ignore_ascii_case(&inst.mod_loader.name))
                } else {
                    true
                }
            })
            .cloned()
            .collect()
    }

    pub(crate) fn open_install_target_dialog(
        &mut self,
        choice: ModVersionChoice,
        ctx: &egui::Context,
    ) {
        let kind = self.version_picker_content_kind;
        let compat = self.exact_compatible_instances_for_choice(&choice, kind);
        let preferred_id = self
            .selected_instance_id
            .as_ref()
            .filter(|id| compat.iter().any(|i| &i.instance_id == *id))
            .cloned()
            .or_else(|| compat.first().map(|i| i.instance_id.clone()));

        let store = InstanceStore::new(&self.paths);
        let base_name = self.version_picker_title.trim();
        let base_name = if base_name.is_empty() {
            "New Instance"
        } else {
            base_name
        };
        let mut candidate = base_name.to_string();
        let mut idx = 2usize;
        while store.load_instance(&candidate).is_ok() {
            candidate = format!("{base_name} ({idx})");
            idx += 1;
        }

        let auto_mc = choice
            .game_versions
            .first()
            .cloned()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                let latest = self.latest_mc_version(false);
                (!latest.is_empty()).then_some(latest)
            })
            .unwrap_or_else(|| "1.21.4".to_string());

        let auto_loader = if kind.is_some() {
            "vanilla".to_string()
        } else {
            choice
                .loaders
                .iter()
                .map(|l| l.to_ascii_lowercase())
                .find(|l| matches!(l.as_str(), "fabric" | "forge" | "neoforge" | "quilt"))
                .or_else(|| {
                    let b = self.browser_loader_filter.to_ascii_lowercase();
                    matches!(b.as_str(), "fabric" | "forge" | "neoforge" | "quilt").then_some(b)
                })
                .unwrap_or_else(|| "fabric".to_string())
        };

        self.install_target_title = self.version_picker_title.clone();
        self.install_target_icon = self.version_picker_icon.clone();
        self.install_target_content_kind = kind;
        self.install_target_choice = Some(choice);
        self.install_target_datapack_world.clear();
        self.install_target_new_name = candidate;
        self.install_target_new_mc_version = auto_mc;
        self.install_target_new_loader = auto_loader;
        self.install_target_enable_lan_agent = false;
        if let Some(id) = preferred_id {
            self.install_target_create_new = false;
            self.install_target_instance_id = id.clone();
            if kind == Some(ContentKind::DataPack) {
                self.load_worlds_async(&id, ctx, false);
            }
        } else {
            self.install_target_create_new = true;
            self.install_target_instance_id.clear();
        }
        self.install_target_open = true;
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn open_modrinth_version_picker(
        &mut self,
        slug: String,
        title: String,
        icon_url: Option<String>,
        is_pack: bool,
        content_kind: Option<ContentKind>,
        global_mode: bool,
        ctx: &egui::Context,
    ) {
        let target_key = format!("mr:{slug}");
        self.setup_version_picker_target_defaults(&title, content_kind, global_mode, ctx);
        self.version_picker_open = true;
        self.version_picker_title = title;
        self.version_picker_icon = icon_url;
        let path_seg = match (is_pack, content_kind) {
            (true, _) => "modpack",
            (false, Some(ContentKind::ResourcePack)) => "resourcepack",
            (false, Some(ContentKind::ShaderPack)) => "shader",
            (false, Some(ContentKind::DataPack)) => "datapack",
            (false, None) => "mod",
        };
        self.version_picker_website_url = Some(format!("https://modrinth.com/{path_seg}/{slug}"));
        self.version_picker_key = target_key.clone();
        self.version_picker_is_pack = is_pack;
        self.version_picker_loading = true;
        self.version_picker_items.clear();
        self.version_picker_error = None;
        self.version_picker_include_beta = self.prefs.modrinth_include_beta;
        self.version_picker_include_alpha = self.prefs.modrinth_include_alpha;
        self.version_picker_selected_id.clear();

        if is_pack {
            self.version_picker_mc_filter = "all".to_string();
            self.version_picker_loader_filter = if self.browser_loader_filter.is_empty() {
                "all".to_string()
            } else {
                self.browser_loader_filter.to_ascii_lowercase()
            };
        } else if global_mode {
            self.version_picker_mc_filter = if self.browser_mc_version_filter.is_empty() {
                "all".to_string()
            } else {
                self.browser_mc_version_filter.clone()
            };
            self.version_picker_loader_filter =
                if content_kind.is_some() || self.browser_loader_filter.is_empty() {
                    "all".to_string()
                } else {
                    self.browser_loader_filter.to_ascii_lowercase()
                };
        } else {
            let inst = self.selected_instance().cloned();
            self.version_picker_mc_filter = inst
                .as_ref()
                .map(|i| i.version_id.clone())
                .unwrap_or_else(|| "all".to_string());
            self.version_picker_loader_filter = if content_kind.is_some() {
                "all".to_string()
            } else if self.browser_loader_filter.is_empty() {
                inst.as_ref()
                    .map(|i| i.mod_loader.name.to_ascii_lowercase())
                    .filter(|l| l != "vanilla")
                    .unwrap_or_else(|| "all".to_string())
            } else {
                self.browser_loader_filter.to_ascii_lowercase()
            };
        }

        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let mr = ModrinthClient::new(cache).map_err(|e| e.to_string())?;
                // Fetch all project versions so the user can filter by Release/Beta/Alpha, MC version, and Loader in the dialog
                let versions = mr
                    .list_versions(&slug, None, None)
                    .await
                    .map_err(|e| e.to_string())?;
                let choices: Vec<ModVersionChoice> = versions
                    .into_iter()
                    .take(120)
                    .filter_map(|v| {
                        let primary = v
                            .files
                            .iter()
                            .find(|f| f.primary)
                            .or_else(|| v.files.first())?;
                        let channel = match v.version_type.to_ascii_lowercase().as_str() {
                            "alpha" => ModpackChannel::Alpha,
                            "beta" => ModpackChannel::Beta,
                            _ => ModpackChannel::Release,
                        };
                        Some(ModVersionChoice {
                            id: v.id,
                            title: v.name,
                            version_number: v.version_number,
                            channel,
                            game_versions: v.game_versions,
                            loaders: v.loaders,
                            file_name: primary.filename.clone(),
                            download_url: Some(primary.url.clone()),
                            file_size: primary.size,
                            sha1: primary.hashes.get("sha1").cloned(),
                        })
                    })
                    .collect();
                Ok::<Vec<ModVersionChoice>, String>(choices)
            }
            .await;
            let _ = tx.send(AppEvent::ModVersionsLoaded {
                target_key,
                versions: res,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn open_curseforge_version_picker(
        &mut self,
        mod_id: u64,
        title: String,
        icon_url: Option<String>,
        content_kind: Option<ContentKind>,
        global_mode: bool,
        ctx: &egui::Context,
    ) {
        let target_key = format!("cf:{mod_id}");
        let web_url = self
            .browser_cf_results
            .iter()
            .find(|m| m.id == mod_id)
            .and_then(|m| {
                m.links
                    .as_ref()
                    .and_then(|l| l.website_url.clone())
                    .or_else(|| {
                        if !m.slug.is_empty() {
                            Some(format!(
                                "https://www.curseforge.com/minecraft/mc-mods/{}",
                                m.slug
                            ))
                        } else {
                            None
                        }
                    })
            });

        self.setup_version_picker_target_defaults(&title, content_kind, global_mode, ctx);
        self.version_picker_open = true;
        self.version_picker_title = title;
        self.version_picker_icon = icon_url;
        self.version_picker_website_url = web_url;
        self.version_picker_key = target_key.clone();
        self.version_picker_is_pack = false;
        self.version_picker_loading = true;
        self.version_picker_items.clear();
        self.version_picker_error = None;
        self.version_picker_include_beta = self.prefs.modrinth_include_beta;
        self.version_picker_include_alpha = self.prefs.modrinth_include_alpha;
        self.version_picker_selected_id.clear();

        let inst = if global_mode {
            None
        } else {
            self.selected_instance().cloned()
        };
        let gv_opt = if global_mode {
            let v = self.browser_mc_version_filter.trim();
            if v.is_empty() || v == "all" {
                None
            } else {
                Some(v.to_string())
            }
        } else {
            inst.as_ref().map(|i| i.version_id.clone())
        };
        self.version_picker_mc_filter = gv_opt.clone().unwrap_or_else(|| "all".to_string());
        self.version_picker_loader_filter = if content_kind.is_some() {
            "all".to_string()
        } else if global_mode {
            if self.browser_loader_filter.is_empty() {
                "all".to_string()
            } else {
                self.browser_loader_filter.to_ascii_lowercase()
            }
        } else if self.browser_loader_filter.is_empty() {
            inst.as_ref()
                .map(|i| i.mod_loader.name.to_ascii_lowercase())
                .filter(|l| l != "vanilla")
                .unwrap_or_else(|| "all".to_string())
        } else {
            self.browser_loader_filter.to_ascii_lowercase()
        };

        let api_key = self.resolved_cf_api_key();
        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let cf = CurseForgeClient::new(api_key, cache).map_err(|e| e.to_string())?;
                let mut files = Vec::new();
                let mut seen_ids = HashSet::new();
                if let Some(ref gv) = gv_opt {
                    if let Ok(gv_files) = cf.get_mod_files(mod_id, Some(gv.as_str()), None).await {
                        for f in gv_files {
                            if seen_ids.insert(f.id) {
                                files.push(f);
                            }
                        }
                    }
                }
                if let Ok(all_files) = cf.get_mod_files(mod_id, None, None).await {
                    for f in all_files {
                        if seen_ids.insert(f.id) {
                            files.push(f);
                        }
                    }
                }
                let choices: Vec<ModVersionChoice> = files
                    .into_iter()
                    .take(100)
                    .map(|f| {
                        let sha1 = f
                            .hashes
                            .iter()
                            .find(|h| h.algo == 1)
                            .map(|h| h.value.clone());
                        let channel = match f.release_type {
                            3 => ModpackChannel::Alpha,
                            2 => ModpackChannel::Beta,
                            _ => ModpackChannel::Release,
                        };
                        let mut gvs = Vec::new();
                        let mut loaders = Vec::new();
                        for token in f.game_versions {
                            let low = token.to_ascii_lowercase();
                            if matches!(low.as_str(), "forge" | "fabric" | "neoforge" | "quilt") {
                                if !loaders.contains(&low) {
                                    loaders.push(low);
                                }
                            } else if token
                                .chars()
                                .next()
                                .map(|c| c.is_ascii_digit())
                                .unwrap_or(false)
                            {
                                gvs.push(token);
                            }
                        }
                        ModVersionChoice {
                            id: f.id.to_string(),
                            title: if f.display_name.is_empty() {
                                f.file_name.clone()
                            } else {
                                f.display_name
                            },
                            version_number: format!("File #{}", f.id),
                            channel,
                            game_versions: gvs,
                            loaders,
                            file_name: f.file_name,
                            download_url: f.download_url.filter(|u| !u.trim().is_empty()),
                            file_size: f.file_length,
                            sha1,
                        }
                    })
                    .collect();
                Ok::<Vec<ModVersionChoice>, String>(choices)
            }
            .await;
            let _ = tx.send(AppEvent::ModVersionsLoaded {
                target_key,
                versions: res,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn filtered_mod_versions(&self) -> Vec<&ModVersionChoice> {
        let mc_filter = self.version_picker_mc_filter.trim();
        let loader_filter = self
            .version_picker_loader_filter
            .trim()
            .to_ascii_lowercase();
        self.version_picker_items
            .iter()
            .filter(|v| match v.channel {
                ModpackChannel::Release => true,
                ModpackChannel::Beta => self.version_picker_include_beta,
                ModpackChannel::Alpha => self.version_picker_include_alpha,
            })
            .filter(|v| {
                if mc_filter.is_empty()
                    || mc_filter.eq_ignore_ascii_case("all")
                    || v.game_versions.is_empty()
                {
                    true
                } else {
                    v.game_versions.iter().any(|gv| gv == mc_filter)
                }
            })
            .filter(|v| {
                if loader_filter.is_empty() || loader_filter == "all" || v.loaders.is_empty() {
                    true
                } else {
                    v.loaders
                        .iter()
                        .any(|l| l.eq_ignore_ascii_case(&loader_filter))
                }
            })
            .collect()
    }

    pub(crate) fn sync_mod_version_picker_selection(&mut self) {
        if self.filtered_mod_versions().is_empty() && !self.version_picker_items.is_empty() {
            // Auto-relax beta/alpha if the mod only has beta/alpha builds for the selected MC version/loader
            if !self.version_picker_include_beta {
                self.version_picker_include_beta = true;
            }
            if self.filtered_mod_versions().is_empty() && !self.version_picker_include_alpha {
                self.version_picker_include_alpha = true;
            }
        }
        let filtered_ids: Vec<String> = self
            .filtered_mod_versions()
            .into_iter()
            .map(|v| v.id.clone())
            .collect();
        if !filtered_ids.contains(&self.version_picker_selected_id) {
            self.version_picker_selected_id = filtered_ids.into_iter().next().unwrap_or_default();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn install_content_choice_to_target(
        &mut self,
        choice: ModVersionChoice,
        create_new: bool,
        target_instance_id: String,
        new_inst_name: String,
        new_mc_version: String,
        new_inst_loader: String,
        datapack_world: Option<String>,
        ctx: &egui::Context,
    ) {
        if choice.download_url.is_none() {
            self.notice_banner = Some((
                format!(
                    "'{}' has third-party distribution disabled by its author on CurseForge.",
                    choice.file_name
                ),
                true,
            ));
            return;
        }
        let content_kind = self
            .install_target_content_kind
            .or(self.version_picker_content_kind);
        let dest_inst = if create_new {
            let trimmed_mc = new_mc_version.trim();
            let mc_ver = if !trimmed_mc.is_empty() {
                trimmed_mc.to_string()
            } else {
                choice
                    .game_versions
                    .first()
                    .cloned()
                    .filter(|v| !v.trim().is_empty())
                    .or_else(|| {
                        let latest = self.latest_mc_version(false);
                        (!latest.is_empty()).then_some(latest)
                    })
                    .unwrap_or_else(|| "1.21.4".to_string())
            };

            let loader_name = if content_kind.is_none() {
                let l = new_inst_loader.trim().to_ascii_lowercase();
                if matches!(l.as_str(), "fabric" | "forge" | "neoforge" | "quilt") {
                    l
                } else {
                    choice
                        .loaders
                        .iter()
                        .find(|x| {
                            matches!(
                                x.to_ascii_lowercase().as_str(),
                                "fabric" | "forge" | "neoforge" | "quilt"
                            )
                        })
                        .map(|x| x.to_ascii_lowercase())
                        .unwrap_or_else(|| "fabric".to_string())
                }
            } else {
                let l = new_inst_loader.trim().to_ascii_lowercase();
                if matches!(
                    l.as_str(),
                    "vanilla" | "fabric" | "forge" | "neoforge" | "quilt"
                ) {
                    l
                } else {
                    "vanilla".to_string()
                }
            };
            let mod_loader = if loader_name == "vanilla" {
                ModLoader::vanilla()
            } else {
                ModLoader {
                    name: loader_name,
                    version: "latest".to_string(),
                }
            };
            let store = InstanceStore::new(&self.paths);
            let mut candidate = new_inst_name.trim().to_string();
            if candidate.is_empty() {
                candidate = self.version_picker_title.trim().to_string();
            }
            if candidate.is_empty() {
                candidate = format!("Minecraft {mc_ver}");
            }
            let base = candidate.clone();
            let mut idx = 2usize;
            while store.load_instance(&candidate).is_ok() {
                candidate = format!("{base} ({idx})");
                idx += 1;
            }
            match store.create_instance(&candidate, &mc_ver, mod_loader) {
                Ok((created, mut inst_settings)) => {
                    inst_settings.min_memory = self.prefs.default_min_memory;
                    inst_settings.max_memory = self.prefs.default_max_memory;
                    inst_settings.width = self.prefs.default_width;
                    inst_settings.height = self.prefs.default_height;
                    inst_settings.fullscreen = self.prefs.default_fullscreen;
                    if !self.prefs.default_java_path.trim().is_empty() {
                        inst_settings.java_path =
                            Some(PathBuf::from(self.prefs.default_java_path.trim()));
                    }
                    apply_lan_agent_option(
                        &mut inst_settings,
                        self.install_target_enable_lan_agent,
                    );
                    let _ = store.save_settings(&created.instance_id, &inst_settings);
                    self.selected_instance_id = Some(created.instance_id.clone());
                    self.reload_instances();
                    created
                }
                Err(err) => {
                    self.notice_banner = Some((format!("Create instance failed: {err}"), true));
                    return;
                }
            }
        } else {
            let found = self
                .instances
                .iter()
                .find(|i| i.instance_id == target_instance_id)
                .cloned()
                .or_else(|| self.selected_instance().cloned());
            let Some(inst) = found else {
                self.notice_banner =
                    Some(("Select or create an instance first.".to_string(), true));
                return;
            };
            inst
        };

        if let Some(kind) = content_kind {
            self.start_direct_content_pack_file_install(
                choice,
                dest_inst,
                kind,
                datapack_world,
                ctx,
            );
        } else {
            self.start_direct_mod_file_install_for(choice, dest_inst, ctx);
        }
    }

    pub(crate) fn start_direct_content_pack_file_install(
        &mut self,
        choice: ModVersionChoice,
        inst: Instance,
        kind: ContentKind,
        datapack_world: Option<String>,
        ctx: &egui::Context,
    ) {
        let Some(url) = choice.download_url else {
            return;
        };
        let kind_label = match kind {
            ContentKind::ResourcePack => "Resource Pack",
            ContentKind::ShaderPack => "Shader Pack",
            ContentKind::DataPack => "Data Pack",
        };
        let item_key = self.version_picker_key.clone();
        self.installing_content_packs.insert(item_key.clone());
        let (task_id, task_ctrl) = self.begin_task_with_control(
            format!("{kind_label}: {}", choice.file_name),
            format!("Downloading into {}...", inst.name),
        );
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let target_dir = if kind == ContentKind::DataPack {
            match datapack_world
                .as_deref()
                .map(str::trim)
                .filter(|w| !w.is_empty())
            {
                Some(world) => mc_dir.join("saves").join(world).join("datapacks"),
                None => mc_dir.join(kind.folder_name()),
            }
        } else {
            mc_dir.join(kind.folder_name())
        };
        let dest = target_dir.join(&choice.file_name);
        let file_name = choice.file_name.clone();
        let sha1 = choice.sha1;
        let file_size = choice.file_size;
        let downloader = self.download_client.with_task_control(task_ctrl);
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let _ = std::fs::create_dir_all(&target_dir);
                let req = mcw_net::DownloadRequest::new(url, dest)
                    .with_sha1(sha1)
                    .with_size(file_size);
                let prog_cb = make_task_byte_cb(
                    tx.clone(),
                    ctx_c.clone(),
                    task_id,
                    format!("Downloading {file_name}"),
                );
                downloader
                    .download_file_with_progress(&req, prog_cb)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<String, String>(file_name)
            }
            .await;
            let _ = tx.send(AppEvent::ContentPackInstallFinished {
                task_id,
                item_key,
                instance_id: inst.instance_id,
                kind,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_direct_mod_file_install(
        &mut self,
        choice: ModVersionChoice,
        ctx: &egui::Context,
    ) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        self.start_direct_mod_file_install_for(choice, inst, ctx);
    }

    pub(crate) fn is_instance_modpack_protected(&self, inst: &Instance) -> bool {
        let is_modpack = is_verified_or_imported_modpack(
            &self.paths.instance_dir(&inst.instance_id),
            &inst.icon_origin,
        );
        let settings = self
            .selected_settings
            .as_ref()
            .filter(|_| self.selected_instance_id.as_deref() == Some(inst.instance_id.as_str()))
            .or_else(|| self.settings_cache.get(&inst.instance_id));
        match settings {
            Some(s) => s.is_mod_repair_and_update_disabled(is_modpack),
            None => is_modpack,
        }
    }

    pub(crate) fn find_duplicate_mod_in_instance(
        &self,
        inst_id: &str,
        incoming_file_name: &str,
        project_id: Option<&str>,
        project_title: Option<&str>,
    ) -> Option<(String, String)> {
        let mods_dir = self.paths.minecraft_dir(inst_id).join("mods");
        let existing_files: Vec<String> = std::fs::read_dir(&mods_dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|n| {
                let l = n.to_ascii_lowercase();
                l.ends_with(".jar") || l.ends_with(".jar.disabled")
            })
            .collect();

        let strip_disabled = |name: &str| -> String {
            let lower = name.to_ascii_lowercase();
            lower
                .strip_suffix(".disabled")
                .unwrap_or(&lower)
                .to_string()
        };
        let incoming_base = strip_disabled(incoming_file_name);
        if let Some(exact) = existing_files
            .iter()
            .find(|f| strip_disabled(f) == incoming_base)
        {
            return Some((
                exact.clone(),
                "A mod with the same file name is already in this instance.".to_string(),
            ));
        }

        let inst_dir = self.paths.instance_dir(inst_id);
        let prov = ModProvenanceRegistry::load(&inst_dir);
        if let Some(pid) = project_id.filter(|s| !s.trim().is_empty()) {
            for entry in prov.mods.values() {
                if entry.project_id == pid {
                    let target = strip_disabled(&entry.file_name);
                    if let Some(on_disk) =
                        existing_files.iter().find(|f| strip_disabled(f) == target)
                    {
                        return Some((
                            on_disk.clone(),
                            format!("This mod project ('{}') is already installed.", entry.title),
                        ));
                    }
                }
            }
        }

        if let Some(title) = project_title
            .map(str::trim)
            .filter(|t| !t.is_empty() && t.len() >= 3)
        {
            for entry in prov.mods.values() {
                if entry.title.eq_ignore_ascii_case(title) {
                    let target = strip_disabled(&entry.file_name);
                    if let Some(on_disk) =
                        existing_files.iter().find(|f| strip_disabled(f) == target)
                    {
                        return Some((
                            on_disk.clone(),
                            format!("'{}' is already installed.", entry.title),
                        ));
                    }
                }
            }
        }

        let incoming_stem = normalize_mod_stem(incoming_file_name);
        if incoming_stem.len() >= 3 {
            if let Some(similar) = existing_files
                .iter()
                .find(|f| normalize_mod_stem(f) == incoming_stem)
            {
                return Some((
                    similar.clone(),
                    "Another version of this mod appears to be installed.".to_string(),
                ));
            }
        }
        None
    }

    pub(crate) fn start_direct_mod_file_install_for(
        &mut self,
        choice: ModVersionChoice,
        inst: Instance,
        ctx: &egui::Context,
    ) {
        if choice.download_url.is_none() {
            self.notice_banner = Some((
                format!(
                    "'{}' has third-party distribution disabled by its author on CurseForge.",
                    choice.file_name
                ),
                true,
            ));
            return;
        }
        let picker_key = self.version_picker_key.clone();
        let picker_title = self.version_picker_title.clone();
        let (provider, project_id) = if let Some(slug) = picker_key.strip_prefix("mr:") {
            (BrowserProvider::Modrinth, Some(slug.to_string()))
        } else if let Some(mid) = picker_key.strip_prefix("cf:") {
            (BrowserProvider::CurseForge, Some(mid.to_string()))
        } else {
            (self.browser_provider, None)
        };
        let proj_name = (!picker_title.trim().is_empty()).then_some(picker_title.clone());

        if let Some((existing_file, reason)) = self.find_duplicate_mod_in_instance(
            &inst.instance_id,
            &choice.file_name,
            project_id.as_deref(),
            proj_name.as_deref(),
        ) {
            self.pending_duplicate_mod_prompts
                .push(PendingDuplicateModPrompt {
                    instance_id: inst.instance_id,
                    instance_name: inst.name,
                    incoming_file_name: choice.file_name.clone(),
                    existing_file_name: existing_file,
                    match_reason: reason,
                    source: DuplicateModSource::Remote {
                        choice: Box::new(choice),
                        provider,
                        project_id,
                        project_name: proj_name,
                    },
                });
            return;
        }

        let target_file = choice.file_name.clone();
        self.execute_remote_mod_file_install(
            inst,
            choice,
            target_file,
            None,
            provider,
            project_id,
            proj_name,
            ctx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_remote_mod_file_install(
        &mut self,
        inst: Instance,
        choice: ModVersionChoice,
        dest_file_name: String,
        remove_existing_file: Option<String>,
        provider: BrowserProvider,
        project_id: Option<String>,
        project_name: Option<String>,
        ctx: &egui::Context,
    ) {
        let Some(url) = choice.download_url else {
            self.notice_banner = Some((
                format!(
                    "'{}' has third-party distribution disabled by its author on CurseForge.",
                    dest_file_name
                ),
                true,
            ));
            return;
        };
        let (task_id, task_ctrl) = self.begin_task_with_control(
            format!("Mod: {dest_file_name}"),
            format!("Downloading into {}...", inst.name),
        );
        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let mods_dir = self.paths.minecraft_dir(&inst.instance_id).join("mods");
        let dest = mods_dir.join(&dest_file_name);
        let cf_api_key = self.resolved_cf_api_key();
        let cache_path = self.paths.cache_root.join("providers.sqlite");
        let picker_title = project_name.unwrap_or_else(|| choice.title.clone());
        let downloader = self.download_client.with_task_control(task_ctrl);
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                if let Some(old_name) = remove_existing_file {
                    remove_mod_file_and_twins(&mods_dir, &old_name);
                }
                let req = mcw_net::DownloadRequest::new(url, dest)
                    .with_sha1(choice.sha1)
                    .with_size(choice.file_size);
                let prog_cb = make_task_byte_cb(
                    tx.clone(),
                    ctx.clone(),
                    task_id,
                    format!("Downloading {dest_file_name}"),
                );
                downloader
                    .download_file_with_progress(&req, prog_cb)
                    .await
                    .map_err(|e| e.to_string())?;

                match provider {
                    BrowserProvider::Modrinth => {
                        if let Some(slug) = project_id {
                            let _ = ModProvenanceRegistry::record_install(
                                &inst_dir,
                                TrackedModEntry {
                                    project_id: slug,
                                    provider: "modrinth".to_string(),
                                    title: picker_title,
                                    file_name: dest_file_name.clone(),
                                    version_id: choice.id.clone(),
                                    version_number: choice.version_number.clone(),
                                    locked: false,
                                    managed_by: None,
                                },
                            );
                        }
                    }
                    BrowserProvider::CurseForge => {
                        if let Some(mid_str) = project_id {
                            let _ = ModProvenanceRegistry::record_install(
                                &inst_dir,
                                TrackedModEntry {
                                    project_id: mid_str.clone(),
                                    provider: "curseforge".to_string(),
                                    title: picker_title,
                                    file_name: dest_file_name.clone(),
                                    version_id: choice.id.clone(),
                                    version_number: choice.version_number.clone(),
                                    locked: false,
                                    managed_by: None,
                                },
                            );
                            if let (Ok(mod_id), Ok(file_id)) =
                                (mid_str.parse::<u64>(), choice.id.parse::<u64>())
                            {
                                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                                if let Ok(cf) = CurseForgeClient::new(cf_api_key, cache) {
                                    let _ = cf
                                        .install_required_dependencies_for_file(
                                            mod_id,
                                            file_id,
                                            &inst.version_id,
                                            &inst.mod_loader.name,
                                            &mods_dir,
                                            &downloader,
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                    _ => {}
                }
                Ok::<String, String>(dest_file_name)
            }
            .await;
            let _ = tx.send(AppEvent::DirectModFileInstalled {
                task_id,
                instance_id: inst.instance_id,
                result: res,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn confirm_duplicate_mod_action(&mut self, action: u8, ctx: &egui::Context) {
        if self.pending_duplicate_mod_prompts.is_empty() {
            return;
        }
        let prompt = self.pending_duplicate_mod_prompts.remove(0);
        if action == 0 {
            self.notice_banner = Some((
                format!(
                    "Cancelled adding duplicate mod '{}'.",
                    prompt.incoming_file_name
                ),
                false,
            ));
            return;
        }
        let mods_dir = self.paths.minecraft_dir(&prompt.instance_id).join("mods");
        let _ = std::fs::create_dir_all(&mods_dir);

        match prompt.source {
            DuplicateModSource::LocalFile { source_path } => {
                let target_name = if action == 1 {
                    remove_mod_file_and_twins(&mods_dir, &prompt.existing_file_name);
                    prompt.incoming_file_name.clone()
                } else {
                    resolve_unique_mod_filename(&mods_dir, &prompt.incoming_file_name)
                };
                match std::fs::copy(&source_path, mods_dir.join(&target_name)) {
                    Ok(_) => {
                        self.mods_cache.remove(&prompt.instance_id);
                        self.load_mods_async(&prompt.instance_id, ctx, true);
                        let msg = if action == 1 {
                            format!(
                                "Replaced '{}' with '{}' in '{}'.",
                                prompt.existing_file_name, target_name, prompt.instance_name
                            )
                        } else {
                            format!(
                                "Added duplicate mod as '{}' in '{}'.",
                                target_name, prompt.instance_name
                            )
                        };
                        self.push_global_log(msg.clone());
                        self.notice_banner = Some((msg, false));
                    }
                    Err(err) => {
                        self.notice_banner =
                            Some((format!("Failed to import mod file: {err}"), true));
                    }
                }
            }
            DuplicateModSource::Remote {
                choice,
                provider,
                project_id,
                project_name,
            } => {
                let inst = self
                    .instances
                    .iter()
                    .find(|i| i.instance_id == prompt.instance_id)
                    .cloned()
                    .or_else(|| {
                        InstanceStore::new(&self.paths)
                            .load_instance(&prompt.instance_id)
                            .ok()
                    });
                let Some(inst) = inst else {
                    self.notice_banner = Some((
                        format!(
                            "Target instance '{}' no longer exists.",
                            prompt.instance_name
                        ),
                        true,
                    ));
                    return;
                };
                if action == 1 {
                    self.execute_remote_mod_file_install(
                        inst,
                        *choice,
                        prompt.incoming_file_name,
                        Some(prompt.existing_file_name),
                        provider,
                        project_id,
                        project_name,
                        ctx,
                    );
                } else {
                    let unique_name =
                        resolve_unique_mod_filename(&mods_dir, &prompt.incoming_file_name);
                    self.execute_remote_mod_file_install(
                        inst,
                        *choice,
                        unique_name,
                        None,
                        provider,
                        project_id,
                        project_name,
                        ctx,
                    );
                }
            }
        }
    }

    pub(crate) fn start_check_mod_updates(&mut self, ctx: &egui::Context) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        if self.checking_mod_updates {
            return;
        }
        self.checking_mod_updates = true;
        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let cache_path = self.paths.cache_root.join("providers.sqlite");
        let cf_key = self.resolved_cf_api_key();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
            let candidates = if let Ok(mr) = ModrinthClient::new(cache.clone()) {
                let cf = CurseForgeClient::new(cf_key, cache).ok();
                ModUpdateManager::check_updates(
                    &inst_dir,
                    &mc_dir,
                    &inst.version_id,
                    &inst.mod_loader.name,
                    &mr,
                    cf.as_ref(),
                )
                .await
            } else {
                Vec::new()
            };
            let _ = tx.send(AppEvent::ModUpdatesChecked {
                instance_id: inst.instance_id,
                candidates,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_apply_mod_updates(&mut self, ctx: &egui::Context) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        let candidates = self.mod_update_candidates.clone();
        if !candidates.iter().any(|c| c.update_available && !c.locked) {
            return;
        }
        let (task_id, task_ctrl) = self.begin_task_with_control(
            format!("Update Mods ({})", inst.name),
            "Downloading mod updates...",
        );
        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let downloader = self.download_client.with_task_control(task_ctrl);
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                ModUpdateManager::apply_updates(&inst_dir, &mc_dir, &candidates, &downloader)
                    .await
                    .map_err(|e| e.to_string())
            }
            .await;
            let _ = tx.send(AppEvent::ModUpdatesApplied {
                task_id,
                instance_id: inst.instance_id,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn resolved_cf_api_key(&self) -> String {
        load_curseforge_api_key(Some(&self.paths.project_root)).unwrap_or_default()
    }

    pub(crate) fn start_java_install(&mut self, major: u32, ctx: &egui::Context) {
        if !self.installing_java_majors.insert(major) {
            return;
        }
        let task_id = self.begin_task(
            format!("Temurin Java {major}"),
            "Downloading & extracting managed runtime...",
        );
        let runtimes_root = self.paths.runtimes_root.clone();
        let client = self.download_client.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let prog_cb = make_task_byte_cb(
                    tx.clone(),
                    ctx.clone(),
                    task_id,
                    format!("Downloading Temurin Java {major}"),
                );
                JavaProvisioner::install_managed_exact_with_progress(
                    &runtimes_root,
                    major,
                    true,
                    &client,
                    prog_cb,
                )
                .await
                .map_err(|e| e.to_string())
            }
            .await;
            let _ = tx.send(AppEvent::JavaInstallFinished {
                task_id,
                major,
                result: res,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_repair(&mut self, ctx: &egui::Context) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        if self.is_instance_busy(&inst.instance_id) {
            return;
        }
        self.notice_banner = None;
        self.launch_states.insert(
            inst.instance_id.clone(),
            LaunchState::Preparing(ProgressEvent {
                stage: mcw_models::progress::ProgressStage::FetchingManifest,
                message: "Starting repair...".to_string(),
                current: 0,
                total: 1,
            }),
        );
        let paths = self.paths.clone();
        let client = self.download_client.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let svc = RepairService::new(&paths, &client);
                let sink = Arc::new(ChannelProgressSink {
                    instance_id: inst.instance_id.clone(),
                    tx: tx.clone(),
                    ctx: ctx.clone(),
                });
                svc.repair_instance(&inst, sink)
                    .await
                    .map_err(|e| e.to_string())
            }
            .await;
            let _ = tx.send(AppEvent::RepairFinished {
                instance_id: inst.instance_id,
                report: res,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_backup_create(&mut self, ctx: &egui::Context) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        self.start_backup_create_for(inst.instance_id, ctx);
    }

    pub(crate) fn start_backup_create_for(&mut self, instance_id: String, ctx: &egui::Context) {
        if self.backup_busy || self.is_instance_busy(&instance_id) {
            return;
        }
        self.backup_busy = true;
        let note = self.backup_note_input.trim().to_string();
        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let id = instance_id.clone();
            let res = tokio::task::spawn_blocking(move || {
                BackupManager::new(&paths)
                    .create_backup(&instance_id, note)
                    .map_err(|e| e.to_string())
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            let _ = tx.send(AppEvent::BackupCreated {
                instance_id: id,
                result: res,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_backup_restore(&mut self, backup_id: String, ctx: &egui::Context) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        if self.backup_busy || self.is_instance_busy(&inst.instance_id) {
            return;
        }
        self.backup_busy = true;
        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let id = inst.instance_id.clone();
            let res = tokio::task::spawn_blocking(move || {
                BackupManager::new(&paths)
                    .restore_backup(&inst.instance_id, &backup_id)
                    .map_err(|e| e.to_string())
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            let _ = tx.send(AppEvent::BackupRestored {
                instance_id: id,
                result: res,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_modpack_install(&mut self, ctx: &egui::Context) {
        let archive = PathBuf::from(self.modpack_path_input.trim());
        if !archive.is_file() {
            self.notice_banner = Some((
                "Select a valid .mrpack or CurseForge .zip file path.".to_string(),
                true,
            ));
            return;
        }
        self.installing_archive_count += 1;
        self.modpack_manual_downloads.clear();
        let name_override = self.modpack_name_input.trim().to_string();
        let label_name = if !name_override.is_empty() {
            name_override.clone()
        } else {
            archive
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("modpack")
                .to_string()
        };
        let task_id = self.begin_task(
            format!("Import: {label_name}"),
            "Reading modpack archive...",
        );
        let paths = self.paths.clone();
        let install_opts = ModpackInstallOptions {
            include_optional: self.archive_include_optional,
            selected_optional_paths: if self.archive_optional_files.is_empty() {
                None
            } else {
                Some(self.archive_selected_optional_ids.clone())
            },
        };
        let enable_lan_agent = self.archive_enable_lan_agent;
        let downloader = self.download_client.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let tx_prog = tx.clone();
            let ctx_prog = ctx.clone();
            let progress: ModpackProgressCallback = Arc::new(move |current, total, message| {
                let _ = tx_prog.send(AppEvent::TaskProgress {
                    task_id,
                    message,
                    current,
                    total,
                });
                ctx_prog.request_repaint();
            });

            let res = async {
                let cache_path = paths.cache_root.join("providers").join("api_cache.sqlite");
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let cf = CurseForgeClient::from_root(&paths.project_root, cache).ok();
                let opt_name = if name_override.is_empty() {
                    None
                } else {
                    Some(name_override.as_str())
                };
                let rep = ModpackInstaller::install_from_archive_with_options(
                    &paths,
                    &archive,
                    opt_name,
                    &downloader,
                    cf.as_ref(),
                    &install_opts,
                    Some(progress),
                )
                .await
                .map_err(|e| e.to_string())?;
                if enable_lan_agent {
                    let store = InstanceStore::new(&paths);
                    if let Ok(mut settings) = store.load_settings(&rep.instance_id) {
                        apply_lan_agent_option(&mut settings, true);
                        let _ = store.save_settings(&rep.instance_id, &settings);
                    }
                }
                Ok::<_, String>(rep)
            }
            .await;
            let _ = tx.send(AppEvent::ModpackInstalled {
                task_id,
                cf_pack_id: None,
                result: res,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_modrinth_pack_install(
        &mut self,
        slug: String,
        title: String,
        version_id: Option<String>,
        ctx: &egui::Context,
    ) {
        let pack_key = format!("mr:{slug}");
        if !self.installing_ext_packs.insert(pack_key.clone()) {
            return;
        }
        let task_id = self.begin_task(
            format!("Modrinth Modpack: {title}"),
            "Fetching Modrinth .mrpack...",
        );
        let paths = self.paths.clone();
        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let downloader = self.download_client.clone();

        self.runtime.spawn(async move {
            let tx_p = tx.clone();
            let ctx_p = ctx.clone();
            let cb: ModpackProgressCallback = Arc::new(move |cur, tot, msg| {
                let _ = tx_p.send(AppEvent::TaskProgress {
                    task_id,
                    current: cur,
                    total: tot,
                    message: msg,
                });
                ctx_p.request_repaint();
            });

            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let mr = ModrinthClient::new(cache).map_err(|e| e.to_string())?;
                let ver = if let Some(vid) = version_id {
                    mr.get_version(&vid).await.map_err(|e| e.to_string())?
                } else {
                    let versions = mr
                        .list_versions(&slug, None, None)
                        .await
                        .map_err(|e| e.to_string())?;
                    versions
                        .into_iter()
                        .next()
                        .ok_or_else(|| format!("No downloadable versions found for '{title}'"))?
                };
                let primary = ver
                    .files
                    .iter()
                    .find(|f| f.primary)
                    .or_else(|| ver.files.first())
                    .ok_or_else(|| "Selected Modrinth version has no .mrpack file".to_string())?;

                let mrpack_dir = paths.cache_root.join("modpacks");
                let _ = std::fs::create_dir_all(&mrpack_dir);
                let archive_path = mrpack_dir.join(&primary.filename);

                cb(0, 1, format!("Downloading {}...", primary.filename));
                let prog_cb = make_task_byte_cb(
                    tx.clone(),
                    ctx.clone(),
                    task_id,
                    format!("Downloading {}", primary.filename),
                );
                downloader
                    .download_file_with_progress(
                        &mcw_net::DownloadRequest::new(primary.url.clone(), archive_path.clone()),
                        prog_cb,
                    )
                    .await
                    .map_err(|e| e.to_string())?;

                let report = ModpackInstaller::install_from_archive_with_progress(
                    &paths,
                    &archive_path,
                    Some(&title),
                    &downloader,
                    None,
                    Some(cb),
                )
                .await
                .map_err(|e| e.to_string())?;
                let _ = ModrinthPackRegistry::set_origin_metadata(
                    &paths.instance_dir(&report.instance_id),
                    &ver.project_id,
                    &slug,
                    &ver.id,
                    &ver.version_number,
                );
                Ok(report)
            }
            .await;

            let _ = tx.send(AppEvent::ExternalModpackInstalled {
                task_id,
                pack_key,
                result: res,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_instance_export(
        &mut self,
        instance_id: String,
        options: ModpackExportOptions,
        ctx: &egui::Context,
    ) {
        let (filename, filter) = match options.mode {
            ModpackExportMode::Mrpack => (
                format!("{instance_id}.mrpack"),
                "Modrinth Modpack (*.mrpack)|*.mrpack|All Files (*.*)|*.*",
            ),
            ModpackExportMode::ProviderProfile => (
                format!("{instance_id}-MCW-Profile.zip"),
                "Zip Archive (*.zip)|*.zip|All Files (*.*)|*.*",
            ),
            ModpackExportMode::Portable { .. } => (
                format!("{instance_id}.mcwpack"),
                "MCW Portable Pack (*.mcwpack)|*.mcwpack|All Files (*.*)|*.*",
            ),
        };
        let Some(output_path) = save_file_dialog("Export Modpack", filter, &filename) else {
            return;
        };
        let display_name = output_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&filename)
            .to_string();
        let task_id = self.begin_task(
            format!("Export: {display_name}"),
            "Packaging instance files...",
        );
        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = tokio::task::spawn_blocking(move || {
                ModpackInstaller::export_instance(&paths, &instance_id, &output_path, &options)
                    .map(|count| (output_path, count))
                    .map_err(|e| e.to_string())
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));

            let _ = tx.send(AppEvent::MrpackExportFinished {
                task_id,
                result: res,
            });
            ctx.request_repaint();
        });
    }
}

fn progress_log_line(message: &str, current: u64, total: u64) -> Option<String> {
    if message.trim().is_empty() {
        return None;
    }
    if total > 20
        && (message.starts_with("Assets ")
            || message.starts_with("Libraries ")
            || message.starts_with("Downloading update ("))
    {
        let pct = current.saturating_mul(100) / total.max(1);
        let bucket = if current == 0 {
            0
        } else if current >= total {
            100
        } else {
            (pct / 25) * 25
        };
        if message.starts_with("Downloading update (") {
            return Some(format!("Downloading update ({bucket}%)"));
        }
        if let Some((prefix, _)) = message.split_once(' ') {
            return Some(format!("{prefix}: {bucket}% ({total} total)"));
        }
    }
    Some(message.to_string())
}

impl McwApp {
    pub(crate) fn poll_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                AppEvent::VersionsLoaded(res) => {
                    self.versions_loading = false;
                    match res {
                        Ok(list) => {
                            self.push_global_log(format!(
                                "[INFO] Loaded Minecraft version manifest ({} versions).",
                                list.len()
                            ));
                            self.available_versions = list;
                            let latest = self.latest_mc_version(self.create_show_snapshots);
                            if !latest.is_empty()
                                && (self.create_version.is_empty() || !self.show_create_modal)
                            {
                                self.create_version = latest;
                            }
                        }
                        Err(err) => {
                            self.push_global_log(format!(
                                "[ERROR] Failed to load Minecraft version manifest: {err}"
                            ));
                        }
                    }
                }
                AppEvent::ModpackInstallVersionsLoaded {
                    request_id,
                    versions,
                } => {
                    if self.modpack_dialog.request_id == request_id {
                        self.modpack_dialog.loading_versions = false;
                        match versions {
                            Ok(list) => {
                                self.push_global_log(format!(
                                    "[NET] Loaded {} modpack version(s) (req #{request_id}).",
                                    list.len()
                                ));
                                self.modpack_dialog.error = None;
                                self.modpack_dialog.versions = list;
                                self.sync_modpack_dialog_selection();
                            }
                            Err(err) => {
                                self.push_global_log(format!(
                                    "[ERROR] Failed to load modpack versions (req #{request_id}): {err}"
                                ));
                                self.modpack_dialog.error = Some(err);
                            }
                        }
                    }
                }
                AppEvent::ThumbnailLoaded { url, size, rgba } => {
                    self.loading_thumbs.remove(&url);
                    let img = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                    let handle = ctx.load_texture(url.clone(), img, egui::TextureOptions::LINEAR);
                    self.thumb_lru_order.retain(|k| k != &url);
                    self.thumb_lru_order.push(url.clone());
                    self.thumb_textures.insert(url, handle);
                    const MAX_THUMB_TEXTURES: usize = 64;
                    while self.thumb_textures.len() > MAX_THUMB_TEXTURES {
                        if let Some(oldest) = self.thumb_lru_order.first().cloned() {
                            self.thumb_lru_order.remove(0);
                            self.thumb_textures.remove(&oldest);
                        } else {
                            break;
                        }
                    }
                }
                AppEvent::ModVersionsLoaded {
                    target_key,
                    versions,
                } => {
                    if self.version_picker_key == target_key {
                        self.version_picker_loading = false;
                        match versions {
                            Ok(list) => {
                                self.push_global_log(format!(
                                    "[NET] Loaded {} file version(s) for '{target_key}'.",
                                    list.len()
                                ));
                                self.version_picker_items = list;
                                self.version_picker_error = None;
                                self.sync_mod_version_picker_selection();
                            }
                            Err(err) => {
                                self.push_global_log(format!(
                                    "[ERROR] Failed to load versions for '{target_key}': {err}"
                                ));
                                self.version_picker_error = Some(err);
                            }
                        }
                    }
                }
                AppEvent::DirectModFileInstalled {
                    task_id,
                    instance_id,
                    result,
                } => {
                    self.finish_task(task_id);
                    match result {
                        Ok(file_name) => {
                            self.mods_cache.remove(&instance_id);
                            if self.selected_instance_id.as_deref() == Some(instance_id.as_str())
                                && (self.active_tab == DetailTab::Mods
                                    || (self.revealed_tab_counts_instance.as_deref()
                                        == Some(instance_id.as_str())
                                        && self.revealed_tab_counts.contains_key(&DetailTab::Mods)))
                            {
                                self.load_mods_async(&instance_id, ctx, true);
                            }
                            let msg = format!("Installed '{file_name}' into '{instance_id}'.");
                            self.push_global_log(format!("[MOD] {msg}"));
                            self.notice_banner = Some((format!("Installed '{file_name}'."), false));
                        }
                        Err(err) => {
                            self.push_global_log(format!(
                                "[ERROR] Mod install failed for '{instance_id}': {err}"
                            ));
                            self.notice_banner = Some((format!("Mod install failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ModUpdatesChecked {
                    instance_id,
                    candidates,
                } => {
                    self.checking_mod_updates = false;
                    let avail = candidates.iter().filter(|c| c.update_available).count();
                    self.push_global_log(format!(
                        "[MOD] Checked tracked mods for '{instance_id}': {avail}/{} update(s) available.",
                        candidates.len()
                    ));
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        self.mod_update_candidates = candidates;
                        self.notice_banner = Some((
                            format!("Checked tracked mods: {avail} update(s) available."),
                            false,
                        ));
                    }
                }
                AppEvent::ModUpdatesApplied {
                    task_id,
                    instance_id,
                    result,
                } => {
                    self.finish_task(task_id);
                    match result {
                        Ok(count) => {
                            self.mods_cache.remove(&instance_id);
                            self.mod_update_candidates.clear();
                            if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                                self.load_mods_async(&instance_id, ctx, true);
                            }
                            self.push_global_log(format!(
                                "[MOD] Updated {count} mod(s) in '{instance_id}'."
                            ));
                            self.notice_banner =
                                Some((format!("Updated {count} mod(s) successfully."), false));
                        }
                        Err(err) => {
                            self.push_global_log(format!(
                                "[ERROR] Mod update failed in '{instance_id}': {err}"
                            ));
                            self.notice_banner = Some((format!("Mod update failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ContentPackUpdatesChecked {
                    instance_id,
                    kind,
                    candidates,
                } => {
                    self.checking_pack_updates = false;
                    let avail = candidates.iter().filter(|c| c.update_available).count();
                    self.push_global_log(format!(
                        "[PACK] Checked {} in '{instance_id}': {avail}/{} update(s) available.",
                        kind.folder_name(),
                        candidates.len()
                    ));
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        self.pack_updates_kind = Some(kind);
                        self.pack_update_candidates = candidates;
                        self.notice_banner = Some((
                            format!(
                                "Checked {}: {avail} Modrinth update(s) available.",
                                kind.folder_name()
                            ),
                            false,
                        ));
                    }
                }
                AppEvent::ContentPackUpdatesApplied {
                    task_id,
                    instance_id,
                    kind,
                    result,
                } => {
                    self.finish_task(task_id);
                    match result {
                        Ok(count) => {
                            self.content_packs_cache
                                .remove(&(instance_id.clone(), kind));
                            self.pack_icon_textures.clear();
                            if self.pack_updates_kind == Some(kind) {
                                self.pack_update_candidates.clear();
                            }
                            if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                                self.load_content_packs_async(&instance_id, kind, ctx, true);
                            }
                            let msg = format!(
                                "Updated {count} pack(s) in '{}/{}'.",
                                instance_id,
                                kind.folder_name()
                            );
                            self.push_global_log(format!("[PACK] {msg}"));
                            self.notice_banner = Some((msg, false));
                        }
                        Err(err) => {
                            self.push_global_log(format!(
                                "[ERROR] Pack update failed in '{instance_id}': {err}"
                            ));
                            self.notice_banner = Some((format!("Pack update failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ModsLoaded { instance_id, mods } => {
                    self.loading_mods.remove(&instance_id);
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        self.selected_mods = mods.clone();
                        if self.revealed_tab_counts_instance.as_deref()
                            == Some(instance_id.as_str())
                            && (self.active_tab == DetailTab::Mods
                                || self.revealed_tab_counts.contains_key(&DetailTab::Mods))
                        {
                            self.revealed_tab_counts.insert(DetailTab::Mods, mods.len());
                        }
                    }
                    self.mods_cache.insert(instance_id, mods);
                    self.refresh_mod_compat_report();
                }
                AppEvent::BackupsLoaded {
                    instance_id,
                    backups,
                } => {
                    self.loading_backups.remove(&instance_id);
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        self.selected_backups = backups.clone();
                        if self.revealed_tab_counts_instance.as_deref()
                            == Some(instance_id.as_str())
                            && (self.active_tab == DetailTab::Backups
                                || self.revealed_tab_counts.contains_key(&DetailTab::Backups))
                        {
                            self.revealed_tab_counts
                                .insert(DetailTab::Backups, backups.len());
                        }
                    }
                    self.backups_cache.insert(instance_id, backups);
                }
                AppEvent::WorldsLoaded {
                    instance_id,
                    worlds,
                } => {
                    self.loading_worlds.remove(&instance_id);
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        self.selected_worlds = worlds.clone();
                        if self.revealed_tab_counts_instance.as_deref()
                            == Some(instance_id.as_str())
                            && (self.active_tab == DetailTab::Worlds
                                || self.revealed_tab_counts.contains_key(&DetailTab::Worlds))
                        {
                            self.revealed_tab_counts
                                .insert(DetailTab::Worlds, worlds.len());
                        }
                    }
                    self.worlds_cache.insert(instance_id, worlds);
                }
                AppEvent::ScreenshotsLoaded {
                    instance_id,
                    screenshots,
                } => {
                    self.loading_screenshots.remove(&instance_id);
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        self.selected_screenshots = screenshots.clone();
                        if self.revealed_tab_counts_instance.as_deref()
                            == Some(instance_id.as_str())
                            && (self.active_tab == DetailTab::Screenshots
                                || self
                                    .revealed_tab_counts
                                    .contains_key(&DetailTab::Screenshots))
                        {
                            self.revealed_tab_counts
                                .insert(DetailTab::Screenshots, screenshots.len());
                        }
                    }
                    self.screenshots_cache.insert(instance_id, screenshots);
                }
                AppEvent::ContentPacksLoaded {
                    instance_id,
                    kind,
                    packs,
                } => {
                    self.loading_content_packs
                        .remove(&(instance_id.clone(), kind));
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        if self.active_tab.content_kind() == Some(kind) {
                            self.selected_content_packs = packs.clone();
                        }
                        let pack_tab = match kind {
                            ContentKind::ResourcePack => Some(DetailTab::ResourcePacks),
                            ContentKind::ShaderPack => Some(DetailTab::Shaders),
                            _ => None,
                        };
                        if let Some(tab) = pack_tab {
                            if self.revealed_tab_counts_instance.as_deref()
                                == Some(instance_id.as_str())
                                && (self.active_tab == tab
                                    || self.revealed_tab_counts.contains_key(&tab))
                            {
                                self.revealed_tab_counts.insert(tab, packs.len());
                            }
                        }
                    }
                    self.content_packs_cache.insert((instance_id, kind), packs);
                }
                AppEvent::ContentPackInstallFinished {
                    task_id,
                    item_key,
                    instance_id,
                    kind,
                    result,
                } => {
                    self.finish_task(task_id);
                    self.installing_content_packs.remove(&item_key);
                    match result {
                        Ok(file_name) => {
                            self.content_packs_cache
                                .remove(&(instance_id.clone(), kind));
                            let pack_tab = match kind {
                                ContentKind::ResourcePack => Some(DetailTab::ResourcePacks),
                                ContentKind::ShaderPack => Some(DetailTab::Shaders),
                                _ => None,
                            };
                            if self.selected_instance_id.as_deref() == Some(instance_id.as_str())
                                && (self.active_tab.content_kind() == Some(kind)
                                    || pack_tab.is_some_and(|t| {
                                        self.revealed_tab_counts_instance.as_deref()
                                            == Some(instance_id.as_str())
                                            && self.revealed_tab_counts.contains_key(&t)
                                    }))
                            {
                                self.load_content_packs_async(&instance_id, kind, ctx, true);
                            }
                            self.push_global_log(format!(
                                "[CONTENT] Installed {:?} '{file_name}' into '{instance_id}'.",
                                kind
                            ));
                            self.notice_banner = Some((format!("Installed '{file_name}'."), false));
                        }
                        Err(err) => {
                            self.push_global_log(format!(
                                "[ERROR] Pack install failed for '{instance_id}': {err}"
                            ));
                            self.notice_banner =
                                Some((format!("Pack install failed: {err}"), true));
                        }
                    }
                }
                AppEvent::McLogsUploaded(result) => {
                    self.uploading_mclogs = false;
                    match result {
                        Ok(res) => {
                            ctx.copy_text(res.url.clone());
                            self.last_mclogs_url = Some(res.url.clone());
                            self.push_global_log(format!(
                                "[INFO] Uploaded sanitized log to {}.",
                                res.url
                            ));
                            self.notice_banner = Some((
                                format!(
                                    "Uploaded sanitized log to {} (copied to clipboard).",
                                    res.url
                                ),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.push_global_log(format!("[ERROR] mclo.gs upload failed: {err}"));
                            self.notice_banner =
                                Some((format!("mclo.gs upload failed: {err}"), true));
                        }
                    }
                }
                AppEvent::TaskProgress {
                    task_id,
                    message,
                    current,
                    total,
                } => {
                    if let Some(log_line) = progress_log_line(&message, current, total) {
                        if self.last_task_log_msg.get(&task_id) != Some(&log_line) {
                            self.last_task_log_msg.insert(task_id, log_line.clone());
                            self.push_global_log(format!("[TASK] #{task_id}: {log_line}"));
                        }
                    }
                    if let Some(task) = self.active_tasks.iter_mut().find(|t| t.id == task_id) {
                        let next_is_bytes = total >= 16_384 || message.contains(" MB)");
                        task.record_progress(message, current, total, next_is_bytes);
                    }
                }
                AppEvent::LaunchProgress { instance_id, event } => {
                    if let Some(log_line) =
                        progress_log_line(&event.message, event.current, event.total)
                    {
                        let formatted = format!("[{:?}] {log_line}", event.stage);
                        if self.last_launch_log_msg.get(&instance_id) != Some(&formatted) {
                            self.last_launch_log_msg
                                .insert(instance_id.clone(), formatted.clone());
                            self.push_global_log(format!("[LAUNCH:{instance_id}] {formatted}"));
                        }
                    }
                    if let Some(&tid) = self.launch_task_ids.get(&instance_id) {
                        if let Some(task) = self.active_tasks.iter_mut().find(|t| t.id == tid) {
                            let next_is_bytes = matches!(
                                event.stage,
                                mcw_models::progress::ProgressStage::DownloadingClient
                                    | mcw_models::progress::ProgressStage::InstallingJava
                                    | mcw_models::progress::ProgressStage::InstallingModLoader
                            ) && event.total >= 16_384;
                            task.record_progress(
                                format!("[{:?}] {}", event.stage, event.message),
                                event.current,
                                event.total,
                                next_is_bytes,
                            );
                        }
                    }
                    self.launch_states
                        .insert(instance_id, LaunchState::Preparing(event));
                }
                AppEvent::LaunchStarted {
                    instance_id,
                    pid,
                    control,
                } => {
                    self.last_launch_log_msg.remove(&instance_id);
                    if let Some(tid) = self.launch_task_ids.remove(&instance_id) {
                        self.finish_task(tid);
                    }
                    if let Some(inst) = self.instances.iter().find(|i| i.instance_id == instance_id)
                    {
                        self.discord_rpc.on_game_started(
                            inst.name.clone(),
                            inst.version_id.clone(),
                            inst.mod_loader.name.clone(),
                        );
                    }
                    GameWindowActivator::allow_foreground(pid);
                    GameWindowActivator::focus_process_window_delayed(pid);
                    if self.prefs.minimize_on_launch {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                    self.push_global_log(format!("Launched instance '{instance_id}' (PID {pid})."));
                    self.launch_states
                        .insert(instance_id, LaunchState::Running { pid, control });
                }
                AppEvent::LogReceived { instance_id, line } => {
                    if line.text.starts_with("[LaunchCheck]")
                        || line.text.starts_with("[ForgePreflight]")
                        || line.text.starts_with("[Security]")
                    {
                        self.push_global_log(format!("[{instance_id}] {}", line.text));
                    }
                    if line.text.contains("[MCW Security Agent]") {
                        self.security_assessments.remove(&instance_id);
                    }
                    let entry = self.instance_logs.entry(instance_id).or_default();
                    if entry.len() >= MAX_LOG_LINES_PER_INSTANCE {
                        entry.remove(0);
                    }
                    entry.push(line);
                }
                AppEvent::LaunchFinished {
                    instance_id,
                    report,
                } => {
                    self.last_launch_log_msg.remove(&instance_id);
                    if let Some(tid) = self.launch_task_ids.remove(&instance_id) {
                        self.finish_task(tid);
                    }
                    self.launch_states
                        .insert(instance_id.clone(), LaunchState::Idle);
                    let any_running = self
                        .launch_states
                        .values()
                        .any(|s| matches!(s, LaunchState::Running { .. }));
                    if !any_running {
                        self.discord_rpc.on_game_stopped();
                        if self.prefs.minimize_on_launch {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        }
                    }
                    self.reload_instances();
                    match &report {
                        Ok(rep) => {
                            self.push_global_log(format!(
                                "Instance '{instance_id}' exited ({:?}, {}s).",
                                rep.classification, rep.duration_secs
                            ));
                        }
                        Err(err) => {
                            self.push_global_log(format!(
                                "[ERROR] Instance '{instance_id}' launch failed: {err}"
                            ));
                        }
                    }
                    match report {
                        Ok(rep) => match rep.classification {
                            ExitClassification::Normal => {
                                self.crash_summaries.remove(&instance_id);
                                self.notice_banner = Some((
                                    format!("Game closed cleanly ({}s).", rep.duration_secs),
                                    false,
                                ));
                            }
                            ExitClassification::StoppedByUser => {
                                self.crash_summaries.remove(&instance_id);
                                self.notice_banner =
                                    Some(("Game stopped by user.".to_string(), false));
                            }
                            ExitClassification::Crashed => {
                                let diag = LaunchErrorPresenter::present(
                                    &format!("exit code {:?}", rep.exit_code),
                                    rep.unsupported_class_version,
                                );
                                self.notice_banner = Some((diag.banner_text(), true));

                                let mut analysis_lines: Vec<String> = self
                                    .instance_logs
                                    .get(&instance_id)
                                    .map(|v| v.iter().map(|l| l.text.clone()).collect())
                                    .unwrap_or_default();
                                let mc_dir = self.paths.minecraft_dir(&instance_id);
                                if let Some(crash_file) = find_latest_crash_report(&mc_dir) {
                                    if let Ok(raw) = std::fs::read_to_string(&crash_file) {
                                        for l in raw.lines().take(80) {
                                            analysis_lines.push(l.to_string());
                                        }
                                    }
                                }
                                let summary = analyze_crash_cause(
                                    &analysis_lines,
                                    rep.exit_code,
                                    rep.unsupported_class_version,
                                );
                                self.crash_summaries.insert(instance_id.clone(), summary);

                                if self.prefs.auto_open_logs_on_crash {
                                    self.active_page = AppPage::Home;
                                    self.prefs.home_view_mode = "workspace".to_string();
                                    if self.selected_instance_id.as_deref()
                                        != Some(instance_id.as_str())
                                    {
                                        self.selected_instance_id = Some(instance_id);
                                        self.sync_selected_from_cache();
                                    }
                                    self.active_tab = DetailTab::Logs;
                                    self.instance_inline_browser = None;
                                }
                            }
                        },
                        Err(err) => {
                            let diag = LaunchErrorPresenter::present(&err, false);
                            if is_account_or_token_error(&err) {
                                let msg = self.t("accounts.token_expired_notice").to_string();
                                self.notice_banner = Some((format!("{msg} ({err})"), true));
                                self.open_accounts_window(ctx);
                            } else {
                                self.notice_banner = Some((diag.banner_text(), true));
                            }
                        }
                    }
                }
                AppEvent::DeviceCodeReady(dc) => {
                    self.ms_device_code = Some(dc);
                }
                AppEvent::MicrosoftLoginFinished(res) => {
                    self.ms_login_in_progress = false;
                    self.ms_login_mode = None;
                    self.ms_login_task = None;
                    self.ms_device_code = None;
                    match res {
                        Ok(acc) => {
                            self.account_avatars.remove(&acc.uuid);
                            self.account_avatars_loading.remove(&acc.uuid);
                            self.reload_accounts();
                            self.ensure_account_avatars_loaded(ctx);
                            self.push_global_log(format!(
                                "[AUTH] Signed in as Microsoft account '{}'.",
                                acc.username
                            ));
                            self.notice_banner = Some((
                                format!("Signed in as Microsoft account '{}'.", acc.username),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.push_global_log(format!(
                                "[ERROR] Microsoft sign-in failed: {err}"
                            ));
                            self.notice_banner =
                                Some((format!("Microsoft sign-in failed: {err}"), true));
                            self.open_accounts_window(ctx);
                        }
                    }
                }
                AppEvent::JavaInstallFinished {
                    task_id,
                    major,
                    result,
                } => {
                    self.finish_task(task_id);
                    self.installing_java_majors.remove(&major);
                    match result {
                        Ok(java) => {
                            self.reload_java();
                            self.start_java_diagnostics_scan(ctx);
                            self.push_global_log(format!(
                                "[JAVA] Installed managed Temurin Java {} ({}).",
                                java.major, java.version_string
                            ));
                            self.notice_banner = Some((
                                format!(
                                    "Installed managed Temurin Java {} ({}).",
                                    java.major, java.version_string
                                ),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("Java installation failed: {err}"), true));
                        }
                    }
                }
                AppEvent::RepairFinished {
                    instance_id,
                    report,
                } => {
                    self.launch_states.insert(instance_id, LaunchState::Idle);
                    match report {
                        Ok(rep) => {
                            self.push_global_log(format!(
                                "[REPAIR] Completed for '{}' ({}); client repaired: {}.",
                                rep.instance_id, rep.version_id, rep.client_repaired
                            ));
                            self.notice_banner = Some((
                                format!(
                                    "Repair complete for '{}' ({}). Client repaired: {}.",
                                    rep.instance_id, rep.version_id, rep.client_repaired
                                ),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.notice_banner = Some((format!("Repair failed: {err}"), true));
                        }
                    }
                }
                AppEvent::BackupCreated {
                    instance_id,
                    result,
                } => {
                    self.backup_busy = false;
                    let pending_update = if self
                        .pending_backup_before_modpack_update
                        .as_ref()
                        .is_some_and(|(id, _)| id == &instance_id)
                    {
                        self.pending_backup_before_modpack_update.take()
                    } else {
                        None
                    };
                    match result {
                        Ok(info) => {
                            self.backup_note_input.clear();
                            self.backups_cache.remove(&instance_id);
                            if self.selected_instance_id.as_deref() == Some(instance_id.as_str())
                                && (self.active_tab == DetailTab::Backups
                                    || (self.revealed_tab_counts_instance.as_deref()
                                        == Some(instance_id.as_str())
                                        && self
                                            .revealed_tab_counts
                                            .contains_key(&DetailTab::Backups)))
                            {
                                self.load_backups_async(&instance_id, ctx, true);
                            }
                            self.push_global_log(format!(
                                "[BACKUP] Created '{}' for '{instance_id}' ({}).",
                                info.id,
                                format_bytes(info.size_bytes)
                            ));
                            self.notice_banner = Some((
                                format!(
                                    "Backup '{}' created ({}).",
                                    info.id,
                                    format_bytes(info.size_bytes)
                                ),
                                false,
                            ));
                            if let Some((inst_id, candidate)) = pending_update {
                                self.start_modpack_update_apply(inst_id, candidate, ctx);
                            }
                        }
                        Err(err) => {
                            let suffix = if pending_update.is_some() {
                                " (modpack update cancelled)."
                            } else {
                                ""
                            };
                            self.notice_banner =
                                Some((format!("Backup failed: {err}{suffix}"), true));
                        }
                    }
                }
                AppEvent::BackupRestored {
                    instance_id,
                    result,
                } => {
                    self.backup_busy = false;
                    match result {
                        Ok(outcome) => {
                            self.backups_cache.remove(&instance_id);
                            self.mods_cache.remove(&instance_id);
                            if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                                self.sync_selected_from_cache();
                                self.ensure_active_tab_loaded(ctx);
                            }
                            self.notice_banner = Some((
                                format!(
                                    "Restored {} files to '{}'.",
                                    outcome.restored_files, outcome.instance_id
                                ),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.notice_banner = Some((format!("Restore failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ModpackInstalled {
                    task_id,
                    cf_pack_id,
                    result,
                } => {
                    self.finish_task(task_id);
                    if let Some(pid) = cf_pack_id {
                        self.installing_cf_packs.remove(&pid);
                    } else {
                        self.installing_archive_count =
                            self.installing_archive_count.saturating_sub(1);
                    }
                    match result {
                        Ok(rep) => {
                            self.reload_instances();
                            self.push_global_log(format!(
                                "[MODPACK] Installed modpack '{}' (MC {}, {}, {} files, {} overrides).",
                                rep.instance_id,
                                rep.game_version,
                                rep.loader.name,
                                rep.downloaded_files,
                                rep.extracted_overrides
                            ));
                            self.notice_banner = Some((
                                format!(
                                    "Installed modpack '{}' (MC {}, {} - {} files, {} overrides{}).",
                                    rep.instance_id,
                                    rep.game_version,
                                    display_loader_name(&rep.loader.name),
                                    rep.downloaded_files,
                                    rep.extracted_overrides,
                                    if rep.manual_downloads.is_empty() {
                                        String::new()
                                    } else {
                                        format!(
                                            ", {} manual mod(s) will be prompted when launching",
                                            rep.manual_downloads.len()
                                        )
                                    }
                                ),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("Modpack install failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ExternalModpackInstalled {
                    task_id,
                    pack_key,
                    result,
                } => {
                    self.finish_task(task_id);
                    self.installing_ext_packs.remove(&pack_key);
                    match result {
                        Ok(rep) => {
                            self.reload_instances();
                            self.push_global_log(format!(
                                "[MODPACK] Installed external modpack '{}' (MC {}, {}, {} files).",
                                rep.instance_id,
                                rep.game_version,
                                rep.loader.name,
                                rep.downloaded_files
                            ));
                            self.notice_banner = Some((
                                format!(
                                    "Installed modpack '{}' (MC {}, {} - {} files).",
                                    rep.instance_id,
                                    rep.game_version,
                                    display_loader_name(&rep.loader.name),
                                    rep.downloaded_files
                                ),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("Modpack install failed: {err}"), true));
                        }
                    }
                }
                AppEvent::MrpackExportFinished { task_id, result } => {
                    self.finish_task(task_id);
                    match result {
                        Ok((path, count)) => {
                            self.notice_banner = Some((
                                format!(
                                    "Exported Modrinth modpack ({count} files) to {}",
                                    path.display()
                                ),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("Export .mrpack failed: {err}"), true));
                        }
                    }
                }
                AppEvent::UpdateCheckFinished { manual, result } => {
                    self.update_checking = false;
                    match result {
                        Ok(res) => {
                            let avail = res.update_available && res.latest.is_some();
                            let status = if let Some(ref latest) = res.latest {
                                if res.update_available {
                                    format!(
                                        "Update available: v{} ({})",
                                        latest.version, latest.source
                                    )
                                } else {
                                    format!(
                                        "Up to date (latest v{} on {})",
                                        latest.version,
                                        res.channel.as_str()
                                    )
                                }
                            } else {
                                format!(
                                    "No published releases found on {} channel yet.",
                                    res.channel.as_str()
                                )
                            };
                            self.push_global_log(format!("[UPDATE] {status}"));
                            self.update_status_text = status.clone();
                            self.last_update_check = Some(res);
                            if avail {
                                self.show_update_dialog = true;
                                self.notice_banner = Some((status, false));
                            } else if manual {
                                self.notice_banner = Some((status, false));
                            }
                        }
                        Err(err) => {
                            let msg = format!("Update check failed: {err}");
                            self.push_global_log(format!("[ERROR] {msg}"));
                            self.update_status_text = msg.clone();
                            if manual {
                                self.notice_banner = Some((msg, true));
                            }
                        }
                    }
                }
                AppEvent::UpdateInstallPrepared { task_id, result } => {
                    self.finish_task(task_id);
                    self.update_downloading = false;
                    match result {
                        Ok(staged) => {
                            let current_exe = std::env::current_exe().ok();
                            match current_exe.as_deref() {
                                Some(exe) => {
                                    match UpdateInstaller::launch_self_update(
                                        &staged,
                                        exe,
                                        std::process::id(),
                                    ) {
                                        Ok(()) => {
                                            self.push_global_log(format!(
                                                "[UPDATE] Staged v{}; launching self-updater and restarting.",
                                                staged.version
                                            ));
                                            self.show_update_dialog = false;
                                            self.update_status_text = format!(
                                                "Installing v{} and restarting...",
                                                staged.version
                                            );
                                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                        }
                                        Err(e) => {
                                            self.notice_banner = Some((
                                                format!("Failed to launch updater: {e}"),
                                                true,
                                            ));
                                        }
                                    }
                                }
                                None => {
                                    self.notice_banner = Some((
                                        "Could not resolve current launcher executable path."
                                            .to_string(),
                                        true,
                                    ));
                                }
                            }
                        }
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("Update download failed: {err}"), true));
                        }
                    }
                }
                AppEvent::AccountAvatarLoaded {
                    account_uuid,
                    size,
                    rgba,
                } => {
                    self.account_avatars_loading.remove(&account_uuid);
                    let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                    let tex = ctx.load_texture(
                        format!("mcw_avatar_{account_uuid}"),
                        image,
                        egui::TextureOptions::NEAREST,
                    );
                    self.account_avatars.insert(account_uuid, tex);
                }
                AppEvent::FirstRunRecommendationReady(rec) => {
                    self.first_run_scanning = false;
                    self.first_run_min_ram = rec.recommended_min_memory_mb;
                    self.first_run_max_ram = rec.recommended_max_memory_mb;
                    if !rec.gpu_detection.has_dedicated_gpu() {
                        self.first_run_prefer_dedicated_gpu = false;
                    }
                    self.first_run_rec = Some(rec);
                }
                AppEvent::StorageUsageScanned {
                    summary,
                    unused_versions,
                } => {
                    self.storage_scanning = false;
                    self.storage_summary = Some(summary);
                    self.storage_unused_versions = unused_versions;
                }
                AppEvent::StorageCleanupFinished { kind, report } => {
                    self.storage_cleaning = false;
                    let msg = if report.errors.is_empty() {
                        format!(
                            "{kind}: removed {} item(s), reclaimed {}.",
                            report.removed_count,
                            format_bytes(report.reclaimed_bytes)
                        )
                    } else {
                        format!(
                            "{kind}: reclaimed {} with {} error(s): {}",
                            format_bytes(report.reclaimed_bytes),
                            report.errors.len(),
                            report.errors[0]
                        )
                    };
                    self.notice_banner = Some((msg, !report.errors.is_empty()));
                    self.start_storage_scan(ctx);
                    if self.active_page == AppPage::Versions {
                        self.load_cached_versions_async(ctx);
                    }
                }
                AppEvent::JavaDiagnosticsScanned(diags) => {
                    self.java_scanning = false;
                    self.push_global_log(format!(
                        "[INFO] Java runtime diagnostics scan complete ({} installation(s) checked).",
                        diags.len()
                    ));
                    self.java_diagnostics = diags;
                }
                AppEvent::OptiFineInstallFinished {
                    task_id,
                    instance_id,
                    result,
                } => {
                    self.finish_task(task_id);
                    self.optifine_installing = false;
                    match result {
                        Ok(state) => {
                            self.mods_cache.remove(&instance_id);
                            self.optifine_jar_input.clear();
                            self.notice_banner = Some((
                                format!(
                                    "Installed OptiFine {} ({}).",
                                    state.version_id, state.mode
                                ),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("OptiFine install failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ContentStoreDedupFinished { task_id, result } => {
                    self.finish_task(task_id);
                    self.dedup_running = false;
                    match result {
                        Ok(stats) => {
                            let saved = format_bytes(stats.estimated_saved_bytes);
                            let count = stats.deduplicated_files;
                            self.content_store_stats = Some(stats);
                            self.notice_banner = Some((
                                format!(
                                    "Content store deduplicated {count} file(s), saving ~{saved}."
                                ),
                                false,
                            ));
                            self.start_storage_scan(ctx);
                        }
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("Content deduplication failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ModpackOptionalFilesLoaded { request_id, files } => {
                    if self.modpack_dialog.request_id == request_id {
                        self.modpack_dialog.loading_optional_files = false;
                        if let Ok(list) = files {
                            let mut selected = HashSet::new();
                            for item in &list {
                                if item.default_selected {
                                    selected.insert(item.id.clone());
                                }
                            }
                            self.modpack_dialog.optional_files = list;
                            self.modpack_dialog.selected_optional_ids = selected;
                        }
                    }
                }
                AppEvent::ServersLoaded {
                    instance_id,
                    servers,
                } => {
                    self.loading_servers.remove(&instance_id);
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        self.selected_servers = servers.clone();
                    }
                    self.servers_cache.insert(instance_id, servers);
                }
                AppEvent::CachedVersionsLoaded(list) => {
                    self.cached_versions_loading = false;
                    self.cached_versions = list;
                }
                AppEvent::LatestJavaReleaseLoaded(res) => {
                    self.latest_java_checking = false;
                    match res {
                        Ok(major) => {
                            self.latest_java_major = Some(major);
                            self.latest_java_error = None;
                        }
                        Err(err) => {
                            self.latest_java_error = Some(err);
                        }
                    }
                }
                AppEvent::BackgroundPicturePicked(picked) => {
                    self.bg_picker_open = false;
                    if let Some(path) = picked {
                        self.prefs.background_image_path = path.display().to_string();
                        self.prefs.background_enabled = true;
                        self.save_prefs();
                    }
                }
                AppEvent::BackgroundImageLoaded { key, result } => {
                    if let Some(err) = self.background.apply_loaded_user_image(
                        ctx,
                        key,
                        self.prefs.background_enabled,
                        &self.prefs.background_image_path,
                        result,
                    ) {
                        self.notice_banner = Some((err, true));
                    }
                }
                AppEvent::ContentBrowserSearchFinished {
                    request_id,
                    category,
                    provider,
                    total_hits,
                    modrinth,
                    curseforge,
                    ftb,
                    atl,
                    technic,
                } => {
                    if request_id != self.browser_request_id {
                        continue;
                    }
                    self.browser_searching = false;
                    self.browser_total_hits = total_hits;
                    let _ = category;
                    match provider {
                        BrowserProvider::Modrinth => match modrinth {
                            Ok(hits) => {
                                self.push_global_log(format!(
                                    "[NET] Modrinth search ({category:?}) returned {} items (total {total_hits}).",
                                    hits.len()
                                ));
                                let first = hits.first().map(|h| format!("mr:{}", h.slug));
                                self.browser_mr_results = hits;
                                if self.browser_selected_key.is_none() {
                                    self.browser_selected_key = first;
                                }
                            }
                            Err(err) => {
                                self.notice_banner =
                                    Some((format!("Modrinth search failed: {err}"), true));
                            }
                        },
                        BrowserProvider::CurseForge => match curseforge {
                            Ok(mods) => {
                                self.push_global_log(format!(
                                    "[NET] CurseForge search ({category:?}) returned {} items (total {total_hits}).",
                                    mods.len()
                                ));
                                let first = mods.first().map(|m| format!("cf:{}", m.id));
                                self.browser_cf_results = mods;
                                if self.browser_selected_key.is_none() {
                                    self.browser_selected_key = first;
                                }
                            }
                            Err(err) => {
                                self.notice_banner =
                                    Some((format!("CurseForge search failed: {err}"), true));
                            }
                        },
                        BrowserProvider::Ftb => match ftb {
                            Ok(packs) => {
                                self.push_global_log(format!(
                                    "[NET] FTB search ({category:?}) returned {} items (total {total_hits}).",
                                    packs.len()
                                ));
                                let first = packs.first().map(|p| format!("ftb:{}", p.id));
                                self.browser_ftb_results = packs;
                                if self.browser_selected_key.is_none() {
                                    self.browser_selected_key = first;
                                }
                            }
                            Err(err) => {
                                self.notice_banner =
                                    Some((format!("FTB search failed: {err}"), true));
                            }
                        },
                        BrowserProvider::AtLauncher => match atl {
                            Ok(packs) => {
                                self.push_global_log(format!(
                                    "[NET] ATLauncher search ({category:?}) returned {} items (total {total_hits}).",
                                    packs.len()
                                ));
                                let first = packs
                                    .first()
                                    .map(|p| format!("atl:{}", p.effective_safe_name()));
                                self.browser_atl_results = packs;
                                if self.browser_selected_key.is_none() {
                                    self.browser_selected_key = first;
                                }
                            }
                            Err(err) => {
                                self.notice_banner =
                                    Some((format!("ATLauncher search failed: {err}"), true));
                            }
                        },
                        BrowserProvider::Technic => match technic {
                            Ok(packs) => {
                                self.push_global_log(format!(
                                    "[NET] Technic search ({category:?}) returned {} items (total {total_hits}).",
                                    packs.len()
                                ));
                                let first = packs.first().map(|p| format!("tech:{}", p.slug));
                                self.browser_technic_results = packs;
                                if self.browser_selected_key.is_none() {
                                    self.browser_selected_key = first;
                                }
                            }
                            Err(err) => {
                                self.notice_banner =
                                    Some((format!("Technic search failed: {err}"), true));
                            }
                        },
                    }
                }
                AppEvent::ConnectivityChecked(status) => {
                    let prev = self.connectivity_status;
                    self.connectivity_checking = false;
                    self.connectivity_status = status;
                    if prev != status {
                        self.push_global_log(format!(
                            "[NET] Connectivity changed: {:?} -> {:?}.",
                            prev, status
                        ));
                    }
                }
                AppEvent::ContentLibraryLoaded(items) => {
                    self.content_library_loading = false;
                    self.content_library_items = items;
                }
                AppEvent::ModpackUpdateChecked {
                    instance_id,
                    result,
                } => {
                    if self.modpack_update_instance_id == instance_id {
                        self.modpack_update_loading = false;
                        match result {
                            Ok(check) => {
                                self.push_global_log(format!(
                                    "[MODPACK] Checked updates for '{instance_id}': {} candidate(s).",
                                    check.versions.len()
                                ));
                                self.modpack_update_error = None;
                                if let Some(first) = check.versions.first() {
                                    self.modpack_update_selected_version = first.version_id.clone();
                                } else {
                                    self.modpack_update_selected_version.clear();
                                }
                                self.modpack_update_check = Some(check);
                            }
                            Err(err) => {
                                self.push_global_log(format!(
                                    "[ERROR] Modpack update check failed for '{instance_id}': {err}"
                                ));
                                self.modpack_update_error = Some(err);
                                self.modpack_update_check = None;
                            }
                        }
                    }
                }
                AppEvent::ModpackUpdateApplied {
                    task_id,
                    instance_id,
                    result,
                } => {
                    self.finish_task(task_id);
                    match result {
                        Ok(rep) => {
                            self.mods_cache.remove(&instance_id);
                            self.content_packs_cache.clear();
                            self.reload_instances();
                            self.sync_selected_from_cache();
                            self.ensure_active_tab_loaded(ctx);
                            self.push_global_log(format!(
                                "[MODPACK] Updated '{}' to {} (MC {} - {}).",
                                rep.instance_id,
                                rep.new_version_number,
                                rep.game_version,
                                rep.loader.name
                            ));
                            self.notice_banner = Some((
                                format!(
                                    "Updated '{}' to {} (MC {} - {}). Preserved saves & options.",
                                    rep.instance_id,
                                    rep.new_version_number,
                                    rep.game_version,
                                    display_loader_name(&rep.loader.name)
                                ),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.push_global_log(format!(
                                "[ERROR] Modpack update failed for '{instance_id}': {err}"
                            ));
                            self.notice_banner =
                                Some((format!("Modpack update failed: {err}"), true));
                        }
                    }
                }
                AppEvent::PreflightConfirmationRequired {
                    instance_id,
                    quick_play_world,
                    quick_play_server,
                    issues,
                } => {
                    self.last_launch_log_msg.remove(&instance_id);
                    if let Some(tid) = self.launch_task_ids.remove(&instance_id) {
                        self.finish_task(tid);
                    }
                    self.push_global_log(format!(
                        "[WARN] Forge preflight check found {} issue(s) for '{instance_id}'; awaiting confirmation.",
                        issues.len()
                    ));
                    self.launch_states
                        .insert(instance_id.clone(), LaunchState::Idle);
                    self.preflight_confirm_instance_id = Some(instance_id);
                    self.preflight_confirm_world = quick_play_world;
                    self.preflight_confirm_server = quick_play_server;
                    self.preflight_confirm_issues = issues;
                }
                AppEvent::ServerRpSecurityConfirmationRequired {
                    instance_id,
                    quick_play_world,
                    quick_play_server,
                    risky_servers,
                    mitigation_enabled,
                    assessment_headline,
                    assessment_description,
                } => {
                    self.last_launch_log_msg.remove(&instance_id);
                    if let Some(tid) = self.launch_task_ids.remove(&instance_id) {
                        self.finish_task(tid);
                    }
                    self.push_global_log(format!(
                        "[SECURITY][WARN] Found {} server(s) with Auto-Accept Resource Packs in '{instance_id}'; awaiting user confirmation before launch.",
                        risky_servers.len()
                    ));
                    self.launch_states
                        .insert(instance_id.clone(), LaunchState::Idle);
                    self.security_rp_confirm_instance_id = Some(instance_id);
                    self.security_rp_confirm_world = quick_play_world;
                    self.security_rp_confirm_server = quick_play_server;
                    self.security_rp_confirm_servers = risky_servers;
                    self.security_rp_confirm_mitigation_enabled = mitigation_enabled;
                    self.security_rp_confirm_headline = assessment_headline;
                    self.security_rp_confirm_description = assessment_description;
                }
                AppEvent::SecurityPolicyRefreshed(policy) => {
                    self.security_policy_refreshing = false;
                    let msg = format!(
                        "Security policy: {} — {}",
                        policy.validity.label(),
                        policy.detail
                    );
                    self.push_global_log(format!("[SECURITY] {msg}"));
                    self.notice_banner = Some((msg, false));
                    self.resolved_security_policy = policy;
                    self.security_assessments.clear();
                }
                AppEvent::SecurityLibrarySyncFinished {
                    instance_id,
                    outcome,
                    assessment,
                } => {
                    self.security_sync_busy = false;
                    if let Some(a) = assessment {
                        self.security_assessments.insert(instance_id.clone(), a);
                    } else {
                        self.security_assessments.remove(&instance_id);
                    }
                    let (msg, is_err) = match outcome {
                        LibrarySyncOutcome::AlreadyUpToDate { description, .. } => {
                            (description, false)
                        }
                        LibrarySyncOutcome::UpdatedPatchedFreetype { description, .. } => {
                            (description, false)
                        }
                        LibrarySyncOutcome::NoPatchAvailable { description, .. } => {
                            (description, false)
                        }
                        LibrarySyncOutcome::OfflineSkipped { description, .. } => {
                            (description, false)
                        }
                        LibrarySyncOutcome::RolledBack { reason, .. } => (reason, true),
                    };
                    self.push_global_log(format!("[SECURITY][SYNC] {msg}"));
                    self.notice_banner = Some((msg, is_err));
                }
                AppEvent::PythonFolderPicked(picked) => {
                    self.python_import_folder_picking = false;
                    if let Some(path) = picked {
                        self.python_import_path_input = path.display().to_string();
                        self.start_python_inspection(ctx);
                    }
                }
                AppEvent::PythonInspectionFinished {
                    source_root,
                    result,
                } => {
                    let current = self.python_import_path_input.trim().to_string();
                    if current == source_root || self.python_import_inspecting {
                        self.python_import_inspecting = false;
                        self.python_import_result = None;
                        match result {
                            Ok(insp) => {
                                self.python_import_error = None;
                                self.python_import_path_input =
                                    insp.source_root.display().to_string();
                                self.python_import_selected = insp
                                    .instances
                                    .iter()
                                    .map(|i| i.folder_name.clone())
                                    .collect();
                                self.python_import_global_settings = insp.has_launcher_settings;
                                self.python_import_runtimes = !insp.managed_runtimes.is_empty();
                                self.python_import_themes = !insp.custom_themes.is_empty();
                                self.python_import_inspection = Some(insp);
                            }
                            Err(err) => {
                                self.python_import_error = Some(err);
                                self.python_import_inspection = None;
                            }
                        }
                    }
                }
                AppEvent::PythonMigrationFinished { task_id, result } => {
                    self.finish_task(task_id);
                    self.python_import_running = false;
                    match result {
                        Ok(res) => {
                            let imported_count = res.imported_instances.len();
                            let skipped_count = res.skipped_instances.len();
                            let rt_count = res.imported_runtimes.len();
                            let th_count = res.imported_themes.len();
                            let err_count = res.errors.len();
                            if let Some(ref gs) = res.imported_global_settings {
                                self.apply_imported_global_settings(gs, ctx);
                            }
                            if th_count > 0 {
                                self.theme_catalog = ThemeCatalog::load(&self.paths.theme_root);
                                self.apply_current_theme(ctx);
                            }
                            if rt_count > 0 {
                                self.reload_java();
                                self.start_java_diagnostics_scan(ctx);
                            }
                            self.reload_instances();
                            self.sync_selected_from_cache();
                            self.start_python_inspection(ctx);
                            self.python_import_result = Some(res);
                            let msg = self.lang.tr_fmt(
                                "python_import.completed_banner",
                                &[
                                    ("imported", &imported_count.to_string()),
                                    ("skipped", &skipped_count.to_string()),
                                    ("runtimes", &rt_count.to_string()),
                                    ("themes", &th_count.to_string()),
                                    ("errors", &err_count.to_string()),
                                ],
                            );
                            self.notice_banner = Some((msg, err_count > 0));
                        }
                        Err(err) => {
                            self.python_import_error = Some(err.clone());
                            self.notice_banner =
                                Some((format!("Python import failed: {err}"), true));
                        }
                    }
                }
                AppEvent::JunkScanCompleted { report, manual } => {
                    self.junk_scanning = false;
                    let threshold = 50 * 1024 * 1024;
                    if (manual && report.total_bytes > 0) || report.total_bytes >= threshold {
                        self.junk_clean_temp = true;
                        self.junk_clean_logs = true;
                        self.junk_clean_caches = true;
                        self.junk_scan_report = Some(report);
                        self.show_junk_cleanup_modal = true;
                    } else if manual {
                        self.notice_banner =
                            Some(("No junk or expired cache files found.".to_string(), false));
                    }
                }
                AppEvent::JunkCleanCompleted(res) => {
                    self.junk_cleaning = false;
                    match res {
                        Ok((count, bytes)) => {
                            self.show_junk_cleanup_modal = false;
                            self.junk_scan_report = None;
                            self.notice_banner = Some((
                                format!(
                                    "Cleaned {count} junk item(s), reclaimed {}.",
                                    format_bytes(bytes)
                                ),
                                false,
                            ));
                            self.start_storage_scan(ctx);
                        }
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("Junk cleanup failed: {err}"), true));
                        }
                    }
                }
                AppEvent::WatchedFolderChanged(inst_id) => {
                    if self.selected_instance_id.as_deref() == Some(inst_id.as_str()) {
                        match self.active_tab {
                            DetailTab::Mods => self.load_mods_async(&inst_id, ctx, true),
                            DetailTab::ResourcePacks => self.load_content_packs_async(
                                &inst_id,
                                ContentKind::ResourcePack,
                                ctx,
                                true,
                            ),
                            DetailTab::Shaders => self.load_content_packs_async(
                                &inst_id,
                                ContentKind::ShaderPack,
                                ctx,
                                true,
                            ),
                            DetailTab::Screenshots => {
                                self.load_screenshots_async(&inst_id, ctx, true)
                            }
                            _ => {}
                        }
                    }
                }
                AppEvent::ChangelogRefreshed(res) => {
                    self.changelog_refreshing = false;
                    match res {
                        Ok(md) => {
                            self.changelog_markdown = md;
                            self.changelog_error = None;
                            self.notice_banner =
                                Some(("Changelog loaded from GitHub.".to_string(), false));
                        }
                        Err(err) => {
                            self.changelog_error = Some(err.clone());
                            self.notice_banner =
                                Some((format!("Failed to fetch changelog: {err}"), true));
                        }
                    }
                }
                AppEvent::StorageRelocationFinished(res) => {
                    self.relocating_storage_busy = false;
                    match res {
                        Ok((backed_up, new_root)) => {
                            self.relocated_storage_prompt = None;
                            self.prefs = LauncherPrefs::load(&self.paths);
                            self.apply_typography_and_scale(ctx);
                            self.theme_catalog = ThemeCatalog::load(&self.paths.theme_root);
                            self.apply_current_theme(ctx);
                            self.reload_instances();
                            self.reload_accounts();
                            self.reload_java();
                            let msg = if backed_up {
                                format!(
                                    "Backed up & moved existing instances/worlds to {}.",
                                    new_root.display()
                                )
                            } else {
                                format!(
                                    "Moved existing instances/worlds to {}.",
                                    new_root.display()
                                )
                            };
                            self.push_global_log(format!("[STORAGE] {msg}"));
                            self.notice_banner = Some((msg, false));
                        }
                        Err(err) => {
                            self.push_global_log(format!(
                                "[ERROR] Storage migration failed: {err}"
                            ));
                            if let Some(prompt) = &mut self.relocated_storage_prompt {
                                prompt.error = Some(err);
                            }
                        }
                    }
                }
                AppEvent::ScreenshotThumbLoaded { path, size, rgba } => {
                    self.screenshot_thumbs_loading.remove(&path);
                    let handle = if !rgba.is_empty() && size[0] > 0 && size[1] > 0 {
                        let img = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                        let key = format!("ss_thumb_{}", path.display());
                        Some(ctx.load_texture(key, img, egui::TextureOptions::LINEAR))
                    } else {
                        None
                    };
                    self.screenshot_thumb_lru.retain(|k| k != &path);
                    self.screenshot_thumb_lru.push(path.clone());
                    self.screenshot_thumbs.insert(path, handle);
                    const MAX_SCREENSHOT_THUMBS: usize = 48;
                    while self.screenshot_thumbs.len() > MAX_SCREENSHOT_THUMBS {
                        if let Some(oldest) = self.screenshot_thumb_lru.first().cloned() {
                            self.screenshot_thumb_lru.remove(0);
                            self.screenshot_thumbs.remove(&oldest);
                        } else {
                            break;
                        }
                    }
                }
                AppEvent::ModCartPreviewReady {
                    request_id,
                    instance_id,
                    entries,
                } => {
                    if request_id == self.mod_cart_request_id
                        && self.mod_cart_target_instance_id.as_deref() == Some(instance_id.as_str())
                    {
                        self.mod_cart_resolving = false;
                        self.mod_cart_preview = entries;
                    }
                }
                AppEvent::ModCartInstallFinished {
                    task_id,
                    instance_id,
                    installed_count,
                    dep_count,
                    failed,
                } => {
                    self.finish_task(task_id);
                    self.mods_cache.remove(&instance_id);
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        self.load_mods_async(&instance_id, ctx, true);
                    }
                    if failed.is_empty() {
                        self.mod_cart.clear();
                        self.mod_cart_preview.clear();
                        let msg = if dep_count > 0 {
                            format!(
                                "Installed {installed_count} mod(s) (+ {dep_count} dependency file(s)) into '{instance_id}'."
                            )
                        } else {
                            format!("Installed {installed_count} mod(s) into '{instance_id}'.")
                        };
                        self.push_global_log(format!("[MOD CART] {msg}"));
                        self.notice_banner = Some((msg, false));
                    } else {
                        self.mod_cart.retain(|item| {
                            failed
                                .iter()
                                .any(|f| f.starts_with(&format!("{}:", item.title)))
                        });
                        self.mod_cart_preview.clear();
                        let summary = format!(
                            "Installed {installed_count} mod(s) (+ {dep_count} dependencies); {} failed ({}).",
                            failed.len(),
                            failed.join("; ")
                        );
                        self.push_global_log(format!("[ERROR] [MOD CART] {summary}"));
                        self.notice_banner = Some((summary, true));
                    }
                }
                AppEvent::UniversalFolderPicked(picked) => {
                    self.universal_import_folder_picking = false;
                    if let Some(path) = picked {
                        self.universal_import_path = path.display().to_string();
                        self.start_universal_import_scan(ctx);
                    }
                }
                AppEvent::UniversalImportScanned(res) => {
                    self.universal_import_scanning = false;
                    match res {
                        Ok(items) => {
                            self.universal_import_error = None;
                            self.universal_import_selected =
                                items.iter().map(|i| i.id_key.clone()).collect();
                            self.universal_import_items = items;
                        }
                        Err(err) => {
                            self.universal_import_items.clear();
                            self.universal_import_selected.clear();
                            self.universal_import_error = Some(err);
                        }
                    }
                }
                AppEvent::UniversalImportFinished { task_id, result } => {
                    self.finish_task(task_id);
                    self.universal_import_importing = false;
                    match result {
                        Ok(ids) => {
                            self.reload_instances();
                            if let Some(first) = ids.first() {
                                self.selected_instance_id = Some(first.clone());
                            }
                            self.sync_selected_from_cache();
                            let msg = if ids.is_empty() {
                                "No external instances were imported (missing Minecraft folders)."
                                    .to_string()
                            } else {
                                format!(
                                    "Imported {} instance(s) from {}.",
                                    ids.len(),
                                    self.universal_import_kind.label()
                                )
                            };
                            self.push_global_log(format!("[IMPORT] {msg}"));
                            self.notice_banner = Some((msg, ids.is_empty()));
                            if !ids.is_empty() {
                                self.show_universal_import_modal = false;
                            }
                        }
                        Err(err) => {
                            self.push_global_log(format!(
                                "[ERROR] [IMPORT] External launcher import failed: {err}"
                            ));
                            self.universal_import_error = Some(err.clone());
                            self.notice_banner =
                                Some((format!("Universal import failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ContentDetailLoaded { key, result } => {
                    if self.browser_detail_loading_key.as_deref() == Some(key.as_str()) {
                        self.browser_detail_loading_key = None;
                    }
                    if self.browser_selected_key.as_deref() == Some(key.as_str()) {
                        match result {
                            Ok(payload) => {
                                for url in &payload.gallery_urls {
                                    self.ensure_thumbnail(url, ctx);
                                }
                                self.browser_detail = Some(payload);
                            }
                            Err(err) => {
                                self.browser_detail = Some(BrowserDetailPayload {
                                    key,
                                    body_excerpt: format!(
                                        "Could not load extended project details: {err}"
                                    ),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
                AppEvent::GalleryPreviewLoaded { url, result } => {
                    if self.gallery_preview_url.as_deref() == Some(url.as_str()) {
                        self.gallery_preview_loading = false;
                        match result {
                            Ok((size, rgba)) => {
                                let color_img =
                                    egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                                let tex = ctx.load_texture(
                                    format!("mcw_gallery_preview_{url}"),
                                    color_img,
                                    egui::TextureOptions::LINEAR,
                                );
                                self.gallery_preview_texture = Some((url, tex));
                                self.gallery_preview_error = None;
                            }
                            Err(err) => {
                                self.gallery_preview_texture = None;
                                self.gallery_preview_error = Some(err);
                            }
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn sample_network_speed(&mut self) {
        let elapsed = self.last_net_sample_at.elapsed().as_secs_f64();
        if elapsed >= 0.5 {
            let now_bytes = mcw_net::total_downloaded_bytes();
            let delta = now_bytes.saturating_sub(self.last_net_sample_bytes);
            if delta == 0 {
                self.current_download_bps = 0.0;
            } else {
                let instant_bps = delta as f64 / elapsed;
                // Smooth exponential moving average so speed readout doesn't jitter wildly
                self.current_download_bps = if self.current_download_bps <= 1.0 {
                    instant_bps
                } else {
                    self.current_download_bps * 0.4 + instant_bps * 0.6
                };
            }
            self.last_net_sample_bytes = now_bytes;
            self.last_net_sample_at = Instant::now();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn open_modpack_install_dialog(
        &mut self,
        provider: ModpackInstallProvider,
        provider_label: &str,
        pack_title: String,
        icon_url: Option<String>,
        website_url: Option<String>,
        default_loader: &str,
        ctx: &egui::Context,
    ) {
        let req_id = self.next_modpack_req_id;
        self.next_modpack_req_id += 1;

        let (min_ram, max_ram) = recommended_modpack_ram_mb(default_loader, self.pc_ram_mb);
        let mut candidate_name = mcw_providers::suggest_short_instance_name(&pack_title, 36);
        let store = InstanceStore::new(&self.paths);
        let base_name = candidate_name.clone();
        let mut idx = 2usize;
        while store.load_instance(&candidate_name).is_ok() {
            candidate_name = format!("{base_name} ({idx})");
            idx += 1;
        }

        if let Some(ref url) = icon_url {
            self.ensure_thumbnail(url, ctx);
        }

        self.modpack_dialog = ModpackInstallDialogState {
            open: true,
            request_id: req_id,
            provider: Some(provider.clone()),
            provider_label: provider_label.to_string(),
            pack_title,
            icon_url,
            website_url,
            instance_name: candidate_name,
            default_loader: default_loader.to_string(),
            versions: Vec::new(),
            selected_version_id: String::new(),
            include_beta: false,
            include_alpha: false,
            loading_versions: true,
            error: None,
            min_ram_mb: min_ram,
            max_ram_mb: max_ram,
            ram_user_edited: false,
            java_custom: false,
            java_path: String::new(),
            jvm_preset: JvmPresetId::Default,
            include_optional_files: true,
            optional_files: Vec::new(),
            selected_optional_ids: HashSet::new(),
            loading_optional_files: false,
            enable_lan_agent: false,
        };

        // ATLauncher versions are already present in `pack.versions` - populate instantaneously!
        if let ModpackInstallProvider::AtLauncher { ref pack } = provider {
            let versions: Vec<ModpackVersionOption> = pack
                .versions
                .iter()
                .map(|v| {
                    let channel = if v.is_dev {
                        ModpackChannel::Alpha
                    } else if !v.is_recommended {
                        ModpackChannel::Beta
                    } else {
                        ModpackChannel::Release
                    };
                    ModpackVersionOption {
                        id: v.version.clone(),
                        label: format!("v{} (MC {})", v.version, v.minecraft),
                        mc_version: v.minecraft.clone(),
                        loader: default_loader.to_string(),
                        channel,
                        download_url: None,
                        file_name: String::new(),
                        file_size: 0,
                        recommended_ram_mb: 0,
                    }
                })
                .collect();
            self.modpack_dialog.loading_versions = false;
            self.modpack_dialog.versions = versions;
            self.sync_modpack_dialog_selection();
            self.load_atlauncher_optional_files(ctx);
            return;
        }

        let paths = self.paths.clone();
        let cf_key = self.resolved_cf_api_key();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res: Result<Vec<ModpackVersionOption>, String> = async {
                let cache = ApiCache::open(&paths.cache_root.join("providers.sqlite"))
                    .ok()
                    .map(Arc::new);
                match provider {
                    ModpackInstallProvider::Modrinth { slug } => {
                        let client = ModrinthClient::new(cache).map_err(|e| e.to_string())?;
                        let versions = client
                            .list_versions(&slug, None, None)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(versions
                            .into_iter()
                            .filter_map(|v| {
                                let primary = v.primary_file()?.clone();
                                let channel = match v.version_type.to_ascii_lowercase().as_str() {
                                    "alpha" => ModpackChannel::Alpha,
                                    "beta" => ModpackChannel::Beta,
                                    _ => ModpackChannel::Release,
                                };
                                let mc = v.game_versions.first().cloned().unwrap_or_default();
                                let loader = v.loaders.first().cloned().unwrap_or_default();
                                Some(ModpackVersionOption {
                                    id: v.id,
                                    label: format!("{} ({}) [MC {}]", v.name, v.version_number, mc),
                                    mc_version: mc,
                                    loader,
                                    channel,
                                    download_url: Some(primary.url),
                                    file_name: primary.filename,
                                    file_size: primary.size,
                                    recommended_ram_mb: 0,
                                })
                            })
                            .collect())
                    }
                    ModpackInstallProvider::CurseForge { mod_id, .. } => {
                        let client =
                            CurseForgeClient::new(cf_key, cache).map_err(|e| e.to_string())?;
                        let files = client
                            .get_mod_files(mod_id, None, None)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(files
                            .into_iter()
                            .map(|f| {
                                let channel = match f.release_type {
                                    3 => ModpackChannel::Alpha,
                                    2 => ModpackChannel::Beta,
                                    _ => ModpackChannel::Release,
                                };
                                let mut mc_ver = String::new();
                                let mut loader = String::new();
                                for gv in &f.game_versions {
                                    let low = gv.to_ascii_lowercase();
                                    if matches!(
                                        low.as_str(),
                                        "forge" | "neoforge" | "fabric" | "quilt"
                                    ) {
                                        loader = low;
                                    } else if gv.starts_with("1.") && mc_ver.is_empty() {
                                        mc_ver = gv.clone();
                                    }
                                }
                                let title = if f.display_name.trim().is_empty() {
                                    f.file_name.clone()
                                } else {
                                    f.display_name.clone()
                                };
                                ModpackVersionOption {
                                    id: f.id.to_string(),
                                    label: if mc_ver.is_empty() {
                                        title
                                    } else {
                                        format!("{title} [MC {mc_ver}]")
                                    },
                                    mc_version: mc_ver,
                                    loader,
                                    channel,
                                    download_url: f.download_url,
                                    file_name: f.file_name,
                                    file_size: f.file_length,
                                    recommended_ram_mb: 0,
                                }
                            })
                            .collect())
                    }
                    ModpackInstallProvider::Ftb { pack_id } => {
                        let client = FtbClient::new(cache).map_err(|e| e.to_string())?;
                        let versions = client
                            .get_pack_versions(pack_id)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(versions
                            .into_iter()
                            .map(|v| {
                                let channel = match v.release_type.to_ascii_lowercase().as_str() {
                                    "alpha" => ModpackChannel::Alpha,
                                    "beta" => ModpackChannel::Beta,
                                    _ => ModpackChannel::Release,
                                };
                                ModpackVersionOption {
                                    id: v.id.to_string(),
                                    label: if v.mc_version.is_empty() {
                                        v.name.clone()
                                    } else {
                                        format!("{} [MC {}]", v.name, v.mc_version)
                                    },
                                    mc_version: v.mc_version,
                                    loader: v.loader,
                                    channel,
                                    download_url: None,
                                    file_name: String::new(),
                                    file_size: 0,
                                    recommended_ram_mb: v.recommended_memory_mb,
                                }
                            })
                            .collect())
                    }
                    ModpackInstallProvider::AtLauncher { .. } => Ok(Vec::new()),
                    ModpackInstallProvider::Technic { slug } => {
                        let client = TechnicClient::new(cache).map_err(|e| e.to_string())?;
                        let detail = client.get_modpack(&slug).await.map_err(|e| e.to_string())?;
                        let mc_ver = detail.minecraft.clone();
                        if !detail.solder_url.trim().is_empty() {
                            let solder = client
                                .get_solder_pack(&detail.solder_url, &slug)
                                .await
                                .map_err(|e| e.to_string())?;
                            let mut opts = Vec::new();
                            for b in solder.builds.into_iter().rev() {
                                let low = b.to_ascii_lowercase();
                                let is_rec = solder.recommended.as_deref() == Some(b.as_str());
                                let is_lat = solder.latest.as_deref() == Some(b.as_str());
                                let channel = if low.contains("alpha") || low.contains("dev") {
                                    ModpackChannel::Alpha
                                } else if (is_lat && !is_rec)
                                    || low.contains("beta")
                                    || low.contains("rc")
                                {
                                    ModpackChannel::Beta
                                } else {
                                    ModpackChannel::Release
                                };
                                let tag = if is_rec {
                                    " (Recommended)"
                                } else if is_lat {
                                    " (Latest)"
                                } else {
                                    ""
                                };
                                let label = if mc_ver.is_empty() {
                                    format!("{b}{tag}")
                                } else {
                                    format!("{b}{tag} [MC {mc_ver}]")
                                };
                                opts.push(ModpackVersionOption {
                                    id: b,
                                    label,
                                    mc_version: mc_ver.clone(),
                                    loader: "forge".to_string(),
                                    channel,
                                    download_url: None,
                                    file_name: String::new(),
                                    file_size: 0,
                                    recommended_ram_mb: 0,
                                });
                            }
                            if opts.is_empty() && !detail.version.is_empty() {
                                opts.push(ModpackVersionOption {
                                    id: detail.version.clone(),
                                    label: if mc_ver.is_empty() {
                                        detail.version.clone()
                                    } else {
                                        format!("{} [MC {mc_ver}]", detail.version)
                                    },
                                    mc_version: mc_ver,
                                    loader: "forge".to_string(),
                                    channel: ModpackChannel::Release,
                                    download_url: None,
                                    file_name: String::new(),
                                    file_size: 0,
                                    recommended_ram_mb: 0,
                                });
                            }
                            Ok(opts)
                        } else {
                            let ver_id = if detail.version.trim().is_empty() {
                                "latest".to_string()
                            } else {
                                detail.version.clone()
                            };
                            let label = if mc_ver.is_empty() {
                                ver_id.clone()
                            } else {
                                format!("{ver_id} [MC {mc_ver}]")
                            };
                            Ok(vec![ModpackVersionOption {
                                id: ver_id,
                                label,
                                mc_version: mc_ver,
                                loader: "forge".to_string(),
                                channel: ModpackChannel::Release,
                                download_url: Some(detail.url),
                                file_name: format!("{slug}.zip"),
                                file_size: 0,
                                recommended_ram_mb: 0,
                            }])
                        }
                    }
                }
            }
            .await;
            let _ = tx.send(AppEvent::ModpackInstallVersionsLoaded {
                request_id: req_id,
                versions: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn filtered_modpack_versions(&self) -> Vec<&ModpackVersionOption> {
        let d = &self.modpack_dialog;
        let filtered: Vec<&ModpackVersionOption> = d
            .versions
            .iter()
            .filter(|v| match v.channel {
                ModpackChannel::Release => true,
                ModpackChannel::Beta => d.include_beta,
                ModpackChannel::Alpha => d.include_alpha,
            })
            .collect();
        // If release filter yields 0 items (e.g. a pack that only publishes Beta/Alpha),
        // fall back to showing all versions so the user isn't stuck with an empty dropdown.
        if filtered.is_empty() && !d.versions.is_empty() {
            d.versions.iter().collect()
        } else {
            filtered
        }
    }

    pub(crate) fn sync_modpack_dialog_selection(&mut self) {
        let filtered_ids: Vec<(String, String)> = self
            .filtered_modpack_versions()
            .into_iter()
            .map(|v| (v.id.clone(), v.loader.clone()))
            .collect();
        if let Some((first_id, first_loader)) = filtered_ids.first() {
            let still_valid = filtered_ids
                .iter()
                .any(|(id, _)| id == &self.modpack_dialog.selected_version_id);
            if !still_valid {
                self.modpack_dialog.selected_version_id = first_id.clone();
            }
            let active_loader = filtered_ids
                .iter()
                .find(|(id, _)| id == &self.modpack_dialog.selected_version_id)
                .map(|(_, l)| l.as_str())
                .filter(|l| !l.is_empty())
                .unwrap_or(if !first_loader.is_empty() {
                    first_loader.as_str()
                } else {
                    self.modpack_dialog.default_loader.as_str()
                });
            if !self.modpack_dialog.ram_user_edited {
                let (min_r, max_r) = recommended_modpack_ram_mb(active_loader, self.pc_ram_mb);
                self.modpack_dialog.min_ram_mb = min_r;
                self.modpack_dialog.max_ram_mb = max_r;
            }
        }
    }

    pub(crate) fn confirm_modpack_install_dialog(&mut self, ctx: &egui::Context) {
        let d = self.modpack_dialog.clone();
        let Some(provider) = d.provider else {
            return;
        };
        let chosen_ver = d
            .versions
            .iter()
            .find(|v| v.id == d.selected_version_id)
            .or_else(|| d.versions.first())
            .cloned();
        let Some(ver) = chosen_ver else {
            return;
        };

        let instance_name = if d.instance_name.trim().is_empty() {
            d.pack_title.clone()
        } else {
            d.instance_name.trim().to_string()
        };
        let custom_java = if d.java_custom && !d.java_path.trim().is_empty() {
            Some(PathBuf::from(d.java_path.trim()))
        } else {
            None
        };
        let jvm_preset = d.jvm_preset;
        let min_ram = d.min_ram_mb.max(256).min(d.max_ram_mb.max(256));
        let max_ram = d.max_ram_mb.max(min_ram);
        let enable_lan_agent = d.enable_lan_agent;
        let icon_url = d.icon_url.clone();
        let install_opts = ModpackInstallOptions {
            include_optional: d.include_optional_files,
            selected_optional_paths: if d.optional_files.is_empty() {
                None
            } else {
                Some(d.selected_optional_ids.clone())
            },
        };
        let provider_origin_tag = match &provider {
            ModpackInstallProvider::Modrinth { .. } => "modpack:modrinth",
            ModpackInstallProvider::CurseForge { .. } => "modpack:curseforge",
            ModpackInstallProvider::Ftb { .. } => "modpack:ftb",
            ModpackInstallProvider::AtLauncher { .. } => "modpack:atlauncher",
            ModpackInstallProvider::Technic { .. } => "modpack:technic",
        };

        let (task_id, task_ctrl) = self.begin_task_with_control(
            format!("Install {}", d.pack_title),
            format!("Preparing {}...", ver.label),
        );
        self.modpack_dialog.open = false;

        let paths = self.paths.clone();
        let cf_key = self.resolved_cf_api_key();
        let downloader = self
            .download_client
            .with_task_control(Arc::clone(&task_ctrl));
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        let pack_key = match &provider {
            ModpackInstallProvider::CurseForge { mod_id, .. } => {
                self.installing_cf_packs.insert(*mod_id);
                format!("cf:{mod_id}")
            }
            ModpackInstallProvider::Modrinth { slug } => {
                let k = format!("mr:{slug}");
                self.installing_ext_packs.insert(k.clone());
                k
            }
            ModpackInstallProvider::Ftb { pack_id } => {
                let k = format!("ftb:{pack_id}");
                self.installing_ext_packs.insert(k.clone());
                k
            }
            ModpackInstallProvider::AtLauncher { pack } => {
                let k = format!("atl:{}", pack.id);
                self.installing_ext_packs.insert(k.clone());
                k
            }
            ModpackInstallProvider::Technic { slug } => {
                let k = format!("tech:{slug}");
                self.installing_ext_packs.insert(k.clone());
                k
            }
        };

        self.runtime.spawn(async move {
            let tx_p = tx.clone();
            let ctx_p = ctx_c.clone();
            let cb: ModpackProgressCallback = Arc::new(move |cur, tot, msg| {
                let _ = tx_p.send(AppEvent::TaskProgress {
                    task_id,
                    message: msg,
                    current: cur,
                    total: tot,
                });
                ctx_p.request_repaint();
            });

            let cf_id_opt = match &provider {
                ModpackInstallProvider::CurseForge { mod_id, .. } => Some(*mod_id),
                _ => None,
            };

            let res: Result<mcw_providers::ModpackInstallReport, String> = async {
                let cache = ApiCache::open(&paths.cache_root.join("providers.sqlite"))
                    .ok()
                    .map(Arc::new);

                let report = match provider {
                    ModpackInstallProvider::Modrinth { slug } => {
                        let mr_ver_id = ver.id.clone();
                        let mr_ver_num = ver.label.clone();
                        let mr_slug = slug.clone();
                        let url = ver
                            .download_url
                            .ok_or_else(|| "Selected Modrinth version has no download URL".to_string())?;
                        let fname = if ver.file_name.is_empty() {
                            format!("{mr_slug}.mrpack")
                        } else {
                            ver.file_name
                        };
                        let archive_path = paths.cache_root.join("modpacks").join(&fname);
                        cb(0, 1, "Downloading .mrpack archive...".to_string());
                        let req = mcw_net::DownloadRequest::new(url, &archive_path)
                            .with_size(ver.file_size);
                        let prog_cb = make_task_byte_cb(
                            tx.clone(),
                            ctx_c.clone(),
                            task_id,
                            format!("Downloading {fname}"),
                        );
                        downloader
                            .download_file_with_progress(&req, prog_cb)
                            .await
                            .map_err(|e| e.to_string())?;
                        let rep = ModpackInstaller::install_from_archive_with_options(
                            &paths,
                            &archive_path,
                            Some(&instance_name),
                            &downloader,
                            None,
                            &install_opts,
                            Some(cb.clone()),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                        let _ = ModrinthPackRegistry::set_origin_metadata(
                            &paths.instance_dir(&rep.instance_id),
                            "",
                            &mr_slug,
                            &mr_ver_id,
                            &mr_ver_num,
                        );
                        rep
                    }
                    ModpackInstallProvider::CurseForge { mod_id, slug } => {
                        let cf = CurseForgeClient::new(cf_key, cache).map_err(|e| e.to_string())?;
                        let dl_url = ver.download_url.ok_or_else(|| {
                            format!(
                                "CurseForge pack '{}' has third-party distribution disabled. Download the ZIP at https://www.curseforge.com/minecraft/modpacks/{slug} and use Import Archive.",
                                d.pack_title
                            )
                        })?;
                        let fname = if ver.file_name.is_empty() {
                            format!("cf-{mod_id}-{}.zip", ver.id)
                        } else {
                            ver.file_name
                        };
                        let archive_path = paths.cache_root.join("modpacks").join(&fname);
                        cb(0, 1, "Downloading CurseForge modpack archive...".to_string());
                        let req = mcw_net::DownloadRequest::new(dl_url, &archive_path)
                            .with_size(ver.file_size);
                        let prog_cb = make_task_byte_cb(
                            tx.clone(),
                            ctx_c.clone(),
                            task_id,
                            format!("Downloading {fname}"),
                        );
                        downloader
                            .download_file_with_progress(&req, prog_cb)
                            .await
                            .map_err(|e| e.to_string())?;
                        let rep = ModpackInstaller::install_from_archive_with_options(
                            &paths,
                            &archive_path,
                            Some(&instance_name),
                            &downloader,
                            Some(&cf),
                            &install_opts,
                            Some(cb.clone()),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                        let file_id = ver.id.parse::<u64>().unwrap_or(0);
                        let _ = CurseForgePackRegistry::set_origin_metadata(
                            &paths.instance_dir(&rep.instance_id),
                            mod_id,
                            &slug,
                            file_id,
                            &ver.label,
                        );
                        rep
                    }
                    ModpackInstallProvider::Ftb { pack_id } => {
                        let client = FtbClient::new(cache).map_err(|e| e.to_string())?;
                        let vid = ver.id.parse::<u64>().ok();
                        client
                            .install_pack_version_with_progress(
                                pack_id,
                                vid,
                                &instance_name,
                                &paths,
                                &downloader,
                                Some(cb.clone()),
                            )
                            .await
                            .map_err(|e| e.to_string())?
                    }
                    ModpackInstallProvider::AtLauncher { pack } => {
                        let client = AtLauncherClient::new(cache).map_err(|e| e.to_string())?;
                        client
                            .install_pack_version_with_options(
                                &pack,
                                Some(&ver.id),
                                Some(&instance_name),
                                &paths,
                                &downloader,
                                &install_opts,
                                Some(cb.clone()),
                            )
                            .await
                            .map_err(|e| e.to_string())?
                    }
                    ModpackInstallProvider::Technic { slug } => {
                        let client = TechnicClient::new(cache).map_err(|e| e.to_string())?;
                        client
                            .install_pack_version_with_progress(
                                &slug,
                                Some(&ver.id),
                                Some(&instance_name),
                                &paths,
                                &downloader,
                                Some(cb.clone()),
                            )
                            .await
                            .map_err(|e| e.to_string())?
                    }
                };

                // Apply configured RAM, JVM preset & Java settings to the newly created instance
                let store = InstanceStore::new(&paths);
                if let Ok(mut settings) = store.load_settings(&report.instance_id) {
                    settings.min_memory = min_ram;
                    settings.max_memory = max_ram;
                    if jvm_preset != JvmPresetId::Default {
                        settings.jvm_arguments = get_preset_flags(jvm_preset)
                            .iter()
                            .map(|s| s.to_string())
                            .collect();
                    }
                    if custom_java.is_some() {
                        settings.java_path = custom_java;
                    }
                    apply_lan_agent_option(&mut settings, enable_lan_agent);
                    let _ = store.save_settings(&report.instance_id, &settings);
                }

                // Download & persist modpack artwork as `.mcw/instance-icon.<ext>` (matching Python!)
                if let Some(ref url) = icon_url {
                    if let Ok(resp) = downloader.http().get(url).send().await {
                        if resp.status().is_success() {
                            if let Ok(bytes) = resp.bytes().await {
                                let _ = store.set_instance_icon_bytes_with_origin(
                                    &report.instance_id,
                                    &bytes,
                                    provider_origin_tag,
                                );
                            }
                        }
                    }
                }

                Ok(report)
            }
            .await;

            if let Some(cf_id) = cf_id_opt {
                let _ = tx.send(AppEvent::ModpackInstalled {
                    task_id,
                    cf_pack_id: Some(cf_id),
                    result: res,
                });
            } else {
                let _ = tx.send(AppEvent::ExternalModpackInstalled {
                    task_id,
                    pack_key,
                    result: res,
                });
            }
            ctx_c.request_repaint();
        });
    }

    // -------------------------------------------------------------------------
    // Stage 3 Controllers: Themes, Account 2D Avatars, Auto-Updater,
    // First-Run Setup Wizard & Storage Cleanup
    // -------------------------------------------------------------------------

    pub(crate) fn apply_current_theme(&mut self, ctx: &egui::Context) {
        let def = self.theme_catalog.find(&self.prefs.theme).clone();
        self.theme_mode = def.base_mode;
        let mut pal = def.palette;
        if !self.prefs.custom_accent_hex.trim().is_empty() {
            let _ = apply_custom_accent(&mut pal, &self.prefs.custom_accent_hex);
        }
        self.motion_mode = MotionMode::parse(&self.prefs.motion_mode);
        self.palette = apply_theme_palette(
            ctx,
            self.theme_mode,
            pal,
            self.motion_mode,
            self.prefs.font_size,
        )
        .glass(self.prefs.panel_opacity);
    }

    pub(crate) fn apply_typography_and_scale(&self, ctx: &egui::Context) {
        let custom_path =
            resolve_custom_font_path(&self.prefs.custom_font_family, &self.system_fonts);
        configure_fonts_with_spec(ctx, &self.prefs.custom_font_family, custom_path.as_deref());
        ctx.set_zoom_factor(self.prefs.ui_scale.clamp(0.75, 1.75));
    }

    pub(crate) fn reload_theme_catalog(&mut self, ctx: &egui::Context) {
        self.theme_catalog = ThemeCatalog::load(&self.paths.theme_root);
        self.apply_current_theme(ctx);
        self.notice_banner = Some((
            format!("Loaded {} theme(s).", self.theme_catalog.themes.len()),
            false,
        ));
    }

    pub(crate) fn ensure_account_avatars_loaded(&mut self, ctx: &egui::Context) {
        for acc in self.accounts.clone() {
            if self.account_avatars.contains_key(&acc.uuid)
                || !self.account_avatars_loading.insert(acc.uuid.clone())
            {
                continue;
            }
            let accounts_root = self.paths.accounts_root.clone();
            let tx = self.tx.clone();
            let ctx_c = ctx.clone();

            self.runtime.spawn(async move {
                let mut rendered: Option<([usize; 2], Vec<u8>)> = None;
                if acc.account_type == AccountSource::Microsoft {
                    if let Some(tex_path) =
                        AccountSkinManager::cache_texture(&accounts_root, &acc).await
                    {
                        if let Ok(bytes) = std::fs::read(&tex_path) {
                            rendered = render_skin_face_rgba(&bytes, 32);
                        }
                    }
                }
                if rendered.is_none() {
                    let variant = AccountSkinManager::default_skin_variant(&acc.uuid);
                    rendered = default_skin_face_rgba(variant, 32);
                }
                if let Some((size, rgba)) = rendered {
                    let _ = tx.send(AppEvent::AccountAvatarLoaded {
                        account_uuid: acc.uuid,
                        size,
                        rgba,
                    });
                    ctx_c.request_repaint();
                }
            });
        }
    }

    pub(crate) fn render_account_avatar(&self, ui: &mut egui::Ui, uuid: &str, size: f32) {
        if let Some(tex) = self.account_avatars.get(uuid) {
            ui.add(
                egui::Image::new(tex)
                    .fit_to_exact_size(Vec2::splat(size))
                    .corner_radius(CornerRadius::same(6)),
            );
        } else {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), self.palette.elevated);
        }
    }

    pub(crate) fn start_update_check(&mut self, manual: bool, ctx: &egui::Context) {
        if !mcw_updater::UPDATES_ENABLED {
            let msg = "Updates are currently disabled for MCW Launcher: Rust Edition.".to_string();
            self.update_status_text = msg.clone();
            if manual {
                self.notice_banner = Some((msg, false));
            }
            return;
        }
        if self.update_checking {
            return;
        }
        if self.connectivity_status == ConnectivityStatus::Offline {
            self.update_status_text = "Offline: update check skipped.".to_string();
            if manual {
                self.notice_banner = Some((
                    "Offline mode: connect to the internet to check for launcher updates."
                        .to_string(),
                    true,
                ));
                self.push_global_log("[UPDATE] Update check skipped while offline.".to_string());
            }
            return;
        }
        self.update_checking = true;
        self.update_status_text = "Checking for updates...".to_string();
        let channel = UpdateChannel::parse(&self.prefs.update_channel);
        self.push_global_log(format!(
            "[UPDATE] Checking for launcher updates on '{}' channel (manual: {manual}).",
            channel.as_str()
        ));
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = match UpdateChecker::new() {
                Ok(checker) => checker
                    .check(mcw_models::APP_VERSION, channel)
                    .await
                    .map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(AppEvent::UpdateCheckFinished {
                manual,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_update_download_and_install(
        &mut self,
        release: ReleaseInfo,
        ctx: &egui::Context,
    ) {
        if self.update_downloading {
            return;
        }
        let any_running = self
            .launch_states
            .values()
            .any(|s| matches!(s, LaunchState::Running { .. }));
        if any_running {
            self.notice_banner = Some((
                "Close running Minecraft instance(s) before installing launcher update."
                    .to_string(),
                true,
            ));
            return;
        }

        self.update_downloading = true;
        self.update_status_text = format!("Downloading v{}...", release.version);
        self.push_global_log(format!(
            "[UPDATE] Downloading update v{} (source: {}).",
            release.version, release.source
        ));
        let task_id = self.begin_task(
            format!("Launcher Update v{}", release.version),
            "Downloading release package...",
        );
        let staging_root = self.paths.cache_root.join("updates");
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let tx_p = tx.clone();
            let ctx_p = ctx_c.clone();
            let prog: Arc<dyn Fn(u64, u64) + Send + Sync> = Arc::new(move |cur, tot| {
                let _ = tx_p.send(AppEvent::TaskProgress {
                    task_id,
                    message: format!(
                        "Downloading update ({} / {})",
                        format_bytes(cur),
                        format_bytes(tot)
                    ),
                    current: cur,
                    total: tot,
                });
                ctx_p.request_repaint();
            });
            let res = UpdateInstaller::download_and_stage(&release, &staging_root, Some(prog))
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::UpdateInstallPrepared {
                task_id,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn open_accounts_window(&mut self, _ctx: &egui::Context) {
        self.show_account_modal = true;
    }

    pub(crate) fn open_export_modpack_window(&mut self, _ctx: &egui::Context) {
        if self.selected_instance().is_some() {
            self.show_export_modal = true;
        }
    }

    pub(crate) fn open_global_logs_window(&mut self, ctx: &egui::Context) {
        if self.show_global_logs_window {
            let title_lbl = self.t("navigation.logs").to_string();
            focus_viewport(
                ctx,
                egui::ViewportId::from_hash_of("mcw_vp_global_logs"),
                Some(&format!("{title_lbl} - MCW Launcher")),
            );
        }
        self.show_global_logs_window = true;
    }

    pub(crate) fn open_about_window(&mut self, _ctx: &egui::Context) {
        self.show_about_window = true;
    }

    pub(crate) fn open_first_run_wizard(&mut self, ctx: &egui::Context) {
        self.show_first_run_wizard = true;
        self.first_run_page = 0;
        self.first_run_language = self.lang.current_locale().to_string();
        self.first_run_auto_updates = self.prefs.auto_check_updates;
        self.first_run_discord_rpc = self.prefs.discord_rpc_enabled;
        self.first_run_download_concurrency = self.prefs.download_concurrency;
        self.first_run_java_path = self.prefs.default_java_path.clone();
        self.first_run_prefer_dedicated_gpu = self.prefs.prefer_dedicated_gpu;
        self.first_run_min_ram = self.prefs.default_min_memory;
        self.first_run_max_ram = self.prefs.default_max_memory;
        self.first_run_offline_msg = None;
        self.start_first_run_scan(ctx);
    }

    pub(crate) fn open_java_manager_window(&mut self, ctx: &egui::Context) {
        self.show_java_manager_window = true;
        if self.java_diagnostics.is_empty() && !self.java_scanning {
            self.start_java_diagnostics_scan(ctx);
        }
        if self.latest_java_major.is_none() && !self.latest_java_checking {
            self.start_check_latest_java_release(ctx);
        }
    }

    pub(crate) fn start_check_latest_java_release(&mut self, ctx: &egui::Context) {
        if self.latest_java_checking {
            return;
        }
        self.latest_java_checking = true;
        self.latest_java_error = None;
        let client = self.download_client.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                JavaProvisioner::fetch_latest_feature_release(&client)
                    .await
                    .map_err(|e| e.to_string())
            }
            .await;
            let _ = tx.send(AppEvent::LatestJavaReleaseLoaded(res));
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_first_run_scan(&mut self, ctx: &egui::Context) {
        if self.first_run_scanning {
            return;
        }
        self.first_run_scanning = true;
        let runtimes_root = self.paths.runtimes_root.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let rec = tokio::task::spawn_blocking(move || {
                FirstRunRecommendationService::inspect(&runtimes_root)
            })
            .await
            .unwrap_or_else(|_| FirstRunRecommendationService::inspect(&PathBuf::from("runtimes")));
            let _ = tx.send(AppEvent::FirstRunRecommendationReady(rec));
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_storage_scan(&mut self, ctx: &egui::Context) {
        if self.storage_scanning {
            return;
        }
        self.storage_scanning = true;
        let paths = self.paths.clone();
        let retention = self.prefs.unused_version_retention_days;
        let active_versions: HashSet<String> = self
            .instances
            .iter()
            .map(|i| i.version_id.clone())
            .collect();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let (summary, unused_versions) = tokio::task::spawn_blocking(move || {
                let s = StorageCleanupService::inspect_usage(&paths);
                let u = StorageCleanupService::scan_unused_versions(
                    &paths,
                    retention,
                    &active_versions,
                );
                (s, u)
            })
            .await
            .unwrap_or_default();
            let _ = tx.send(AppEvent::StorageUsageScanned {
                summary,
                unused_versions,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_clear_cache(&mut self, ctx: &egui::Context) {
        if self.storage_cleaning {
            return;
        }
        self.storage_cleaning = true;
        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let report = tokio::task::spawn_blocking(move || {
                StorageCleanupService::clear_api_and_temp_cache(&paths)
            })
            .await
            .unwrap_or_default();
            let _ = tx.send(AppEvent::StorageCleanupFinished {
                kind: "API & Temp Cache".to_string(),
                report,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_prune_unused_versions(&mut self, ctx: &egui::Context) {
        if self.storage_cleaning || self.storage_unused_versions.is_empty() {
            return;
        }
        self.storage_cleaning = true;
        let candidates = self.storage_unused_versions.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let report = tokio::task::spawn_blocking(move || {
                StorageCleanupService::prune_unused_versions(&candidates)
            })
            .await
            .unwrap_or_default();
            let _ = tx.send(AppEvent::StorageCleanupFinished {
                kind: "Unused Minecraft Versions".to_string(),
                report,
            });
            ctx_c.request_repaint();
        });
    }

    // -------------------------------------------------------------------------
    // Stage 4 Controllers: Global Logs, Mod Compatibility, Repair Center,
    // Local Content Import & Drag-and-Drop
    // -------------------------------------------------------------------------

    pub(crate) fn push_global_log(&mut self, entry: impl Into<String>) {
        let raw = entry.into();
        let has_hms_ts = {
            let b = raw.as_bytes();
            b.len() >= 10
                && b[0] == b'['
                && b[3] == b':'
                && b[6] == b':'
                && b[9] == b']'
                && b[1..3].iter().all(u8::is_ascii_digit)
                && b[4..6].iter().all(u8::is_ascii_digit)
                && b[7..9].iter().all(u8::is_ascii_digit)
        };
        let line = if has_hms_ts {
            raw
        } else {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() % 86_400)
                .unwrap_or(0);
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            format!("[{h:02}:{m:02}:{s:02}] {raw}")
        };
        if self.global_logs.len() >= 2000 {
            self.global_logs.pop_front();
        }
        self.global_logs.push_back(line);
    }

    pub(crate) fn refresh_mod_compat_report(&mut self) {
        if let Some(inst) = self.selected_instance() {
            if inst.mod_loader.is_vanilla() || self.selected_mods.is_empty() {
                self.mod_compat_report = None;
            } else {
                self.mod_compat_report = Some(ModCompatibilityAnalyzer::analyze(
                    &inst.mod_loader.name,
                    &self.selected_mods,
                ));
            }
        } else {
            self.mod_compat_report = None;
        }
    }

    pub(crate) fn open_repair_center(&mut self, instance_id: &str, ctx: &egui::Context) {
        if let Some(inst) = self
            .instances
            .iter()
            .find(|i| i.instance_id == instance_id)
            .cloned()
        {
            self.repair_center_report = Some(InstanceHealthManager::scan(&self.paths, &inst));
            if !inst.mod_loader.is_vanilla() && !self.mods_cache.contains_key(instance_id) {
                self.load_mods_async(instance_id, ctx, false);
            } else {
                self.refresh_mod_compat_report();
            }
            self.show_repair_center_window = true;
        }
    }

    pub(crate) fn import_local_mod_files(
        &mut self,
        inst_id: &str,
        files: &[PathBuf],
        ctx: &egui::Context,
    ) -> usize {
        let mods_dir = self.paths.minecraft_dir(inst_id).join("mods");
        if std::fs::create_dir_all(&mods_dir).is_err() {
            return 0;
        }
        let inst_name = self
            .instances
            .iter()
            .find(|i| i.instance_id == inst_id)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| inst_id.to_string());
        let mut copied = 0usize;
        let mut queued = 0usize;
        for src in files {
            if !src.is_file() {
                continue;
            }
            let Some(name) = src.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if !(lower.ends_with(".jar") || lower.ends_with(".jar.disabled")) {
                continue;
            }
            if let Some((existing_file, reason)) =
                self.find_duplicate_mod_in_instance(inst_id, name, None, None)
            {
                self.pending_duplicate_mod_prompts
                    .push(PendingDuplicateModPrompt {
                        instance_id: inst_id.to_string(),
                        instance_name: inst_name.clone(),
                        incoming_file_name: name.to_string(),
                        existing_file_name: existing_file,
                        match_reason: reason,
                        source: DuplicateModSource::LocalFile {
                            source_path: src.clone(),
                        },
                    });
                queued += 1;
                continue;
            }
            if std::fs::copy(src, mods_dir.join(name)).is_ok() {
                copied += 1;
            }
        }
        if copied > 0 {
            self.mods_cache.remove(inst_id);
            self.load_mods_async(inst_id, ctx, true);
            self.push_global_log(format!(
                "Imported {copied} local mod JAR(s) into '{inst_id}'."
            ));
            self.notice_banner = Some((format!("Imported {copied} mod file(s)."), false));
        } else if queued > 0 {
            self.notice_banner = Some((
                format!("{queued} duplicate mod file(s) awaiting confirmation."),
                false,
            ));
        }
        copied
    }

    pub(crate) fn import_local_pack_files(
        &mut self,
        inst_id: &str,
        kind: ContentKind,
        files: &[PathBuf],
        ctx: &egui::Context,
    ) -> usize {
        let dir = self.paths.minecraft_dir(inst_id).join(kind.folder_name());
        if std::fs::create_dir_all(&dir).is_err() {
            return 0;
        }
        let mut copied = 0usize;
        for src in files {
            if !src.is_file() {
                continue;
            }
            let Some(name) = src.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if !(lower.ends_with(".zip") || lower.ends_with(".zip.disabled")) {
                continue;
            }
            if std::fs::copy(src, dir.join(name)).is_ok() {
                copied += 1;
            }
        }
        if copied > 0 {
            self.content_packs_cache
                .remove(&(inst_id.to_string(), kind));
            self.load_content_packs_async(inst_id, kind, ctx, true);
            self.push_global_log(format!(
                "Imported {copied} local {:?} archive(s) into '{inst_id}'.",
                kind
            ));
            self.notice_banner = Some((format!("Imported {copied} pack file(s)."), false));
        }
        copied
    }

    pub(crate) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let paths: Vec<PathBuf> = dropped.into_iter().filter_map(|f| f.path).collect();
        if paths.is_empty() {
            return;
        }

        let mut modpack_archives = Vec::new();
        let mut jar_files = Vec::new();
        let mut zip_files = Vec::new();

        for path in paths {
            if !path.is_file() {
                continue;
            }
            let lower = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if lower.ends_with(".mrpack") || lower.ends_with(".mcwpack") {
                modpack_archives.push(path);
            } else if lower.ends_with(".jar") || lower.ends_with(".jar.disabled") {
                jar_files.push(path);
            } else if lower.ends_with(".zip") || lower.ends_with(".zip.disabled") {
                zip_files.push(path);
            }
        }

        if let Some(archive) = modpack_archives.into_iter().next() {
            self.modpack_path_input = archive.display().to_string();
            self.modpack_name_input.clear();
            self.inspect_local_archive_optional_files();
            self.create_tab = CreateInstanceTab::ImportArchive;
            self.start_modpack_install(ctx);
            return;
        }

        if !jar_files.is_empty() {
            if let Some(inst) = self.selected_instance().cloned() {
                if inst.mod_loader.is_vanilla() {
                    self.notice_banner = Some((
                        "Selected instance uses Vanilla; switch to a mod loader before importing .jar mods."
                            .to_string(),
                        true,
                    ));
                } else {
                    self.import_local_mod_files(&inst.instance_id, &jar_files, ctx);
                }
            } else {
                self.notice_banner = Some((
                    "Select an instance before dropping .jar mod files.".to_string(),
                    true,
                ));
            }
        }

        if !zip_files.is_empty() {
            if let Some(kind) = self.active_tab.content_kind() {
                if let Some(inst) = self.selected_instance().cloned() {
                    self.import_local_pack_files(&inst.instance_id, kind, &zip_files, ctx);
                }
            } else if let Some(archive) = zip_files.into_iter().next() {
                self.modpack_path_input = archive.display().to_string();
                self.modpack_name_input.clear();
                self.inspect_local_archive_optional_files();
                self.create_tab = CreateInstanceTab::ImportArchive;
                self.start_modpack_install(ctx);
            }
        }
    }

    // -------------------------------------------------------------------------
    // Stage 5 Controllers: Java Diagnostics, LAN Agent Log, OptiFine Standalone,
    // Content Store Deduplication, Loader Diagnostics & Optional Files
    // -------------------------------------------------------------------------

    pub(crate) fn inspect_local_archive_optional_files(&mut self) {
        self.archive_optional_files.clear();
        self.archive_selected_optional_ids.clear();
        self.archive_include_optional = true;
        let path = PathBuf::from(self.modpack_path_input.trim());
        if !path.is_file() {
            return;
        }
        if self.modpack_name_input.trim().is_empty() {
            if let Some(raw_name) = mcw_providers::inspect_archive_name(&path) {
                let mut candidate_name = mcw_providers::suggest_short_instance_name(&raw_name, 36);
                let store = InstanceStore::new(&self.paths);
                let base_name = candidate_name.clone();
                let mut idx = 2usize;
                while store.load_instance(&candidate_name).is_ok() {
                    candidate_name = format!("{base_name} ({idx})");
                    idx += 1;
                }
                self.modpack_name_input = candidate_name;
            }
        }
        if let Ok(list) = ModpackInstaller::inspect_optional_files(&path) {
            for item in &list {
                if item.default_selected {
                    self.archive_selected_optional_ids.insert(item.id.clone());
                }
            }
            self.archive_optional_files = list;
        }
    }

    pub(crate) fn load_atlauncher_optional_files(&mut self, ctx: &egui::Context) {
        let Some(ModpackInstallProvider::AtLauncher { ref pack }) = self.modpack_dialog.provider
        else {
            return;
        };
        let pack_clone = pack.clone();
        let req_id = self.modpack_dialog.request_id;
        let ver_id = self.modpack_dialog.selected_version_id.clone();
        if ver_id.is_empty() {
            return;
        }
        self.modpack_dialog.loading_optional_files = true;
        self.modpack_dialog.optional_files.clear();
        self.modpack_dialog.selected_optional_ids.clear();

        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res: Result<Vec<ModpackOptionalFile>, String> = async {
                let cache = ApiCache::open(&paths.cache_root.join("providers.sqlite"))
                    .ok()
                    .map(Arc::new);
                let client = AtLauncherClient::new(cache).map_err(|e| e.to_string())?;
                client
                    .inspect_optional_mods(&pack_clone, Some(&ver_id))
                    .await
                    .map_err(|e| e.to_string())
            }
            .await;
            let _ = tx.send(AppEvent::ModpackOptionalFilesLoaded {
                request_id: req_id,
                files: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_java_diagnostics_scan(&mut self, ctx: &egui::Context) {
        if self.java_scanning {
            return;
        }
        self.java_scanning = true;
        let runtimes = self.paths.runtimes_root.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();
        self.runtime.spawn(async move {
            let diags =
                tokio::task::spawn_blocking(move || JavaDiagnosticsManager::scan(Some(&runtimes)))
                    .await
                    .unwrap_or_default();
            let _ = tx.send(AppEvent::JavaDiagnosticsScanned(diags));
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn uninstall_managed_java(&mut self, executable: PathBuf, ctx: &egui::Context) {
        match JavaDiagnosticsManager::uninstall_managed(&self.paths.runtimes_root, &executable) {
            Ok(true) => {
                self.reload_java();
                self.start_java_diagnostics_scan(ctx);
                self.push_global_log(format!(
                    "Uninstalled managed Java runtime at {}",
                    executable.display()
                ));
                self.notice_banner = Some(("Uninstalled managed Java runtime.".to_string(), false));
            }
            Ok(false) => {
                self.notice_banner = Some((
                    "Selected managed Java directory no longer exists.".to_string(),
                    true,
                ));
            }
            Err(err) => {
                self.notice_banner =
                    Some((format!("Failed to uninstall Java runtime: {err}"), true));
            }
        }
    }

    pub(crate) fn open_lan_agent_log_window(&mut self, instance_id: &str, ctx: &egui::Context) {
        if self.show_lan_agent_log_window {
            let title = self.t("lan.agent.log.title").to_string();
            focus_viewport(
                ctx,
                egui::ViewportId::from_hash_of("mcw_vp_lan_agent_log"),
                Some(&title),
            );
        }
        self.lan_agent_log_instance_id = instance_id.to_string();
        self.refresh_lan_agent_log();
        self.show_lan_agent_log_window = true;
    }

    pub(crate) fn refresh_lan_agent_log(&mut self) {
        if self.lan_agent_log_instance_id.is_empty() {
            self.lan_agent_log_text.clear();
            return;
        }
        let log_path =
            LanAgentManager::log_path(&self.paths.instance_dir(&self.lan_agent_log_instance_id));
        self.lan_agent_log_text = if log_path.is_file() {
            std::fs::read_to_string(&log_path).unwrap_or_else(|e| {
                self.lang
                    .tr_fmt("lan.agent.log.read_error", &[("error", &e.to_string())])
            })
        } else {
            String::new()
        };
    }

    pub(crate) fn start_optifine_standalone_install(
        &mut self,
        instance: Instance,
        jar_path: PathBuf,
        ctx: &egui::Context,
    ) {
        if self.optifine_installing {
            return;
        }
        let Some(java_exe) = self
            .selected_settings
            .as_ref()
            .and_then(|s| s.java_path.clone())
            .or_else(|| self.java_installations.first().map(|j| j.path.clone()))
        else {
            self.notice_banner = Some((
                "No Java runtime found. Install Java 17 or 21 first.".to_string(),
                true,
            ));
            return;
        };

        self.optifine_installing = true;
        let task_id = self.begin_task(
            format!("OptiFine: {}", instance.name),
            "Preparing isolated OptiFine standalone installer...",
        );
        let paths = self.paths.clone();
        let client = self.download_client.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res: Result<OptiFineState, String> = async {
                let manifest_svc = VersionManifestService::new(&paths, &client);
                let entry = manifest_svc
                    .find_version(&instance.version_id, false)
                    .await
                    .map_err(|e| e.to_string())?;
                let base_meta = manifest_svc
                    .load_version_metadata(&entry)
                    .await
                    .map_err(|e| e.to_string())?;
                let client_jar = paths.client_jar(&base_meta.id);
                if let Some(dl) = base_meta.downloads.as_ref().and_then(|d| d.client.as_ref()) {
                    let req = mcw_net::DownloadRequest::new(&dl.url, &client_jar)
                        .with_sha1(dl.sha1.clone())
                        .with_size(dl.size);
                    client
                        .download_file(&req)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                let paths_c = paths.clone();
                let inst_c = instance.clone();
                tokio::task::spawn_blocking(move || {
                    OptiFineManager::install_standalone_profile(
                        &paths_c, &inst_c, &base_meta, &jar_path, &java_exe,
                    )
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())
            }
            .await;

            let _ = tx.send(AppEvent::OptiFineInstallFinished {
                task_id,
                instance_id: instance.instance_id,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_content_store_dedup(&mut self, ctx: &egui::Context) {
        if self.dedup_running {
            return;
        }
        self.dedup_running = true;
        let task_id = self.begin_task(
            "Deduplicate Shared Content",
            "Scanning instances for duplicate mods and resource/shader packs...",
        );
        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = tokio::task::spawn_blocking(move || {
                ContentStore::deduplicate_instances(&paths).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|inner| inner);

            let _ = tx.send(AppEvent::ContentStoreDedupFinished {
                task_id,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn export_loader_diagnostics(&mut self, inst: &Instance) {
        let default_name = format!(
            "{}-{}-diagnostics.zip",
            inst.instance_id, inst.mod_loader.name
        );
        let Some(dest) = save_file_dialog(
            self.t("forge.export_diagnostics"),
            "ZIP archive (*.zip)|*.zip|All Files (*.*)|*.*",
            &default_name,
        ) else {
            return;
        };

        let cached_profile_path = match inst.mod_loader.name.to_ascii_lowercase().as_str() {
            "forge" | "neoforge" | "quilt" => {
                let id = format!(
                    "{}-{}-{}",
                    inst.mod_loader.name.to_ascii_lowercase(),
                    inst.version_id,
                    inst.mod_loader.version
                );
                let p = self.paths.version_json(&id);
                if p.is_file() {
                    Some(p)
                } else {
                    None
                }
            }
            _ => None,
        };
        let metadata: Option<mcw_models::minecraft::VersionMetadata> = cached_profile_path
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok());

        match LoaderDiagnosticsExporter::export(
            &self.paths,
            inst,
            metadata.as_ref(),
            &dest,
            mcw_models::APP_VERSION,
        ) {
            Ok(out) => {
                self.notice_banner = Some((
                    format!("Exported mod-loader diagnostics to {}", out.display()),
                    false,
                ));
            }
            Err(err) => {
                self.notice_banner = Some((
                    format!("Failed to export mod-loader diagnostics: {err}"),
                    true,
                ));
            }
        }
    }

    // -------------------------------------------------------------------------
    // v0.14 Parity Controllers: Connectivity, Content Library, Modpack Update,
    // Issue Report, Preflight Confirm & Unsaved Instance Settings Guard
    // -------------------------------------------------------------------------

    pub(crate) fn start_connectivity_check(&mut self, ctx: &egui::Context) {
        if self.connectivity_checking {
            return;
        }
        self.connectivity_checking = true;
        self.last_connectivity_check_at = Instant::now();
        let client = self.download_client.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let status = ConnectivityMonitor::probe(&client).await;
            let _ = tx.send(AppEvent::ConnectivityChecked(status));
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn open_content_library_window(&mut self, ctx: &egui::Context) {
        self.show_content_library_window = true;
        self.start_content_library_scan(ctx);
    }

    pub(crate) fn start_content_library_scan(&mut self, ctx: &egui::Context) {
        if self.content_library_loading {
            return;
        }
        self.content_library_loading = true;
        let paths = self.paths.clone();
        let instances = self.instances.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let collected = tokio::task::spawn_blocking(move || {
                let mut rows = Vec::new();
                for inst in &instances {
                    let inst_dir = paths.instance_dir(&inst.instance_id);
                    for item in InstalledContentLibrary::scan(&inst_dir, &inst.name) {
                        rows.push((inst.instance_id.clone(), inst.name.clone(), item));
                    }
                }
                rows
            })
            .await
            .unwrap_or_default();
            let _ = tx.send(AppEvent::ContentLibraryLoaded(collected));
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn apply_proxy_settings(&mut self, ctx: &egui::Context) {
        mcw_net::set_global_proxy_config(mcw_net::ProxyConfig {
            mode: self.prefs.proxy_mode.clone(),
            host: self.prefs.proxy_host.clone(),
            port: self.prefs.proxy_port,
            username: self.prefs.proxy_username.clone(),
            password: self.prefs.proxy_password.clone(),
        });
        match DownloadClient::new() {
            Ok(client) => {
                self.download_client = client;
                self.save_prefs();
                self.start_connectivity_check(ctx);
                let summary =
                    if self.prefs.proxy_mode == "none" || self.prefs.proxy_host.trim().is_empty() {
                        "Direct connection enabled (no proxy).".to_string()
                    } else {
                        format!(
                            "Applied {} proxy ({}:{}).",
                            self.prefs.proxy_mode.to_ascii_uppercase(),
                            self.prefs.proxy_host.trim(),
                            self.prefs.proxy_port
                        )
                    };
                self.push_global_log(format!("[NETWORK] {summary}"));
                self.notice_banner = Some((summary, false));
            }
            Err(err) => {
                self.notice_banner = Some((format!("Invalid proxy configuration: {err}"), true));
            }
        }
    }

    pub(crate) fn open_modpack_update_window(&mut self, instance_id: String, ctx: &egui::Context) {
        if self.show_modpack_update_window && self.modpack_update_instance_id == instance_id {
            return;
        }
        self.show_modpack_update_window = true;
        self.modpack_update_instance_id = instance_id.clone();
        self.modpack_update_check = None;
        self.modpack_update_selected_version.clear();
        self.modpack_update_error = None;
        self.modpack_update_confirm_candidate = None;
        self.start_modpack_update_check(instance_id, ctx);
    }

    pub(crate) fn start_modpack_update_check(&mut self, instance_id: String, ctx: &egui::Context) {
        self.modpack_update_loading = true;
        self.modpack_update_error = None;
        self.modpack_update_confirm_candidate = None;
        let paths = self.paths.clone();
        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let cf_key = self.resolved_cf_api_key();
        let include_beta = self.modpack_update_include_beta;
        let include_alpha = self.modpack_update_include_alpha;
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let mr = ModrinthClient::new(cache.clone()).map_err(|e| e.to_string())?;
                let cf = CurseForgeClient::new(cf_key, cache.clone()).ok();
                let ftb = FtbClient::new(cache.clone()).ok();
                let atl = AtLauncherClient::new(cache).ok();
                let inst_dir = paths.instance_dir(&instance_id);
                ModpackUpdateService::check_updates(
                    &instance_id,
                    &inst_dir,
                    &mr,
                    cf.as_ref(),
                    ftb.as_ref(),
                    atl.as_ref(),
                    include_beta,
                    include_alpha,
                )
                .await
                .map_err(|e| e.to_string())
            }
            .await;

            let _ = tx.send(AppEvent::ModpackUpdateChecked {
                instance_id,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_modpack_update_apply(
        &mut self,
        instance_id: String,
        candidate: mcw_providers::ModpackUpdateCandidate,
        ctx: &egui::Context,
    ) {
        let task_id = self.begin_task(
            format!("Update Modpack: {instance_id}"),
            format!("Updating to {}...", candidate.version_number),
        );
        let paths = self.paths.clone();
        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let cf_key = self.resolved_cf_api_key();
        let downloader = self.download_client.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let tx_p = tx.clone();
            let ctx_p = ctx_c.clone();
            let cb: ModpackProgressCallback = Arc::new(move |cur, tot, msg| {
                let _ = tx_p.send(AppEvent::TaskProgress {
                    task_id,
                    current: cur,
                    total: tot,
                    message: msg,
                });
                ctx_p.request_repaint();
            });
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let cf = CurseForgeClient::new(cf_key, cache.clone()).ok();
                let ftb = FtbClient::new(cache.clone()).ok();
                let atl = AtLauncherClient::new(cache).ok();
                ModpackUpdateService::apply_update(
                    &paths,
                    &instance_id,
                    &candidate,
                    &downloader,
                    cf.as_ref(),
                    ftb.as_ref(),
                    atl.as_ref(),
                    Some(cb),
                )
                .await
                .map_err(|e| e.to_string())
            }
            .await;

            let _ = tx.send(AppEvent::ModpackUpdateApplied {
                task_id,
                instance_id,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn current_issue_report_input(&self) -> IssueReportInput {
        let inst = self.selected_instance();
        let log_excerpt = if let Some(sel) = inst {
            self.instance_logs
                .get(&sel.instance_id)
                .map(|lines| {
                    lines
                        .iter()
                        .rev()
                        .take(50)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .map(|l| l.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    self.global_logs
                        .iter()
                        .rev()
                        .take(40)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n")
                })
        } else {
            self.global_logs
                .iter()
                .rev()
                .take(40)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        };

        IssueReportInput {
            launcher_version: format!("v{}", mcw_models::APP_VERSION),
            os_info: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            summary: self.issue_report_summary.clone(),
            instance_name: inst.map(|i| i.name.clone()),
            minecraft_version: inst.map(|i| i.version_id.clone()),
            mod_loader: inst.map(|i| i.mod_loader.name.clone()),
            log_excerpt,
        }
    }

    pub(crate) fn rebuild_issue_report_markdown(&mut self) {
        let input = self.current_issue_report_input();
        self.issue_report_markdown = IssueReportBuilder::build_markdown(&input);
    }

    pub(crate) fn open_issue_report_window(&mut self, _ctx: &egui::Context) {
        self.show_issue_report_window = true;
        self.rebuild_issue_report_markdown();
    }

    pub(crate) fn request_tab_switch(&mut self, tab: DetailTab, ctx: &egui::Context) {
        if self.active_tab == DetailTab::Settings
            && tab != DetailTab::Settings
            && self.instance_settings_dirty
        {
            self.pending_tab_switch = Some(tab);
            self.pending_instance_switch = None;
            self.show_unsaved_changes_window = true;
            return;
        }
        self.active_tab = tab;
        self.instance_inline_browser = None;
        self.browser_query.clear();
        self.browser_inline_detail_open = false;
        self.browser_detail = None;
        self.browser_detail_loading_key = None;
        self.mod_search_query.clear();
        self.mod_status_filter = "all".to_string();
        self.logs_tab_search.clear();
        self.logs_level_filter = "all".to_string();
        self.ensure_active_tab_loaded(ctx);
        self.clear_remote_thumbs_if_idle();
        self.record_nav_location();
    }

    pub(crate) fn request_instance_switch(&mut self, instance_id: String, ctx: &egui::Context) {
        if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
            self.record_nav_location();
            return;
        }
        if self.active_tab == DetailTab::Settings && self.instance_settings_dirty {
            self.pending_instance_switch = Some(instance_id);
            self.pending_tab_switch = None;
            self.show_unsaved_changes_window = true;
            return;
        }
        self.selected_instance_id = Some(instance_id);
        self.instance_inline_browser = None;
        self.browser_query.clear();
        self.browser_inline_detail_open = false;
        self.browser_detail = None;
        self.browser_detail_loading_key = None;
        self.mod_search_query.clear();
        self.logs_tab_search.clear();
        self.sync_selected_from_cache();
        self.ensure_active_tab_loaded(ctx);
        self.clear_remote_thumbs_if_idle();
        self.record_nav_location();
    }

    pub(crate) fn save_selected_instance_settings(&mut self) -> bool {
        let Some(inst) = self.selected_instance().cloned() else {
            return false;
        };
        let jvm_args: Vec<String> = self
            .edit_jvm_args_text
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let game_args: Vec<String> = self
            .edit_game_args_text
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let some_env_vars: Vec<(String, String)> = self
            .edit_env_vars_text
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    return None;
                }
                let (k, v) = trimmed.split_once('=')?;
                let key = k.trim();
                (!key.is_empty()).then(|| (key.to_string(), v.to_string()))
            })
            .collect();
        let Some(ref mut s) = self.selected_settings else {
            return false;
        };
        s.jvm_arguments = jvm_args;
        s.game_arguments = game_args;
        s.env_vars = some_env_vars;
        let notes_to_save = self.edit_instance_notes_text.clone();
        let store = InstanceStore::new(&self.paths);
        match store.save_settings(&inst.instance_id, s) {
            Ok(()) => {
                self.settings_cache
                    .insert(inst.instance_id.clone(), s.clone());
                let notes_path = self.paths.instance_dir(&inst.instance_id).join("notes.txt");
                if notes_to_save.trim().is_empty() {
                    if notes_path.exists() {
                        let _ = std::fs::remove_file(&notes_path);
                    }
                } else if let Err(err) = std::fs::write(&notes_path, &notes_to_save) {
                    self.instance_settings_dirty = false;
                    self.notice_banner =
                        Some((format!("Settings saved, but notes failed: {err}"), true));
                    return true;
                }
                self.instance_notes_baseline = notes_to_save;
                self.instance_settings_dirty = false;
                self.notice_banner = Some(("Settings saved.".to_string(), false));
                true
            }
            Err(e) => {
                self.notice_banner = Some((format!("Failed to save settings: {e}"), true));
                false
            }
        }
    }

    pub(crate) fn discard_selected_instance_settings(&mut self) {
        self.instance_settings_dirty = false;
        self.sync_selected_from_cache();
    }

    pub(crate) fn apply_pending_unsaved_navigation(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        if let Some(tab) = self.pending_tab_switch.take() {
            self.active_tab = tab;
            self.instance_inline_browser = None;
            self.ensure_active_tab_loaded(ctx);
            changed = true;
        }
        if let Some(inst_id) = self.pending_instance_switch.take() {
            self.selected_instance_id = Some(inst_id);
            self.instance_inline_browser = None;
            self.sync_selected_from_cache();
            self.ensure_active_tab_loaded(ctx);
            changed = true;
        }
        if changed {
            self.clear_remote_thumbs_if_idle();
            self.record_nav_location();
        }
    }

    pub(crate) fn open_python_import_window(&mut self, _ctx: &egui::Context) {
        self.show_python_import_window = true;
    }

    pub(crate) fn start_python_inspection(&mut self, ctx: &egui::Context) {
        let raw_root = self.python_import_path_input.trim().to_string();
        if raw_root.is_empty() {
            self.python_import_error = Some(self.t("python_import.error_empty_path").to_string());
            return;
        }
        self.python_import_inspecting = true;
        self.python_import_error = None;
        let dest_paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn_blocking(move || {
            let res = inspect_python_launcher(std::path::Path::new(&raw_root), &dest_paths)
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::PythonInspectionFinished {
                source_root: raw_root,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_python_migration(&mut self, ctx: &egui::Context) {
        if self.python_import_running {
            return;
        }
        let raw_root = self.python_import_path_input.trim().to_string();
        if raw_root.is_empty() {
            self.python_import_error = Some(self.t("python_import.error_empty_path").to_string());
            return;
        }
        self.python_import_running = true;
        self.python_import_error = None;
        self.python_import_result = None;
        let task_id = self.begin_task(
            "Import from MCW Launcher Python".to_string(),
            "Copying selected instances & assets (source stays read-only)...",
        );
        let opts = PythonMigrationOptions {
            selected_folders: self.python_import_selected.iter().cloned().collect(),
            conflict_strategy: self.python_import_conflict,
            import_global_settings: self.python_import_global_settings,
            import_runtimes: self.python_import_runtimes,
            import_themes: self.python_import_themes,
        };
        let dest_paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn_blocking(move || {
            let tx_p = tx.clone();
            let ctx_p = ctx_c.clone();
            let cb = move |cur: usize, tot: usize, msg: String| {
                let _ = tx_p.send(AppEvent::TaskProgress {
                    task_id,
                    message: msg,
                    current: cur as u64,
                    total: tot.max(1) as u64,
                });
                ctx_p.request_repaint();
            };
            let res = migrate_from_python_launcher(
                std::path::Path::new(&raw_root),
                &dest_paths,
                &opts,
                Some(&cb),
            )
            .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::PythonMigrationFinished {
                task_id,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    fn apply_imported_global_settings(
        &mut self,
        gs: &mcw_storage::ImportedGlobalSettings,
        ctx: &egui::Context,
    ) {
        if let Some(ref lang) = gs.language {
            self.prefs.language = lang.clone();
            self.lang.set_language(&self.prefs.normalized_locale());
        }
        if let Some(v) = gs.show_snapshots {
            self.prefs.show_snapshots = v;
            self.create_show_snapshots = v;
        }
        if let Some(v) = gs.remember_window_size {
            self.prefs.remember_window_size = v;
        }
        if let Some(v) = gs.show_content_descriptions {
            self.prefs.show_content_descriptions = v;
        }
        if let Some(v) = gs.debug_mode {
            self.prefs.debug_mode = v;
        }
        if let Some(v) = gs.prefer_dedicated_gpu {
            self.prefs.prefer_dedicated_gpu = v;
        }
        if let Some(v) = gs.discord_rpc_enabled {
            self.prefs.discord_rpc_enabled = v;
        }
        if let Some(ref cid) = gs.discord_client_id {
            self.prefs.discord_client_id = cid.clone();
        }
        if let Some(v) = gs.minimize_on_launch {
            self.prefs.minimize_on_launch = v;
        }
        if let Some(ref motion) = gs.motion_mode {
            self.prefs.motion_mode = motion.clone();
        }
        if let Some(ref hex) = gs.custom_accent_hex {
            self.prefs.custom_accent_hex = hex.clone();
            self.custom_accent_input = hex.clone();
        }
        if let Some(ref th) = gs.theme {
            if self
                .theme_catalog
                .themes
                .iter()
                .any(|t| t.id.eq_ignore_ascii_case(th))
            {
                self.prefs.theme = th.clone();
            }
        }
        if let Some(v) = gs.modrinth_include_beta {
            self.prefs.modrinth_include_beta = v;
        }
        if let Some(v) = gs.modrinth_include_alpha {
            self.prefs.modrinth_include_alpha = v;
        }
        if let Some(ref p) = gs.modrinth_failure_policy {
            self.prefs.modrinth_failure_policy = p.clone();
        }
        if let Some(ref p) = gs.curseforge_failure_policy {
            self.prefs.curseforge_failure_policy = p.clone();
        }
        if let Some(v) = gs.download_limit_mbps {
            self.prefs.download_limit_mbps = mcw_net::set_global_bandwidth_limit_mbps(v);
        }
        if let Some(v) = gs.download_concurrency {
            self.prefs.download_concurrency = v;
        }
        if let Some(v) = gs.unused_version_retention_days {
            self.prefs.unused_version_retention_days = v;
        }
        if let Some(v) = gs.default_min_memory {
            self.prefs.default_min_memory = v;
        }
        if let Some(v) = gs.default_max_memory {
            self.prefs.default_max_memory = v;
        }
        if let Some(v) = gs.default_width {
            self.prefs.default_width = v;
        }
        if let Some(v) = gs.default_height {
            self.prefs.default_height = v;
        }
        if let Some(v) = gs.default_fullscreen {
            self.prefs.default_fullscreen = v;
        }
        if let Some(v) = gs.auto_check_updates {
            self.prefs.auto_check_updates = v;
        }
        if let Some(ref ch) = gs.update_channel {
            self.prefs.update_channel = ch.clone();
        }
        self.apply_current_theme(ctx);
        self.discord_rpc.configure(
            self.prefs.discord_rpc_enabled,
            self.prefs.discord_client_id.clone(),
        );
        self.save_prefs();
    }

    pub(crate) fn start_junk_scan(&mut self, manual: bool, ctx: &egui::Context) {
        if self.junk_scanning || self.junk_cleaning {
            return;
        }
        self.junk_scanning = true;
        let paths = self.paths.clone();
        let retention_days = self.prefs.junk_retention_days.max(1);
        let active_versions: HashSet<String> = self
            .instances
            .iter()
            .map(|i| i.version_id.clone())
            .collect();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let report = tokio::task::spawn_blocking(move || {
                StorageCleanupService::scan_junk(&paths, retention_days, &active_versions)
            })
            .await
            .unwrap_or_default();
            let _ = tx.send(AppEvent::JunkScanCompleted { report, manual });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_junk_clean(
        &mut self,
        include_temp: bool,
        include_logs: bool,
        include_caches: bool,
        ctx: &egui::Context,
    ) {
        let Some(report) = self.junk_scan_report.clone() else {
            return;
        };
        if self.junk_cleaning {
            return;
        }
        self.junk_cleaning = true;
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = tokio::task::spawn_blocking(move || {
                StorageCleanupService::clean_junk(
                    &report,
                    include_temp,
                    include_logs,
                    include_caches,
                )
            })
            .await
            .map(|r| (r.removed_count, r.reclaimed_bytes))
            .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::JunkCleanCompleted(res));
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn sync_active_folder_watcher(&mut self, ctx: &egui::Context) {
        let target: Option<(String, std::path::PathBuf)> =
            if self.active_page == AppPage::Home && self.instance_inline_browser.is_none() {
                self.selected_instance_id.as_ref().and_then(|id| {
                    let mc_dir = self.paths.minecraft_dir(id);
                    let sub = match self.active_tab {
                        DetailTab::Mods => "mods",
                        DetailTab::ResourcePacks => "resourcepacks",
                        DetailTab::Shaders => "shaderpacks",
                        DetailTab::Screenshots => "screenshots",
                        _ => return None,
                    };
                    let dir = mc_dir.join(sub);
                    dir.is_dir().then(|| (id.clone(), dir))
                })
            } else {
                None
            };

        let target_path = target.as_ref().map(|(_, d)| d.clone());
        if self.watched_folder_path == target_path {
            return;
        }
        self.folder_watcher = None;
        self.watched_folder_path = None;

        if let Some((inst_id, dir)) = target {
            let tx = self.tx.clone();
            let ctx_c = ctx.clone();
            let last_sent = Arc::new(std::sync::Mutex::new(
                Instant::now() - Duration::from_secs(2),
            ));
            let handler = move |res: std::result::Result<Event, notify::Error>| {
                let Ok(ev) = res else {
                    return;
                };
                let dominated_by_tmp = !ev.paths.is_empty()
                    && ev.paths.iter().all(|p| {
                        let s = p.to_string_lossy();
                        s.ends_with(".part") || s.contains(".part.seg.") || s.ends_with(".tmp")
                    });
                if dominated_by_tmp {
                    return;
                }
                if let Ok(mut guard) = last_sent.lock() {
                    if guard.elapsed() >= Duration::from_millis(400) {
                        *guard = Instant::now();
                        let _ = tx.send(AppEvent::WatchedFolderChanged(inst_id.clone()));
                        ctx_c.request_repaint();
                    }
                }
            };
            if let Ok(mut watcher) = notify::recommended_watcher(handler) {
                if watcher.watch(&dir, RecursiveMode::NonRecursive).is_ok() {
                    self.folder_watcher = Some(watcher);
                    self.watched_folder_path = Some(dir);
                }
            }
        }
    }

    pub(crate) fn start_changelog_refresh(&mut self, ctx: &egui::Context) {
        if self.changelog_refreshing {
            return;
        }
        self.changelog_error = None;
        if self.connectivity_status == ConnectivityStatus::Offline {
            let msg = "Offline: connect to the internet to load CHANGELOG.md from GitHub.";
            self.changelog_error = Some(msg.to_string());
            self.notice_banner = Some((msg.to_string(), true));
            return;
        }
        self.changelog_refreshing = true;
        let client = self.download_client.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();
        let url = format!(
            "https://raw.githubusercontent.com/{}/main/CHANGELOG.md",
            mcw_models::GITHUB_REPOSITORY
        );
        self.runtime.spawn(async move {
            let res = client.get_text(&url).await.map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::ChangelogRefreshed(res));
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_desktop_storage_relocation(
        &mut self,
        backup_first: bool,
        ctx: &egui::Context,
    ) {
        if self.relocating_storage_busy {
            return;
        }
        let Some(prompt) = self.relocated_storage_prompt.as_mut() else {
            return;
        };
        prompt.error = None;
        let old_root = prompt.old_root.clone();
        let new_root = prompt.new_root.clone();
        self.relocating_storage_busy = true;
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = tokio::task::spawn_blocking(move || {
                LauncherPaths::relocate_user_data(&old_root, &new_root, backup_first)
                    .map(|()| (backup_first, new_root))
                    .map_err(|e| e.to_string())
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            let _ = tx.send(AppEvent::StorageRelocationFinished(res));
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn ensure_screenshot_thumbnail(
        &mut self,
        path: &std::path::Path,
        ctx: &egui::Context,
    ) {
        if self.screenshot_thumbs.contains_key(path) {
            self.screenshot_thumb_lru.retain(|p| p != path);
            self.screenshot_thumb_lru.push(path.to_path_buf());
            return;
        }
        if self.screenshot_thumbs_loading.contains(path) {
            return;
        }
        self.screenshot_thumbs_loading.insert(path.to_path_buf());
        let path_owned = path.to_path_buf();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();
        self.runtime.spawn_blocking(move || {
            let decoded = std::fs::read(&path_owned)
                .ok()
                .and_then(|bytes| image::load_from_memory(&bytes).ok())
                .map(|img| {
                    let thumb = img.thumbnail(320, 180).to_rgba8();
                    let size = [thumb.width() as usize, thumb.height() as usize];
                    (size, thumb.into_raw())
                });
            let (size, rgba) = decoded.unwrap_or(([0, 0], Vec::new()));
            let _ = tx.send(AppEvent::ScreenshotThumbLoaded {
                path: path_owned,
                size,
                rgba,
            });
            ctx_c.request_repaint();
        });
    }

    fn decode_image_bytes_to_texture(
        key: &str,
        bytes: &[u8],
        max_dim: u32,
        ctx: &egui::Context,
    ) -> Option<egui::TextureHandle> {
        let img = image::load_from_memory(bytes).ok()?;
        let rgba = img.thumbnail(max_dim, max_dim).to_rgba8();
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        Some(ctx.load_texture(key.to_string(), color, egui::TextureOptions::LINEAR))
    }

    pub(crate) fn load_world_icon_texture(
        &mut self,
        icon_path: &std::path::Path,
        ctx: &egui::Context,
    ) -> Option<egui::TextureHandle> {
        if let Some(cached) = self.world_icon_textures.get(icon_path) {
            return cached.clone();
        }
        let key = format!("world_icon_{}", icon_path.display());
        let tex = std::fs::read(icon_path)
            .ok()
            .and_then(|bytes| Self::decode_image_bytes_to_texture(&key, &bytes, 96, ctx));
        self.world_icon_textures
            .insert(icon_path.to_path_buf(), tex.clone());
        tex
    }

    pub(crate) fn load_pack_icon_texture(
        &mut self,
        pack: &ContentPackInfo,
        ctx: &egui::Context,
    ) -> Option<egui::TextureHandle> {
        if let Some(cached) = self.pack_icon_textures.get(&pack.path) {
            return cached.clone();
        }
        let key = format!("pack_icon_{}", pack.path.display());
        let tex = pack
            .icon_png
            .as_deref()
            .and_then(|bytes| Self::decode_image_bytes_to_texture(&key, bytes, 64, ctx));
        self.pack_icon_textures
            .insert(pack.path.clone(), tex.clone());
        tex
    }

    pub(crate) fn start_check_content_pack_updates(
        &mut self,
        kind: ContentKind,
        ctx: &egui::Context,
    ) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        if self.checking_pack_updates {
            return;
        }
        if self.connectivity_status == ConnectivityStatus::Offline {
            self.notice_banner = Some((
                "Offline mode: connect to the internet to check pack updates.".to_string(),
                true,
            ));
            return;
        }
        self.checking_pack_updates = true;
        self.pack_updates_kind = Some(kind);
        self.pack_update_candidates.clear();

        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let cache_path = self.paths.cache_root.join("providers.sqlite");
        let folder = kind.folder_name().to_string();
        let loaders: Vec<&'static str> = match kind {
            ContentKind::ShaderPack => vec!["iris", "optifine", "canvas", "vanilla", "minecraft"],
            ContentKind::ResourcePack | ContentKind::DataPack => vec!["minecraft"],
        };
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
            let candidates = if let Ok(mr) = ModrinthClient::new(cache) {
                ModUpdateManager::check_content_pack_updates(
                    &mc_dir,
                    &folder,
                    &loaders,
                    &inst.version_id,
                    &mr,
                )
                .await
            } else {
                Vec::new()
            };
            let _ = tx.send(AppEvent::ContentPackUpdatesChecked {
                instance_id: inst.instance_id,
                kind,
                candidates,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_apply_content_pack_updates(
        &mut self,
        kind: ContentKind,
        ctx: &egui::Context,
    ) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        if self.pack_updates_kind != Some(kind) {
            return;
        }
        let candidates = self.pack_update_candidates.clone();
        if !candidates.iter().any(|c| c.update_available) {
            return;
        }
        let (task_id, task_ctrl) = self.begin_task_with_control(
            format!("Update {} ({})", kind.folder_name(), inst.name),
            "Downloading pack updates...",
        );
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let folder = kind.folder_name().to_string();
        let downloader = self.download_client.with_task_control(task_ctrl);
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = ModUpdateManager::apply_content_pack_updates(
                &mc_dir,
                &folder,
                &candidates,
                &downloader,
            )
            .await
            .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::ContentPackUpdatesApplied {
                task_id,
                instance_id: inst.instance_id,
                kind,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn toggle_mod_cart_item(&mut self, item: ModCartItem) {
        toggle_mod_cart_item_in_list(&mut self.mod_cart, item);
        self.mod_cart_preview.clear();
    }

    pub(crate) fn open_mod_cart_modal(&mut self, ctx: &egui::Context) {
        let valid_current = self
            .mod_cart_target_instance_id
            .as_deref()
            .and_then(|id| self.instances.iter().find(|i| i.instance_id == id))
            .is_some_and(|i| !i.mod_loader.is_vanilla());
        if !valid_current {
            self.mod_cart_target_instance_id = self
                .selected_instance()
                .filter(|i| !i.mod_loader.is_vanilla())
                .or_else(|| self.instances.iter().find(|i| !i.mod_loader.is_vanilla()))
                .map(|i| i.instance_id.clone());
        }
        self.show_mod_cart_modal = true;
        self.start_resolve_mod_cart_preview(ctx);
    }

    pub(crate) fn start_resolve_mod_cart_preview(&mut self, ctx: &egui::Context) {
        let Some(target_id) = self.mod_cart_target_instance_id.clone() else {
            self.mod_cart_resolving = false;
            self.mod_cart_preview.clear();
            return;
        };
        let Some(inst) = self
            .instances
            .iter()
            .find(|i| i.instance_id == target_id && !i.mod_loader.is_vanilla())
            .cloned()
        else {
            self.mod_cart_resolving = false;
            self.mod_cart_preview.clear();
            return;
        };
        if self.mod_cart.is_empty() || self.connectivity_status == ConnectivityStatus::Offline {
            self.mod_cart_resolving = false;
            self.mod_cart_preview.clear();
            return;
        }

        self.mod_cart_request_id = self.mod_cart_request_id.wrapping_add(1);
        let request_id = self.mod_cart_request_id;
        self.mod_cart_resolving = true;
        self.mod_cart_preview.clear();

        let cart = self.mod_cart.clone();
        let cache_path = self.paths.cache_root.join("providers.sqlite");
        let cf_api_key = self.resolved_cf_api_key();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            use std::collections::{HashMap, HashSet, VecDeque};

            let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
            let mr_opt = ModrinthClient::new(cache.clone()).ok();
            let cf_opt = CurseForgeClient::new(cf_api_key, cache).ok();

            let mut cart_mr_projects: HashSet<String> = HashSet::new();
            let mut cart_cf_ids: HashSet<u64> = HashSet::new();
            for item in &cart {
                match &item.provider {
                    CartModProvider::Modrinth { slug } => {
                        cart_mr_projects.insert(slug.clone());
                    }
                    CartModProvider::CurseForge { mod_id } => {
                        cart_cf_ids.insert(*mod_id);
                    }
                }
            }

            let mut seen_mr: HashSet<String> = HashSet::new();
            let mut seen_cf: HashSet<u64> = HashSet::new();
            let mut mr_dep_queue: VecDeque<(String, Option<String>, String)> = VecDeque::new();
            let mut cf_dep_queue: VecDeque<(u64, String)> = VecDeque::new();
            let mut entries: Vec<CartResolvedEntry> = Vec::with_capacity(cart.len());

            for item in &cart {
                match &item.provider {
                    CartModProvider::Modrinth { slug } => {
                        let Some(ref mr) = mr_opt else {
                            entries.push(CartResolvedEntry {
                                provider: item.provider.clone(),
                                title: item.title.clone(),
                                is_dependency: false,
                                required_by: None,
                                resolved_version: None,
                                resolved_file_name: None,
                                error: Some("Modrinth client unavailable.".to_string()),
                            });
                            continue;
                        };
                        match mr
                            .list_versions(
                                slug,
                                Some(&inst.mod_loader.name),
                                Some(&inst.version_id),
                            )
                            .await
                        {
                            Ok(list) => {
                                if let Some(ver) = list.into_iter().next() {
                                    cart_mr_projects.insert(ver.project_id.clone());
                                    seen_mr.insert(ver.project_id.clone());
                                    let fname = ver.primary_file().map(|f| f.filename.clone());
                                    let err = if fname.is_none() {
                                        Some("No downloadable file found.".to_string())
                                    } else {
                                        None
                                    };
                                    entries.push(CartResolvedEntry {
                                        provider: item.provider.clone(),
                                        title: item.title.clone(),
                                        is_dependency: false,
                                        required_by: None,
                                        resolved_version: Some(ver.version_number.clone()),
                                        resolved_file_name: fname,
                                        error: err,
                                    });
                                    for dep in &ver.dependencies {
                                        if dep.dependency_type.eq_ignore_ascii_case("required") {
                                            if let Some(ref pid) = dep.project_id {
                                                mr_dep_queue.push_back((
                                                    pid.clone(),
                                                    dep.version_id.clone(),
                                                    item.title.clone(),
                                                ));
                                            } else if let Some(ref vid) = dep.version_id {
                                                mr_dep_queue.push_back((
                                                    String::new(),
                                                    Some(vid.clone()),
                                                    item.title.clone(),
                                                ));
                                            }
                                        }
                                    }
                                } else {
                                    entries.push(CartResolvedEntry {
                                        provider: item.provider.clone(),
                                        title: item.title.clone(),
                                        is_dependency: false,
                                        required_by: None,
                                        resolved_version: None,
                                        resolved_file_name: None,
                                        error: Some(format!(
                                            "No compatible version for MC {} ({})",
                                            inst.version_id, inst.mod_loader.name
                                        )),
                                    });
                                }
                            }
                            Err(err) => {
                                entries.push(CartResolvedEntry {
                                    provider: item.provider.clone(),
                                    title: item.title.clone(),
                                    is_dependency: false,
                                    required_by: None,
                                    resolved_version: None,
                                    resolved_file_name: None,
                                    error: Some(err.to_string()),
                                });
                            }
                        }
                    }
                    CartModProvider::CurseForge { mod_id } => {
                        let Some(ref cf) = cf_opt else {
                            entries.push(CartResolvedEntry {
                                provider: item.provider.clone(),
                                title: item.title.clone(),
                                is_dependency: false,
                                required_by: None,
                                resolved_version: None,
                                resolved_file_name: None,
                                error: Some("CurseForge client unavailable.".to_string()),
                            });
                            continue;
                        };
                        match cf
                            .get_mod_files(
                                *mod_id,
                                Some(&inst.version_id),
                                Some(&inst.mod_loader.name),
                            )
                            .await
                        {
                            Ok(files) => {
                                if let Some(f) = files.into_iter().next() {
                                    seen_cf.insert(*mod_id);
                                    entries.push(CartResolvedEntry {
                                        provider: item.provider.clone(),
                                        title: item.title.clone(),
                                        is_dependency: false,
                                        required_by: None,
                                        resolved_version: Some(f.display_name.clone()),
                                        resolved_file_name: Some(f.file_name.clone()),
                                        error: None,
                                    });
                                    for dep in &f.dependencies {
                                        if dep.relation_type == 3 {
                                            cf_dep_queue
                                                .push_back((dep.mod_id, item.title.clone()));
                                        }
                                    }
                                } else {
                                    entries.push(CartResolvedEntry {
                                        provider: item.provider.clone(),
                                        title: item.title.clone(),
                                        is_dependency: false,
                                        required_by: None,
                                        resolved_version: None,
                                        resolved_file_name: None,
                                        error: Some(format!(
                                            "No compatible file for MC {} ({})",
                                            inst.version_id, inst.mod_loader.name
                                        )),
                                    });
                                }
                            }
                            Err(err) => {
                                entries.push(CartResolvedEntry {
                                    provider: item.provider.clone(),
                                    title: item.title.clone(),
                                    is_dependency: false,
                                    required_by: None,
                                    resolved_version: None,
                                    resolved_file_name: None,
                                    error: Some(err.to_string()),
                                });
                            }
                        }
                    }
                }
            }

            if let Some(ref mr) = mr_opt {
                while let Some((proj, pinned_vid, req_by)) = mr_dep_queue.pop_front() {
                    if entries.len() >= 120 {
                        break;
                    }
                    if !proj.is_empty()
                        && (cart_mr_projects.contains(&proj) || seen_mr.contains(&proj))
                    {
                        continue;
                    }
                    let ver_res = if let Some(ref vid) = pinned_vid {
                        mr.get_version(vid).await.map(Some)
                    } else {
                        mr.list_versions(&proj, Some(&inst.mod_loader.name), Some(&inst.version_id))
                            .await
                            .map(|list| list.into_iter().next())
                    };
                    match ver_res {
                        Ok(Some(ver)) => {
                            if cart_mr_projects.contains(&ver.project_id)
                                || !seen_mr.insert(ver.project_id.clone())
                            {
                                continue;
                            }
                            let slug_or_pid = if proj.is_empty() {
                                ver.project_id.clone()
                            } else {
                                proj.clone()
                            };
                            let fname = ver.primary_file().map(|f| f.filename.clone());
                            let dep_title = if !ver.name.trim().is_empty() {
                                ver.name.clone()
                            } else if let Some(ref f) = fname {
                                f.clone()
                            } else {
                                slug_or_pid.clone()
                            };
                            entries.push(CartResolvedEntry {
                                provider: CartModProvider::Modrinth { slug: slug_or_pid },
                                title: dep_title,
                                is_dependency: true,
                                required_by: Some(req_by.clone()),
                                resolved_version: Some(ver.version_number.clone()),
                                resolved_file_name: fname,
                                error: None,
                            });
                            for dep in &ver.dependencies {
                                if dep.dependency_type.eq_ignore_ascii_case("required") {
                                    if let Some(ref pid) = dep.project_id {
                                        if !seen_mr.contains(pid) && !cart_mr_projects.contains(pid)
                                        {
                                            mr_dep_queue.push_back((
                                                pid.clone(),
                                                dep.version_id.clone(),
                                                req_by.clone(),
                                            ));
                                        }
                                    } else if let Some(ref vid) = dep.version_id {
                                        mr_dep_queue.push_back((
                                            String::new(),
                                            Some(vid.clone()),
                                            req_by.clone(),
                                        ));
                                    }
                                }
                            }
                        }
                        _ if !proj.is_empty() && seen_mr.insert(proj.clone()) => {
                            entries.push(CartResolvedEntry {
                                provider: CartModProvider::Modrinth { slug: proj.clone() },
                                title: format!("Dependency ({proj})"),
                                is_dependency: true,
                                required_by: Some(req_by),
                                resolved_version: None,
                                resolved_file_name: None,
                                error: Some(
                                    "Could not pre-resolve required dependency version."
                                        .to_string(),
                                ),
                            });
                        }
                        _ => {}
                    }
                }
            }

            if let Some(ref cf) = cf_opt {
                struct CfDepResolved {
                    mod_id: u64,
                    required_by: String,
                    version: Option<String>,
                    file_name: Option<String>,
                    error: Option<String>,
                }
                let mut cf_deps_resolved: Vec<CfDepResolved> = Vec::new();
                while let Some((dep_id, req_by)) = cf_dep_queue.pop_front() {
                    if cf_deps_resolved.len() >= 40 {
                        break;
                    }
                    if cart_cf_ids.contains(&dep_id) || !seen_cf.insert(dep_id) {
                        continue;
                    }
                    match cf
                        .get_mod_files(dep_id, Some(&inst.version_id), Some(&inst.mod_loader.name))
                        .await
                    {
                        Ok(files) => {
                            if let Some(f) = files.into_iter().next() {
                                for sub in &f.dependencies {
                                    if sub.relation_type == 3 && !seen_cf.contains(&sub.mod_id) {
                                        cf_dep_queue.push_back((sub.mod_id, req_by.clone()));
                                    }
                                }
                                cf_deps_resolved.push(CfDepResolved {
                                    mod_id: dep_id,
                                    required_by: req_by,
                                    version: Some(f.display_name.clone()),
                                    file_name: Some(f.file_name.clone()),
                                    error: None,
                                });
                            } else {
                                cf_deps_resolved.push(CfDepResolved {
                                    mod_id: dep_id,
                                    required_by: req_by,
                                    version: None,
                                    file_name: None,
                                    error: Some("No compatible dependency file found.".to_string()),
                                });
                            }
                        }
                        Err(e) => {
                            cf_deps_resolved.push(CfDepResolved {
                                mod_id: dep_id,
                                required_by: req_by,
                                version: None,
                                file_name: None,
                                error: Some(e.to_string()),
                            });
                        }
                    }
                }
                let dep_ids: Vec<u64> = cf_deps_resolved.iter().map(|d| d.mod_id).collect();
                let mut title_map: HashMap<u64, String> = HashMap::new();
                if !dep_ids.is_empty() {
                    if let Ok(mods_meta) = cf.get_mods_batch(&dep_ids).await {
                        for m in mods_meta {
                            title_map.insert(m.id, m.name);
                        }
                    }
                }
                for dep in cf_deps_resolved {
                    let title = title_map
                        .get(&dep.mod_id)
                        .cloned()
                        .unwrap_or_else(|| format!("CurseForge Mod #{}", dep.mod_id));
                    entries.push(CartResolvedEntry {
                        provider: CartModProvider::CurseForge { mod_id: dep.mod_id },
                        title,
                        is_dependency: true,
                        required_by: Some(dep.required_by),
                        resolved_version: dep.version,
                        resolved_file_name: dep.file_name,
                        error: dep.error,
                    });
                }
            }

            let _ = tx.send(AppEvent::ModCartPreviewReady {
                request_id,
                instance_id: inst.instance_id,
                entries,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_install_mod_cart(&mut self, ctx: &egui::Context) {
        let Some(target_id) = self.mod_cart_target_instance_id.clone() else {
            return;
        };
        let Some(inst) = self
            .instances
            .iter()
            .find(|i| i.instance_id == target_id && !i.mod_loader.is_vanilla())
            .cloned()
        else {
            self.notice_banner = Some((
                "Select a valid modded instance to install the cart.".to_string(),
                true,
            ));
            return;
        };
        if self.mod_cart.is_empty() {
            return;
        }
        let cart = self.mod_cart.clone();
        let total = cart.len() as u64;
        let (task_id, task_ctrl) = self.begin_task_with_control(
            format!("Batch Mod Install ({})", cart.len()),
            format!("Preparing {} mod(s) for {}...", cart.len(), inst.name),
        );
        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let mods_dir = self.paths.minecraft_dir(&inst.instance_id).join("mods");
        let cache_path = self.paths.cache_root.join("providers.sqlite");
        let cf_api_key = self.resolved_cf_api_key();
        let downloader = self.download_client.with_task_control(task_ctrl.clone());
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let _ = std::fs::create_dir_all(&mods_dir);
            let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
            let mr_res = ModrinthClient::new(cache.clone());
            let cf_res = CurseForgeClient::new(cf_api_key, cache);

            let mut installed_count = 0usize;
            let mut dep_count = 0usize;
            let mut failed: Vec<String> = Vec::new();

            for (idx, item) in cart.iter().enumerate() {
                if task_ctrl.is_cancelled() {
                    failed.push(format!("{}: Cancelled", item.title));
                    continue;
                }
                let _ = tx.send(AppEvent::TaskProgress {
                    task_id,
                    message: format!("Installing {} ({}/{})...", item.title, idx + 1, total),
                    current: idx as u64,
                    total,
                });
                ctx_c.request_repaint();

                match &item.provider {
                    CartModProvider::Modrinth { slug } => match &mr_res {
                        Ok(mr) => {
                            match mr
                                .install_with_dependencies(
                                    slug,
                                    &inst.version_id,
                                    &inst.mod_loader.name,
                                    &mods_dir,
                                    &downloader,
                                )
                                .await
                            {
                                Ok(files) => {
                                    installed_count += 1;
                                    dep_count += files.iter().filter(|f| f.is_dependency).count();
                                    for f in files {
                                        let file_name = f
                                            .file_path
                                            .file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("")
                                            .to_string();
                                        if !file_name.is_empty() {
                                            let title = if f.is_dependency {
                                                file_name.trim_end_matches(".jar").to_string()
                                            } else {
                                                item.title.clone()
                                            };
                                            let _ = ModProvenanceRegistry::record_install(
                                                &inst_dir,
                                                TrackedModEntry {
                                                    project_id: f.project_id,
                                                    provider: "modrinth".to_string(),
                                                    title,
                                                    file_name,
                                                    version_id: f.version_number.clone(),
                                                    version_number: f.version_number,
                                                    locked: false,
                                                    managed_by: None,
                                                },
                                            );
                                        }
                                    }
                                }
                                Err(err) => {
                                    failed.push(format!("{}: {}", item.title, err));
                                }
                            }
                        }
                        Err(err) => failed.push(format!("{}: {}", item.title, err)),
                    },
                    CartModProvider::CurseForge { mod_id } => match &cf_res {
                        Ok(cf) => {
                            match cf
                                .install_mod(
                                    *mod_id,
                                    None,
                                    &inst.version_id,
                                    &inst.mod_loader.name,
                                    &mods_dir,
                                    &downloader,
                                )
                                .await
                            {
                                Ok(dest_path) => {
                                    installed_count += 1;
                                    let file_name = dest_path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("")
                                        .to_string();
                                    if !file_name.is_empty() {
                                        let _ = ModProvenanceRegistry::record_install(
                                            &inst_dir,
                                            TrackedModEntry {
                                                project_id: mod_id.to_string(),
                                                provider: "curseforge".to_string(),
                                                title: item.title.clone(),
                                                file_name,
                                                version_id: String::new(),
                                                version_number: String::new(),
                                                locked: false,
                                                managed_by: None,
                                            },
                                        );
                                    }
                                }
                                Err(err) => {
                                    failed.push(format!("{}: {}", item.title, err));
                                }
                            }
                        }
                        Err(err) => failed.push(format!("{}: {}", item.title, err)),
                    },
                }
            }

            let _ = tx.send(AppEvent::TaskProgress {
                task_id,
                message: "Finalizing batch install...".to_string(),
                current: total,
                total,
            });
            let _ = tx.send(AppEvent::ModCartInstallFinished {
                task_id,
                instance_id: inst.instance_id,
                installed_count,
                dep_count,
                failed,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn open_universal_import_modal(&mut self, ctx: &egui::Context) {
        self.show_universal_import_modal = true;
        self.universal_import_error = None;
        if self.universal_import_path.trim().is_empty() {
            let detected = detect_default_launcher_roots();
            if let Some((kind, path)) = detected.into_iter().next() {
                self.universal_import_kind = kind;
                self.universal_import_path = path.display().to_string();
            } else if let Some(cand) = default_root_candidates_for_kind(self.universal_import_kind)
                .into_iter()
                .next()
            {
                self.universal_import_path = cand.display().to_string();
            }
        }
        let trimmed = self.universal_import_path.trim();
        if !trimmed.is_empty() && std::path::Path::new(trimmed).is_dir() {
            self.start_universal_import_scan(ctx);
        }
    }

    pub(crate) fn switch_universal_import_kind(
        &mut self,
        kind: ExternalLauncherKind,
        ctx: &egui::Context,
    ) {
        self.universal_import_kind = kind;
        self.universal_import_items.clear();
        self.universal_import_selected.clear();
        self.universal_import_error = None;

        let candidates = default_root_candidates_for_kind(kind);
        let picked = candidates
            .iter()
            .find(|p| p.is_dir())
            .cloned()
            .or_else(|| candidates.into_iter().next());
        self.universal_import_path = picked.map(|p| p.display().to_string()).unwrap_or_default();

        let trimmed = self.universal_import_path.trim();
        if !trimmed.is_empty() && std::path::Path::new(trimmed).is_dir() {
            self.start_universal_import_scan(ctx);
        }
    }

    pub(crate) fn start_universal_import_scan(&mut self, ctx: &egui::Context) {
        if self.universal_import_scanning {
            return;
        }
        let raw_root = self.universal_import_path.trim().to_string();
        if raw_root.is_empty() {
            self.universal_import_error =
                Some("Enter or browse to a launcher folder to scan.".to_string());
            return;
        }
        self.universal_import_scanning = true;
        self.universal_import_error = None;
        let kind = self.universal_import_kind;
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn_blocking(move || {
            let res = inspect_external_launcher(kind, std::path::Path::new(&raw_root))
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::UniversalImportScanned(res));
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn start_universal_import(&mut self, ctx: &egui::Context) {
        if self.universal_import_importing {
            return;
        }
        let selected: Vec<DiscoveredExternalInstance> = self
            .universal_import_items
            .iter()
            .filter(|i| self.universal_import_selected.contains(&i.id_key))
            .cloned()
            .collect();
        if selected.is_empty() {
            self.universal_import_error =
                Some("Select at least one instance to import.".to_string());
            return;
        }
        self.universal_import_importing = true;
        self.universal_import_error = None;
        let task_id = self.begin_task(
            format!("Import {} External Instance(s)", selected.len()),
            format!(
                "Copying mods, configs, and worlds from {}...",
                self.universal_import_kind.label()
            ),
        );
        let dest_paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn_blocking(move || {
            let res = import_external_instances(&dest_paths, &selected).map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::UniversalImportFinished {
                task_id,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }

    pub(crate) fn open_browser_inline_detail(&mut self, key: &str, ctx: &egui::Context) {
        self.browser_selected_key = Some(key.to_string());
        self.browser_inline_detail_open = true;
        if self.browser_detail.as_ref().is_some_and(|d| d.key == key) {
            return;
        }
        if self.browser_detail_loading_key.as_deref() == Some(key) {
            return;
        }

        if let Some(slug) = key.strip_prefix("mr:") {
            let key_owned = key.to_string();
            let slug_owned = slug.to_string();
            let fallback_desc = self
                .browser_mr_results
                .iter()
                .find(|h| h.slug == slug_owned)
                .map(|h| h.description.clone())
                .unwrap_or_default();
            self.browser_detail = None;
            self.browser_detail_loading_key = Some(key_owned.clone());
            let cache_path = self
                .paths
                .cache_root
                .join("providers")
                .join("api_cache.sqlite");
            let tx = self.tx.clone();
            let ctx_c = ctx.clone();

            self.runtime.spawn(async move {
                let res = async {
                    let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                    let mr = ModrinthClient::new(cache).map_err(|e| e.to_string())?;
                    let proj = mr
                        .get_project(&slug_owned)
                        .await
                        .map_err(|e| e.to_string())?;
                    let raw_body = if proj.body.trim().is_empty() {
                        if proj.description.trim().is_empty() {
                            fallback_desc
                        } else {
                            proj.description
                        }
                    } else {
                        proj.body
                    };
                    let mut versions: Vec<String> =
                        proj.game_versions.into_iter().rev().take(10).collect();
                    versions.reverse();
                    let gallery_urls: Vec<String> = proj
                        .gallery
                        .into_iter()
                        .filter(|g| !g.url.trim().is_empty())
                        .take(6)
                        .map(|g| g.url)
                        .collect();
                    Ok::<_, String>(BrowserDetailPayload {
                        key: key_owned.clone(),
                        body_excerpt: clean_project_markdown_preview(&raw_body, 2800),
                        categories: proj.categories,
                        loaders: proj.loaders,
                        game_versions: versions,
                        client_side: proj.client_side,
                        server_side: proj.server_side,
                        gallery_urls,
                        source_url: proj.source_url,
                        issues_url: proj.issues_url,
                    })
                }
                .await;
                let _ = tx.send(AppEvent::ContentDetailLoaded {
                    key: key_owned,
                    result: res,
                });
                ctx_c.request_repaint();
            });
            return;
        }

        if let Some(id_str) = key.strip_prefix("cf:") {
            let Ok(mod_id) = id_str.parse::<u64>() else {
                return;
            };
            let key_owned = key.to_string();
            let found = self.browser_cf_results.iter().find(|m| m.id == mod_id);
            let fallback_desc = found.map(|m| m.summary.clone()).unwrap_or_default();
            let gallery_urls: Vec<String> = found
                .map(|m| {
                    m.screenshots
                        .iter()
                        .filter_map(|s| s.thumbnail_url.clone().or_else(|| s.url.clone()))
                        .filter(|u| !u.trim().is_empty())
                        .take(6)
                        .collect()
                })
                .unwrap_or_default();
            for url in &gallery_urls {
                self.ensure_thumbnail(url, ctx);
            }
            self.browser_detail = None;
            self.browser_detail_loading_key = Some(key_owned.clone());
            let api_key = self.resolved_cf_api_key();
            let cache_path = self
                .paths
                .cache_root
                .join("providers")
                .join("api_cache.sqlite");
            let tx = self.tx.clone();
            let ctx_c = ctx.clone();

            self.runtime.spawn(async move {
                let res = async {
                    let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                    let cf = CurseForgeClient::new(api_key, cache).map_err(|e| e.to_string())?;
                    let body = cf
                        .get_mod_description(mod_id)
                        .await
                        .unwrap_or_else(|_| fallback_desc.clone());
                    let body_excerpt = if body.trim().is_empty() {
                        fallback_desc
                    } else {
                        body
                    };
                    Ok::<_, String>(BrowserDetailPayload {
                        key: key_owned.clone(),
                        body_excerpt,
                        categories: Vec::new(),
                        loaders: Vec::new(),
                        game_versions: Vec::new(),
                        client_side: String::new(),
                        server_side: String::new(),
                        gallery_urls,
                        source_url: None,
                        issues_url: None,
                    })
                }
                .await;
                let _ = tx.send(AppEvent::ContentDetailLoaded {
                    key: key_owned,
                    result: res,
                });
                ctx_c.request_repaint();
            });
            return;
        }

        // Local metadata detail for FTB / ATLauncher / Technic packs
        self.browser_detail_loading_key = None;
        let payload =
            if let Some(ftb_id) = key.strip_prefix("ftb:").and_then(|s| s.parse::<u64>().ok()) {
                self.browser_ftb_results
                    .iter()
                    .find(|p| p.id == ftb_id)
                    .map(|p| BrowserDetailPayload {
                        key: key.to_string(),
                        body_excerpt: clean_project_markdown_preview(&p.synopsis, 2400),
                        ..Default::default()
                    })
            } else if let Some(atl_key) = key.strip_prefix("atl:") {
                self.browser_atl_results
                    .iter()
                    .find(|p| p.effective_safe_name() == atl_key)
                    .map(|p| {
                        let vers: Vec<String> = p
                            .versions
                            .iter()
                            .take(8)
                            .map(|v| v.minecraft.clone())
                            .collect();
                        BrowserDetailPayload {
                            key: key.to_string(),
                            body_excerpt: clean_project_markdown_preview(&p.description, 2400),
                            game_versions: vers,
                            ..Default::default()
                        }
                    })
            } else if let Some(tech_slug) = key.strip_prefix("tech:") {
                self.browser_technic_results
                    .iter()
                    .find(|p| p.slug == tech_slug)
                    .map(|p| BrowserDetailPayload {
                        key: key.to_string(),
                        body_excerpt: clean_project_markdown_preview(&p.description, 2400),
                        game_versions: if p.minecraft.is_empty() {
                            Vec::new()
                        } else {
                            vec![p.minecraft.clone()]
                        },
                        ..Default::default()
                    })
            } else {
                None
            };
        self.browser_detail = payload;
    }

    pub(crate) fn open_gallery_preview(&mut self, url: String, ctx: &egui::Context) {
        let trimmed = url.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        self.gallery_preview_url = Some(trimmed.clone());
        self.gallery_preview_error = None;
        if self
            .gallery_preview_texture
            .as_ref()
            .is_some_and(|(u, _)| u == &trimmed)
        {
            self.gallery_preview_loading = false;
            return;
        }
        self.gallery_preview_texture = None;
        self.gallery_preview_loading = true;
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let http = reqwest::Client::builder()
                    .user_agent(mcw_models::user_agent())
                    .timeout(Duration::from_secs(15))
                    .build()
                    .map_err(|e| e.to_string())?;
                let resp = http.get(&trimmed).send().await.map_err(|e| e.to_string())?;
                if !resp.status().is_success() {
                    return Err(format!("HTTP {}", resp.status()));
                }
                let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
                let decoded = tokio::task::spawn_blocking(move || {
                    let img = image::load_from_memory(&bytes)
                        .map_err(|e| format!("Invalid image data: {e}"))?;
                    let thumb = img.thumbnail(1280, 960).to_rgba8();
                    let size = [thumb.width() as usize, thumb.height() as usize];
                    Ok::<_, String>((size, thumb.into_raw()))
                })
                .await
                .map_err(|e| e.to_string())??;
                Ok::<_, String>(decoded)
            }
            .await;

            let _ = tx.send(AppEvent::GalleryPreviewLoaded {
                url: trimmed,
                result: res,
            });
            ctx_c.request_repaint();
        });
    }
}

/// Simplifies Markdown/HTML project descriptions into concise readable overview lines.
pub(crate) fn clean_project_markdown_preview(raw: &str, max_chars: usize) -> String {
    let stripped = mcw_providers::strip_html_to_preview(raw, max_chars.saturating_add(500));
    let mut lines = Vec::new();
    for line in stripped.lines() {
        let t = line.trim();
        // Skip pure markdown badge / image links such as [![...](...)](...) or ![alt](url)
        if t.starts_with("![") || t.starts_with("[![") || t == "---" || t == "***" {
            continue;
        }
        lines.push(t.to_string());
    }
    let joined = lines.join("\n").trim().to_string();
    if max_chars == 0 || joined.chars().count() <= max_chars {
        return joined;
    }
    let mut clipped: String = joined.chars().take(max_chars.saturating_sub(3)).collect();
    while clipped.ends_with(char::is_whitespace) {
        clipped.pop();
    }
    clipped.push_str("...");
    clipped
}

pub(crate) fn apply_lan_agent_option(settings: &mut InstanceSettings, enable: bool) {
    if enable {
        settings.offline_multiplayer_enabled = true;
        settings.lan_auth_mode = "private_offline".to_string();
        settings.lan_connection_provider = "manual".to_string();
    }
}

pub(crate) fn remove_mod_file_and_twins(mods_dir: &std::path::Path, file_name: &str) {
    let lower = file_name.to_ascii_lowercase();
    let base = if lower.ends_with(".disabled") {
        &file_name[..file_name.len() - ".disabled".len()]
    } else {
        file_name
    };
    let enabled = mods_dir.join(base);
    let disabled = mods_dir.join(format!("{base}.disabled"));
    let exact = mods_dir.join(file_name);
    let _ = std::fs::remove_file(exact);
    let _ = std::fs::remove_file(enabled);
    let _ = std::fs::remove_file(disabled);
}

fn split_mod_jar_ext(file_name: &str) -> (&str, &str) {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".jar.disabled") {
        (
            &file_name[..file_name.len() - ".jar.disabled".len()],
            ".jar.disabled",
        )
    } else if lower.ends_with(".jar") {
        (&file_name[..file_name.len() - ".jar".len()], ".jar")
    } else {
        (file_name, "")
    }
}

fn strip_numeric_dup_suffix(stem: &str) -> &str {
    let trimmed = stem.trim_end();
    if let Some(without_paren) = trimmed.strip_suffix(')') {
        if let Some(open_idx) = without_paren.rfind(" (") {
            let digits = &without_paren[open_idx + 2..];
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                return &without_paren[..open_idx];
            }
        }
    }
    trimmed
}

pub(crate) fn normalize_mod_stem(file_name: &str) -> String {
    let (stem_raw, _) = split_mod_jar_ext(file_name);
    let stem = strip_numeric_dup_suffix(stem_raw);
    let mut parts: Vec<String> = Vec::new();
    for raw_tok in stem.split(['-', '_', '+', ' ']) {
        let tok = raw_tok.trim();
        if tok.is_empty() {
            continue;
        }
        let lower = tok.to_ascii_lowercase();
        let ver_body = lower
            .strip_prefix("mc")
            .or_else(|| lower.strip_prefix('v'))
            .or_else(|| lower.strip_prefix('r'))
            .unwrap_or(&lower);
        let looks_like_version = ver_body.chars().next().is_some_and(|c| c.is_ascii_digit())
            && (lower != ver_body
                || ver_body.contains('.')
                || ver_body.chars().all(|c| c.is_ascii_digit()));
        if looks_like_version {
            break;
        }
        parts.push(lower);
    }
    while parts.len() > 1
        && matches!(
            parts.last().map(String::as_str),
            Some("fabric" | "forge" | "neoforge" | "quilt")
        )
    {
        parts.pop();
    }
    if parts.is_empty() {
        stem.to_ascii_lowercase()
    } else {
        parts.join("-")
    }
}

fn make_task_byte_cb(
    tx: std::sync::mpsc::Sender<AppEvent>,
    ctx: egui::Context,
    task_id: u64,
    label: String,
) -> impl Fn(u64, u64) + Send + Sync + 'static {
    use std::sync::atomic::{AtomicU64, Ordering};
    let last = Arc::new(AtomicU64::new(0));
    move |done, total| {
        if done == 0 && total == 0 {
            return;
        }
        let prev = last.load(Ordering::Relaxed);
        let finished = total > 0 && done >= total;
        if finished || prev == 0 || done.saturating_sub(prev) >= 128 * 1024 {
            last.store(done.max(1), Ordering::Relaxed);
            let msg = if total > 0 {
                format!(
                    "{label} ({:.1} / {:.1} MB)",
                    done as f64 / 1_048_576.0,
                    total as f64 / 1_048_576.0
                )
            } else {
                format!("{label} ({:.1} MB)", done as f64 / 1_048_576.0)
            };
            let _ = tx.send(AppEvent::TaskProgress {
                task_id,
                message: msg,
                current: done,
                total: total.max(done),
            });
            ctx.request_repaint();
        }
    }
}

pub(crate) fn resolve_unique_mod_filename(
    mods_dir: &std::path::Path,
    incoming_file_name: &str,
) -> String {
    let (stem_raw, ext) = split_mod_jar_ext(incoming_file_name);
    let base_stem = strip_numeric_dup_suffix(stem_raw);
    let mut idx = 1usize;
    loop {
        let candidate_stem = if idx == 1 {
            base_stem.to_string()
        } else {
            format!("{base_stem} ({idx})")
        };
        let jar_name = if ext == ".jar.disabled" {
            format!("{candidate_stem}.jar")
        } else {
            format!("{candidate_stem}{ext}")
        };
        let disabled_name = if ext == ".jar.disabled" || ext == ".jar" {
            format!("{candidate_stem}.jar.disabled")
        } else {
            format!("{candidate_stem}{ext}.disabled")
        };
        if !mods_dir.join(&jar_name).exists() && !mods_dir.join(&disabled_name).exists() {
            return format!("{candidate_stem}{ext}");
        }
        idx += 1;
    }
}

pub(crate) fn is_account_or_token_error(err: &str) -> bool {
    let low = err.to_ascii_lowercase();
    [
        "token",
        "expired",
        "invalid_grant",
        "unauthorized",
        "xsts",
        "xbox",
        "minecraft authentication",
        "refresh failed",
        "no refresh token",
        "missing refresh",
        "sign in again",
        "log in again",
        "401",
        "dpapi",
        "account authentication",
        "no active account",
    ]
    .iter()
    .any(|kw| low.contains(kw))
}

pub(crate) fn resolve_account_for_instance<'a>(
    accounts: &'a [StoredAccount],
    global_active: Option<&'a StoredAccount>,
    preferred_uuid: Option<&str>,
) -> Option<&'a StoredAccount> {
    if let Some(pref) = preferred_uuid.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(hit) = accounts.iter().find(|a| a.uuid == pref) {
            return Some(hit);
        }
    }
    global_active
}

/// Formats an instance's installed mod list as either a Markdown table or plain text.
pub(crate) fn format_modlist_export(
    instance_name: &str,
    mc_version: &str,
    loader: &str,
    mods: &[mcw_mods::ModInfo],
    as_markdown: bool,
) -> String {
    use std::fmt::Write as _;

    let mut sorted: Vec<&mcw_mods::ModInfo> = mods.iter().collect();
    sorted.sort_by(|a, b| {
        let an = if a.name.trim().is_empty() {
            &a.file_name
        } else {
            &a.name
        };
        let bn = if b.name.trim().is_empty() {
            &b.file_name
        } else {
            &b.name
        };
        an.to_ascii_lowercase()
            .cmp(&bn.to_ascii_lowercase())
            .then_with(|| a.file_name.cmp(&b.file_name))
    });

    let total = sorted.len();
    let enabled_count = sorted.iter().filter(|m| m.enabled).count();
    let disabled_count = total.saturating_sub(enabled_count);
    let loader_label = display_loader_name(loader);

    let mut out = String::new();
    if as_markdown {
        let _ = writeln!(out, "# {instance_name} — Modlist\n");
        let _ = writeln!(out, "- **Minecraft**: `{mc_version}`");
        let _ = writeln!(out, "- **Modloader**: {loader_label}");
        let _ = writeln!(
            out,
            "- **Total Mods**: {total} ({enabled_count} enabled, {disabled_count} disabled)\n"
        );
        if sorted.is_empty() {
            let _ = writeln!(out, "_No mods installed._");
            return out;
        }
        let _ = writeln!(out, "| Mod | Version | Author(s) | Status | File |");
        let _ = writeln!(out, "| --- | --- | --- | --- | --- |");
        for m in sorted {
            let name = if m.name.trim().is_empty() {
                m.file_name.as_str()
            } else {
                m.name.as_str()
            };
            let ver = if m.version.trim().is_empty() {
                "-"
            } else {
                m.version.as_str()
            };
            let authors = if m.authors.is_empty() {
                "-".to_string()
            } else {
                m.authors.join(", ")
            };
            let status = if m.enabled { "Enabled" } else { "Disabled" };
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | `{}` |",
                name.replace('|', "\\|"),
                ver.replace('|', "\\|"),
                authors.replace('|', "\\|"),
                status,
                m.file_name.replace('|', "\\|"),
            );
        }
    } else {
        let _ = writeln!(out, "Instance: {instance_name}");
        let _ = writeln!(out, "Minecraft: {mc_version}");
        let _ = writeln!(out, "Modloader: {loader_label}");
        let _ = writeln!(
            out,
            "Total Mods: {total} ({enabled_count} enabled, {disabled_count} disabled)\n"
        );
        if sorted.is_empty() {
            let _ = writeln!(out, "(No mods installed)");
            return out;
        }
        for m in sorted {
            let name = if m.name.trim().is_empty() {
                m.file_name.as_str()
            } else {
                m.name.as_str()
            };
            out.push_str("- ");
            out.push_str(name);
            if !m.version.trim().is_empty() {
                let _ = write!(out, " ({})", m.version.trim());
            }
            if !m.authors.is_empty() {
                let _ = write!(out, " by {}", m.authors.join(", "));
            }
            if !m.enabled {
                out.push_str(" [DISABLED]");
            }
            let _ = writeln!(out, " [{}]", m.file_name);
        }
    }
    out
}

/// Evaluates whether an installed mod matches the current text query and status filter.
pub(crate) fn matches_mod_filter(
    m: &mcw_mods::ModInfo,
    query: &str,
    status_filter: &str,
    loader_name: &str,
    has_update: bool,
) -> bool {
    let q = query.trim().to_ascii_lowercase();
    if !q.is_empty() {
        let name_hit = m.name.to_ascii_lowercase().contains(&q);
        let file_hit = m.file_name.to_ascii_lowercase().contains(&q);
        let id_hit = m.mod_id.to_ascii_lowercase().contains(&q);
        if !name_hit && !file_hit && !id_hit {
            return false;
        }
    }
    match status_filter {
        "enabled" => m.enabled,
        "disabled" => !m.enabled,
        "updates" => has_update,
        "incompatible" => !is_loader_compatible(m.loader, loader_name),
        _ => true,
    }
}

/// Sorts instances for the Home Library Grid View (`last_played`, `playtime`, or `name`).
pub(crate) fn sort_instances_for_grid(instances: &[Instance], sort_by: &str) -> Vec<Instance> {
    let mut out = instances.to_vec();
    out.sort_by(|a, b| match sort_by {
        "playtime" => b
            .total_playtime_seconds
            .cmp(&a.total_playtime_seconds)
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            }),
        "name" => a
            .name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.instance_id.cmp(&b.instance_id)),
        _ => b
            .last_played
            .cmp(&a.last_played)
            .then_with(|| b.total_playtime_seconds.cmp(&a.total_playtime_seconds))
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            }),
    });
    out
}

/// Toggles a mod item inside the download cart (deduplicating by provider identity).
/// Returns `true` if added, `false` if removed.
pub(crate) fn toggle_mod_cart_item_in_list(cart: &mut Vec<ModCartItem>, item: ModCartItem) -> bool {
    if let Some(idx) = cart.iter().position(|x| x.provider == item.provider) {
        cart.remove(idx);
        false
    } else {
        cart.push(item);
        true
    }
}

/// Evaluates whether a log line matches `"all"`, `"info"`, `"warn"`, or `"error"` level filters.
pub(crate) fn matches_log_level(line: &str, is_stderr: bool, level_filter: &str) -> bool {
    let up = line.to_ascii_uppercase();
    let is_err = is_stderr
        || up.contains("[ERROR]")
        || up.contains("/ERROR]")
        || up.contains("[FATAL]")
        || up.contains("/FATAL]")
        || up.contains("EXCEPTION");
    let is_warn = !is_err && (up.contains("[WARN]") || up.contains("/WARN]"));
    match level_filter {
        "error" => is_err,
        "warn" => is_warn,
        "info" => !is_err && !is_warn,
        _ => true,
    }
}

pub(crate) fn compute_local_instance_security(
    paths: &LauncherPaths,
    inst: &Instance,
    mitigate_minecraft2shell: bool,
    policy: &ResolvedSecurityPolicy,
) -> InstanceSecurityAssessment {
    let base_json = paths.version_json(&inst.version_id);
    let Ok(base_raw) = fs::read_to_string(&base_json) else {
        return InstanceSecurityAssessment {
            status: SecurityStatus::Unknown,
            status_label: SecurityStatus::Unknown.as_str().to_string(),
            headline: format!("Unknown — Metadata Pending ({})", inst.version_id),
            description: format!(
                "Unknown: Version metadata for Minecraft {} is not cached locally yet. MCW Security Manager will sync official Mojang checksums and verify FreeType libraries before launching.",
                inst.version_id
            ),
            freetype_status: FreetypeLibraryStatus::Unknown,
            should_attach_agent: false,
            agent_runtime_state: SecurityAgentRuntimeState::NotLoaded,
            policy_validity: policy.validity,
            reasons: vec!["Local version.json not yet cached on disk.".to_string()],
            recommended_action: Some(
                "Click Sync Mojang Security Libraries or Launch once to download and verify metadata."
                    .to_string(),
            ),
        };
    };

    let Ok(mut merged) = serde_json::from_str::<mcw_models::minecraft::VersionMetadata>(&base_raw)
    else {
        return InstanceSecurityAssessment {
            status: SecurityStatus::Unknown,
            status_label: SecurityStatus::Unknown.as_str().to_string(),
            headline: format!("Unknown — Malformed Metadata ({})", inst.version_id),
            description: format!(
                "Unknown: Cached version metadata for Minecraft {} could not be parsed cleanly. Re-sync Mojang Security Libraries to verify files.",
                inst.version_id
            ),
            freetype_status: FreetypeLibraryStatus::Unknown,
            should_attach_agent: false,
            agent_runtime_state: SecurityAgentRuntimeState::NotLoaded,
            policy_validity: policy.validity,
            reasons: vec!["Cached version.json could not be parsed.".to_string()],
            recommended_action: Some("Click Sync Mojang Security Libraries.".to_string()),
        };
    };

    // Include any cached modloader profile libraries if the instance uses Fabric/Quilt/Forge/NeoForge.
    if !inst.mod_loader.is_vanilla() {
        let loader_low = inst.mod_loader.name.trim().to_ascii_lowercase();
        let loader_ver = inst.mod_loader.version.trim().to_ascii_lowercase();
        let versions_dir = paths.cache_root.join("versions");
        if let Ok(entries) = fs::read_dir(&versions_dir) {
            for entry in entries.flatten() {
                let dir_name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if dir_name.contains(&inst.version_id.to_ascii_lowercase())
                    && dir_name.contains(&loader_low)
                    && (loader_ver.is_empty() || dir_name.contains(&loader_ver))
                {
                    let profile_json = entry
                        .path()
                        .join(format!("{}.json", entry.file_name().to_string_lossy()));
                    if let Ok(lraw) = fs::read_to_string(profile_json) {
                        if let Ok(lmeta) =
                            serde_json::from_str::<mcw_models::minecraft::VersionMetadata>(&lraw)
                        {
                            merged.libraries.extend(lmeta.libraries);
                            break;
                        }
                    }
                }
            }
        }
    }

    let java_major = merged
        .java_version
        .as_ref()
        .map(|j| j.major_version)
        .unwrap_or_else(|| {
            mcw_runtime::JavaMajorPolicy::required_for_minecraft(&inst.version_id, None)
                .unwrap_or(21)
        });

    SecurityManager::assess_instance(
        paths,
        &merged,
        &inst.instance_id,
        mitigate_minecraft2shell,
        java_major,
        policy,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        apply_lan_agent_option, format_modlist_export, is_account_or_token_error,
        matches_log_level, matches_mod_filter, normalize_mod_stem, resolve_account_for_instance,
        resolve_unique_mod_filename, sort_instances_for_grid, toggle_mod_cart_item_in_list,
        AccountSource, CartModProvider, Instance, InstanceSettings, ModCartItem, StoredAccount,
    };
    use mcw_mods::{ModInfo, ModLoaderTag};
    use std::path::PathBuf;

    #[test]
    fn filters_log_lines_by_level() {
        let info_line = "[12:00:00] [main/INFO]: Loading 42 mods";
        let warn_line = "[12:00:01] [main/WARN]: Missing optional config key";
        let err_line = "[12:00:02] [Render thread/ERROR]: Failed to bind shader";

        assert!(matches_log_level(info_line, false, "all"));
        assert!(matches_log_level(info_line, false, "info"));
        assert!(!matches_log_level(info_line, false, "warn"));
        assert!(!matches_log_level(info_line, false, "error"));

        assert!(matches_log_level(warn_line, false, "warn"));
        assert!(!matches_log_level(warn_line, false, "info"));
        assert!(!matches_log_level(warn_line, false, "error"));

        assert!(matches_log_level(err_line, false, "error"));
        assert!(matches_log_level("raw stderr line", true, "error"));
        assert!(!matches_log_level("raw stderr line", true, "info"));
    }

    #[test]
    fn mod_cart_deduplicates_and_toggles_items() {
        let mut cart = Vec::new();
        let sodium = ModCartItem {
            provider: CartModProvider::Modrinth {
                slug: "sodium".to_string(),
            },
            title: "Sodium".to_string(),
            icon_url: None,
        };
        let jei = ModCartItem {
            provider: CartModProvider::CurseForge { mod_id: 238222 },
            title: "JEI".to_string(),
            icon_url: None,
        };

        assert!(toggle_mod_cart_item_in_list(&mut cart, sodium.clone()));
        assert!(toggle_mod_cart_item_in_list(&mut cart, jei.clone()));
        assert_eq!(cart.len(), 2);

        // Toggling an existing provider entry removes it instead of duplicating
        assert!(!toggle_mod_cart_item_in_list(&mut cart, sodium));
        assert_eq!(cart.len(), 1);
        assert_eq!(cart[0].title, "JEI");
    }

    #[test]
    fn sorts_instances_for_library_grid() {
        let mut a = Instance::new("a", "Zeta Pack", "1.20.1");
        a.last_played = "2026-11-20T10:00:00Z".to_string();
        a.total_playtime_seconds = 100;

        let mut b = Instance::new("b", "Alpha Pack", "1.20.1");
        b.last_played = "2026-11-24T10:00:00Z".to_string();
        b.total_playtime_seconds = 50;

        let mut c = Instance::new("c", "Middle Pack", "1.20.1");
        c.last_played = String::new();
        c.total_playtime_seconds = 900;

        let list = vec![a, b, c];

        let by_recent: Vec<String> = sort_instances_for_grid(&list, "last_played")
            .into_iter()
            .map(|i| i.instance_id)
            .collect();
        assert_eq!(by_recent, vec!["b", "a", "c"]);

        let by_playtime: Vec<String> = sort_instances_for_grid(&list, "playtime")
            .into_iter()
            .map(|i| i.instance_id)
            .collect();
        assert_eq!(by_playtime, vec!["c", "a", "b"]);

        let by_name: Vec<String> = sort_instances_for_grid(&list, "name")
            .into_iter()
            .map(|i| i.instance_id)
            .collect();
        assert_eq!(by_name, vec!["b", "c", "a"]);
    }

    #[test]
    fn filters_mods_by_query_and_status() {
        let sodium = ModInfo {
            file_name: "sodium-0.5.8.jar".to_string(),
            path: PathBuf::from("sodium-0.5.8.jar"),
            enabled: true,
            size_bytes: 1024,
            mod_id: "sodium".to_string(),
            name: "Sodium".to_string(),
            version: "0.5.8".to_string(),
            description: String::new(),
            authors: Vec::new(),
            loader: ModLoaderTag::Fabric,
            required_java_major: 17,
            embedded_mod_ids: Vec::new(),
            depends: Vec::new(),
        };
        let forge_disabled = ModInfo {
            file_name: "jei-forge-1.20.jar.disabled".to_string(),
            path: PathBuf::from("jei-forge-1.20.jar.disabled"),
            enabled: false,
            size_bytes: 2048,
            mod_id: "jei".to_string(),
            name: "Just Enough Items".to_string(),
            version: "15.2.0".to_string(),
            description: String::new(),
            authors: Vec::new(),
            loader: ModLoaderTag::Forge,
            required_java_major: 17,
            embedded_mod_ids: Vec::new(),
            depends: Vec::new(),
        };

        assert!(matches_mod_filter(&sodium, "", "all", "fabric", false));
        assert!(matches_mod_filter(
            &sodium, "sod", "enabled", "fabric", false
        ));
        assert!(!matches_mod_filter(
            &sodium, "sod", "disabled", "fabric", false
        ));
        assert!(matches_mod_filter(
            &sodium, "0.5.8", "updates", "fabric", true
        ));
        assert!(!matches_mod_filter(
            &sodium, "0.5.8", "updates", "fabric", false
        ));
        assert!(matches_mod_filter(
            &forge_disabled,
            "jei",
            "disabled",
            "fabric",
            false
        ));
        assert!(matches_mod_filter(
            &forge_disabled,
            "just enough",
            "incompatible",
            "fabric",
            false
        ));
        assert!(!matches_mod_filter(
            &sodium,
            "",
            "incompatible",
            "fabric",
            false
        ));
    }

    #[test]
    fn formats_modlist_as_markdown_and_text() {
        let mods = vec![
            ModInfo {
                file_name: "sodium-0.5.8.jar".to_string(),
                path: PathBuf::from("sodium-0.5.8.jar"),
                enabled: true,
                size_bytes: 1024,
                mod_id: "sodium".to_string(),
                name: "Sodium".to_string(),
                version: "0.5.8".to_string(),
                description: String::new(),
                authors: vec!["JellySquid".to_string()],
                loader: ModLoaderTag::Fabric,
                required_java_major: 17,
                embedded_mod_ids: Vec::new(),
                depends: Vec::new(),
            },
            ModInfo {
                file_name: "alpha-lib-1.0.jar.disabled".to_string(),
                path: PathBuf::from("alpha-lib-1.0.jar.disabled"),
                enabled: false,
                size_bytes: 512,
                mod_id: "alpha-lib".to_string(),
                name: "Alpha Lib".to_string(),
                version: "1.0.0".to_string(),
                description: String::new(),
                authors: Vec::new(),
                loader: ModLoaderTag::Fabric,
                required_java_major: 17,
                embedded_mod_ids: Vec::new(),
                depends: Vec::new(),
            },
        ];

        let md = format_modlist_export("My Pack", "1.20.4", "fabric", &mods, true);
        assert!(md.contains("# My Pack — Modlist"));
        assert!(md.contains("**Total Mods**: 2 (1 enabled, 1 disabled)"));
        assert!(md.contains("| Alpha Lib | 1.0.0 | - | Disabled | `alpha-lib-1.0.jar.disabled` |"));
        assert!(md.contains("| Sodium | 0.5.8 | JellySquid | Enabled | `sodium-0.5.8.jar` |"));
        // Alpha Lib sorts before Sodium
        assert!(md.find("Alpha Lib").unwrap() < md.find("Sodium").unwrap());

        let txt = format_modlist_export("My Pack", "1.20.4", "fabric", &mods, false);
        assert!(txt.contains("Instance: My Pack"));
        assert!(txt.contains("- Alpha Lib (1.0.0) [DISABLED] [alpha-lib-1.0.jar.disabled]"));
        assert!(txt.contains("- Sodium (0.5.8) by JellySquid [sodium-0.5.8.jar]"));
    }

    #[test]
    fn duplicate_mod_helpers_normalize_stems_and_append_suffix() {
        assert_eq!(
            normalize_mod_stem("sodium-fabric-0.5.8+mc1.20.4.jar"),
            "sodium"
        );
        assert_eq!(
            normalize_mod_stem("sodium-fabric-0.5.11+mc1.20.4 (2).jar.disabled"),
            "sodium"
        );
        assert_eq!(
            normalize_mod_stem("3dskinlayers-fabric-1.6.2-mc1.20.4.jar"),
            "3dskinlayers"
        );
        assert_eq!(
            normalize_mod_stem("fabric-api-0.92.0+1.20.1.jar"),
            "fabric-api"
        );

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("sodium-0.5.8.jar"), b"1").unwrap();
        std::fs::write(dir.path().join("sodium-0.5.8 (2).jar.disabled"), b"2").unwrap();
        assert_eq!(
            resolve_unique_mod_filename(dir.path(), "sodium-0.5.8.jar"),
            "sodium-0.5.8 (3).jar"
        );
    }

    #[test]
    fn apply_lan_agent_option_enables_manual_offline_lan() {
        let mut settings = InstanceSettings::default();
        apply_lan_agent_option(&mut settings, false);
        assert!(!settings.offline_multiplayer_enabled);

        apply_lan_agent_option(&mut settings, true);
        assert!(settings.offline_multiplayer_enabled);
        assert_eq!(settings.lan_auth_mode, "private_offline");
        assert_eq!(settings.lan_connection_provider, "manual");
    }

    #[test]
    fn detects_account_and_token_expiration_errors() {
        assert!(is_account_or_token_error(
            "Microsoft token expired or refresh failed: invalid_grant"
        ));
        assert!(is_account_or_token_error(
            "No refresh token stored for account 'Player' (please log in again)"
        ));
        assert!(is_account_or_token_error("XSTS authentication failed: 401"));
        assert!(!is_account_or_token_error(
            "UnsupportedClassVersionError: Java 21 required"
        ));
        assert!(!is_account_or_token_error(
            "Checksum mismatch for asset.jar"
        ));
    }

    #[test]
    fn resolves_per_instance_account_override_with_global_fallback() {
        let a1 = StoredAccount {
            uuid: "u1".to_string(),
            username: "MainMSA".to_string(),
            account_type: AccountSource::Microsoft,
            skin_url: None,
            active: true,
        };
        let a2 = StoredAccount {
            uuid: "u2".to_string(),
            username: "LanAlt".to_string(),
            account_type: AccountSource::Offline,
            skin_url: None,
            active: false,
        };
        let list = vec![a1.clone(), a2.clone()];

        assert_eq!(
            resolve_account_for_instance(&list, Some(&list[0]), None).map(|a| a.uuid.as_str()),
            Some("u1")
        );
        assert_eq!(
            resolve_account_for_instance(&list, Some(&list[0]), Some("u2"))
                .map(|a| a.uuid.as_str()),
            Some("u2")
        );
        assert_eq!(
            resolve_account_for_instance(&list, Some(&list[0]), Some("missing"))
                .map(|a| a.uuid.as_str()),
            Some("u1")
        );
    }
}
