use super::platform::*;
use super::*;

impl McwApp {
    pub(crate) fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.lang.tr(key)
    }

    pub(crate) fn save_prefs(&mut self) {
        self.prefs.theme = match self.theme_mode {
            ThemeMode::Dark => "dark".to_string(),
            ThemeMode::Light => "light".to_string(),
        };
        self.prefs.language = self.lang.current_locale().to_string();
        self.prefs.save(&self.paths);
    }

    pub(crate) fn reload_instances(&mut self) {
        let store = InstanceStore::new(&self.paths);
        self.instances = store.list_instances().unwrap_or_default();
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

    pub(crate) fn open_manual_downloads_for(&mut self, instance_id: &str) {
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
                            "Imported {resolved} mod(s) for '{inst_id}'. All manual downloads complete — modpack is ready!"
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
        self.selected_mods = self.mods_cache.get(id).cloned().unwrap_or_default();
        self.selected_backups = self.backups_cache.get(id).cloned().unwrap_or_default();
        self.selected_worlds = self.worlds_cache.get(id).cloned().unwrap_or_default();
        self.selected_screenshots = self.screenshots_cache.get(id).cloned().unwrap_or_default();
        self.selected_content_packs = self
            .content_packs_cache
            .get(&(id.clone(), self.packs_kind))
            .cloned()
            .unwrap_or_default();
        if let Some(inst) = self.instances.iter().find(|i| &i.instance_id == id) {
            self.edit_runtime_version = inst.version_id.clone();
            self.edit_runtime_loader = inst.mod_loader.name.to_ascii_lowercase();
            self.edit_group_input = inst.group.clone();
            self.edit_tags_input = inst.tags.join(", ");
        }
        if let Some(ref s) = self.selected_settings {
            self.edit_jvm_args_text = s.jvm_arguments.join("\n");
            self.edit_game_args_text = s.game_arguments.join("\n");
        } else {
            self.edit_jvm_args_text.clear();
            self.edit_game_args_text.clear();
        }
        self.mod_update_candidates.clear();
    }

    pub(crate) fn ensure_active_tab_loaded(&mut self, ctx: &egui::Context) {
        let Some(id) = self.selected_instance_id.clone() else {
            return;
        };
        match self.active_tab {
            DetailTab::Mods => self.load_mods_async(&id, ctx, false),
            DetailTab::Packs => self.load_content_packs_async(&id, self.packs_kind, ctx, false),
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
                    && self.packs_kind == kind
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

    pub(crate) fn start_content_pack_search(&mut self, ctx: &egui::Context) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        self.packs_searching = true;
        let query = self.packs_query.clone();
        let kind = self.packs_kind;
        let is_cf = self.packs_provider == ModProviderTab::CurseForge;
        let api_key = self.resolved_cf_api_key();
        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
            if is_cf {
                let cf_res = async {
                    let client =
                        CurseForgeClient::new(api_key, cache).map_err(|e| e.to_string())?;
                    let search = client
                        .search(&CurseForgeSearchParams {
                            query,
                            kind: kind.curseforge_kind().to_string(),
                            loader: None,
                            game_version: Some(inst.version_id.clone()),
                            page_size: 12,
                            ..Default::default()
                        })
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok::<Vec<CurseForgeMod>, String>(search.data)
                }
                .await;
                let _ = tx.send(AppEvent::ContentPackSearchFinished {
                    provider_is_cf: true,
                    modrinth: Ok(Vec::new()),
                    curseforge: cf_res,
                });
            } else {
                let mr_res = async {
                    let mr = ModrinthClient::new(cache).map_err(|e| e.to_string())?;
                    let search = mr
                        .search(&ModrinthSearchParams {
                            query,
                            project_type: kind.modrinth_project_type().to_string(),
                            loader: None,
                            game_version: Some(inst.version_id.clone()),
                            limit: 12,
                            ..Default::default()
                        })
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok::<Vec<ModrinthSearchHit>, String>(search.hits)
                }
                .await;
                let _ = tx.send(AppEvent::ContentPackSearchFinished {
                    provider_is_cf: false,
                    modrinth: mr_res,
                    curseforge: Ok(Vec::new()),
                });
            }
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_content_pack_install(
        &mut self,
        provider_is_cf: bool,
        item_key: String,
        display_name: String,
        cf_mod_id: Option<u64>,
        ctx: &egui::Context,
    ) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        if !self.installing_content_packs.insert(item_key.clone()) {
            return;
        }
        let kind = self.packs_kind;
        let kind_label = match kind {
            ContentKind::ResourcePack => "Resource Pack",
            ContentKind::ShaderPack => "Shader Pack",
        };
        let task_id = self.begin_task(
            format!("{kind_label}: {display_name}"),
            format!("Downloading into {}...", inst.name),
        );
        let target_dir = self
            .paths
            .minecraft_dir(&inst.instance_id)
            .join(kind.folder_name());
        let api_key = self.resolved_cf_api_key();
        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let slug = item_key.clone();

        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let downloader = DownloadClient::new().map_err(|e| e.to_string())?;
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
        let id = instance_id.to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let mods = tokio::task::spawn_blocking(move || ModManager::new(&mods_dir).list())
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
        let control = TaskControl::new();
        self.active_tasks.push(ActiveTask {
            id,
            label: label.into(),
            message: message.into(),
            current: 0,
            total: 0,
            control: Arc::clone(&control),
        });
        (id, control)
    }

    pub(crate) fn finish_task(&mut self, task_id: u64) {
        self.active_tasks.retain(|t| t.id != task_id);
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
        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let client = DownloadClient::new().map_err(|e| e.to_string())?;
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
        self.start_launch_with_world(None, ctx);
    }

    pub(crate) fn start_launch_with_world(
        &mut self,
        quick_play_world: Option<String>,
        ctx: &egui::Context,
    ) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        let Some(settings) = self.selected_settings.clone() else {
            return;
        };
        let Some(account) = self.active_account().cloned() else {
            self.notice_banner = Some((
                "Add or sign in to an account before launching.".to_string(),
                true,
            ));
            self.show_account_modal = true;
            return;
        };

        let instance_id = inst.instance_id.clone();
        if self.is_instance_busy(&instance_id) {
            return;
        }

        // Lesson #8: Clear previous launch errors & logs for the new run.
        self.notice_banner = None;
        self.instance_logs.insert(instance_id.clone(), Vec::new());
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
        let tx = self.tx.clone();
        let ctx = ctx.clone();

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
                            .map_err(|e| e.to_string())?;
                        let ms = MicrosoftAuthClient::new().map_err(|e| e.to_string())?;
                        let refreshed = ms
                            .refresh_microsoft_token(&rt)
                            .await
                            .map_err(|e| e.to_string())?;
                        let login = ms
                            .complete_minecraft_chain(refreshed)
                            .await
                            .map_err(|e| e.to_string())?;
                        if let Some(ref new_rt) = login.refresh_token {
                            let _ = acc_store.store_refresh_token(&account.uuid, new_rt);
                        }
                        (login.authentication, AccountSource::Microsoft)
                    }
                };

                let client = DownloadClient::new()
                    .map_err(|e| e.to_string())?
                    .with_task_control(task_ctrl);
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

                prepared.spec.jvm_args =
                    LanAgentManager::sanitize_user_jvm_args(&prepared.spec.jvm_args);
                if LanAgentManager::is_enabled(
                    settings.offline_multiplayer_enabled,
                    &settings.lan_auth_mode,
                ) {
                    let agent_args = LanAgentManager::build_jvm_args(
                        &paths.cache_root.join("lan_agent"),
                        &paths.instance_dir(&inst.instance_id),
                        &inst.mod_loader.name,
                        &[],
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
                }

                let log_path = paths
                    .logs_root
                    .join(format!("{}-latest.log", inst.instance_id));
                let log_sink = Arc::new(ChannelLogSink {
                    instance_id: inst.instance_id.clone(),
                    tx: tx.clone(),
                    ctx: ctx.clone(),
                });
                let supervised =
                    SupervisedProcess::spawn(&prepared.spec, Some(&log_path), log_sink)
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
                Ok::<_, String>(report)
            }
            .await;

            let _ = tx.send(AppEvent::LaunchFinished {
                instance_id: inst.instance_id,
                report: result,
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

    pub(crate) fn start_modrinth_search(&mut self, ctx: &egui::Context) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        self.mod_searching = true;
        let query = self.mod_search_query.clone();
        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        let loader_filter = self.mod_loader_filter.clone();
        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let client = ModrinthClient::new(cache).map_err(|e| e.to_string())?;
                let loader = if loader_filter.is_empty() || loader_filter == "all" {
                    None
                } else {
                    Some(loader_filter)
                };
                let search = client
                    .search(&ModrinthSearchParams {
                        query,
                        project_type: "mod".to_string(),
                        loader,
                        game_version: Some(inst.version_id.clone()),
                        limit: 15,
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<Vec<ModrinthSearchHit>, String>(search.hits)
            }
            .await;
            let _ = tx.send(AppEvent::ModrinthSearchFinished(res));
            ctx.request_repaint();
        });
    }

    pub(crate) fn ensure_thumbnail(&mut self, url: &str, ctx: &egui::Context) {
        let u = url.trim();
        if u.is_empty()
            || self.thumb_textures.contains_key(u)
            || !self.loading_thumbs.insert(u.to_string())
        {
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
                        .corner_radius(CornerRadius::same(4)),
                );
                return;
            }
        }
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
        ui.painter().rect(
            rect,
            CornerRadius::same(4),
            p.elevated,
            Stroke::new(1.0_f32, p.border),
            egui::StrokeKind::Inside,
        );
    }

    pub(crate) fn open_modrinth_version_picker(
        &mut self,
        slug: String,
        title: String,
        icon_url: Option<String>,
        is_pack: bool,
        ctx: &egui::Context,
    ) {
        let target_key = format!("mr:{slug}");
        self.version_picker_open = true;
        self.version_picker_title = title;
        self.version_picker_icon = icon_url;
        self.version_picker_website_url = Some(format!(
            "https://modrinth.com/{}/{}",
            if is_pack { "modpack" } else { "mod" },
            slug
        ));
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
            self.version_picker_loader_filter = if self.pack_loader_filter.is_empty() {
                "all".to_string()
            } else {
                self.pack_loader_filter.to_ascii_lowercase()
            };
        } else {
            let inst = self.selected_instance().cloned();
            self.version_picker_mc_filter = inst
                .as_ref()
                .map(|i| i.version_id.clone())
                .unwrap_or_else(|| "all".to_string());
            self.version_picker_loader_filter = if self.mod_loader_filter.is_empty() {
                inst.as_ref()
                    .map(|i| i.mod_loader.name.to_ascii_lowercase())
                    .filter(|l| l != "vanilla")
                    .unwrap_or_else(|| "all".to_string())
            } else {
                self.mod_loader_filter.to_ascii_lowercase()
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
        ctx: &egui::Context,
    ) {
        let target_key = format!("cf:{mod_id}");
        let web_url = self
            .curseforge_results
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

        let inst = self.selected_instance().cloned();
        let gv_opt = inst.as_ref().map(|i| i.version_id.clone());
        self.version_picker_mc_filter = gv_opt.clone().unwrap_or_else(|| "all".to_string());
        self.version_picker_loader_filter = if self.mod_loader_filter.is_empty() {
            inst.as_ref()
                .map(|i| i.mod_loader.name.to_ascii_lowercase())
                .filter(|l| l != "vanilla")
                .unwrap_or_else(|| "all".to_string())
        } else {
            self.mod_loader_filter.to_ascii_lowercase()
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

    pub(crate) fn start_direct_mod_file_install(
        &mut self,
        choice: ModVersionChoice,
        ctx: &egui::Context,
    ) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        let Some(url) = choice.download_url else {
            self.notice_banner = Some((
                format!(
                    "'{}' has third-party distribution disabled by its author on CurseForge.",
                    choice.file_name
                ),
                true,
            ));
            return;
        };
        let (task_id, task_ctrl) = self.begin_task_with_control(
            format!("Mod: {}", choice.file_name),
            format!("Downloading into {}...", inst.name),
        );
        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let mods_dir = self.paths.minecraft_dir(&inst.instance_id).join("mods");
        let dest = mods_dir.join(&choice.file_name);
        let picker_key = self.version_picker_key.clone();
        let picker_title = self.version_picker_title.clone();
        let cf_api_key = self.resolved_cf_api_key();
        let cache_path = self.paths.cache_root.join("providers.sqlite");
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let downloader = DownloadClient::new()
                    .map_err(|e| e.to_string())?
                    .with_task_control(task_ctrl);
                let req = mcw_net::DownloadRequest::new(url, dest)
                    .with_sha1(choice.sha1)
                    .with_size(choice.file_size);
                downloader
                    .download_file(&req)
                    .await
                    .map_err(|e| e.to_string())?;

                if let Some(slug) = picker_key.strip_prefix("mr:") {
                    let _ = ModProvenanceRegistry::record_install(
                        &inst_dir,
                        TrackedModEntry {
                            project_id: slug.to_string(),
                            provider: "modrinth".to_string(),
                            title: picker_title,
                            file_name: choice.file_name.clone(),
                            version_id: choice.id.clone(),
                            version_number: choice.version_number.clone(),
                            locked: false,
                            managed_by: None,
                        },
                    );
                } else if let Some(mid_str) = picker_key.strip_prefix("cf:") {
                    let _ = ModProvenanceRegistry::record_install(
                        &inst_dir,
                        TrackedModEntry {
                            project_id: mid_str.to_string(),
                            provider: "curseforge".to_string(),
                            title: picker_title,
                            file_name: choice.file_name.clone(),
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
                Ok::<String, String>(choice.file_name)
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
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let downloader = DownloadClient::new()
                    .map_err(|e| e.to_string())?
                    .with_task_control(task_ctrl);
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

    pub(crate) fn start_prepare_lan_hosting(&mut self, provider: String, ctx: &egui::Context) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        let (task_id, task_ctrl) = self.begin_task_with_control(
            format!("LAN Hosting ({})", inst.name),
            format!("Configuring {} LAN connection...", provider),
        );
        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let cache_path = self.paths.cache_root.join("providers.sqlite");
        let tx = self.tx.clone();
        let ctx_c = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let mr = ModrinthClient::new(cache).map_err(|e| e.to_string())?;
                let downloader = DownloadClient::new()
                    .map_err(|e| e.to_string())?
                    .with_task_control(task_ctrl);
                LanHostingManager::prepare(
                    &inst_dir,
                    &mc_dir,
                    &inst.version_id,
                    &inst.mod_loader.name,
                    &provider,
                    &mr,
                    &downloader,
                )
                .await
                .map_err(|e| e.to_string())
            }
            .await;
            let _ = tx.send(AppEvent::LanHostingPrepared {
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

    pub(crate) fn start_curseforge_search(&mut self, ctx: &egui::Context) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        self.mod_searching = true;
        let query = self.mod_search_query.clone();
        let api_key = self.resolved_cf_api_key();
        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        let loader_filter = self.mod_loader_filter.clone();
        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let client = CurseForgeClient::new(api_key, cache).map_err(|e| e.to_string())?;
                let loader = if loader_filter.is_empty() || loader_filter == "all" {
                    None
                } else {
                    Some(loader_filter)
                };
                let search = client
                    .search(&CurseForgeSearchParams {
                        query,
                        kind: "mod".to_string(),
                        loader,
                        game_version: Some(inst.version_id.clone()),
                        page_size: 15,
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<Vec<CurseForgeMod>, String>(search.data)
            }
            .await;
            let _ = tx.send(AppEvent::CurseForgeSearchFinished(res));
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_curseforge_modpack_search(&mut self, ctx: &egui::Context) {
        self.pack_browser_searching = true;
        let query = self.pack_browser_query.clone();
        let api_key = self.resolved_cf_api_key();
        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        let loader_opt = if self.pack_loader_filter.is_empty() || self.pack_loader_filter == "all" {
            None
        } else {
            Some(self.pack_loader_filter.clone())
        };
        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let client = CurseForgeClient::new(api_key, cache).map_err(|e| e.to_string())?;
                let search = client
                    .search(&CurseForgeSearchParams {
                        query,
                        kind: "modpack".to_string(),
                        loader: loader_opt,
                        page_size: 15,
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<Vec<CurseForgeMod>, String>(search.data)
            }
            .await;
            let _ = tx.send(AppEvent::CurseForgeModpacksSearchFinished(res));
            ctx.request_repaint();
        });
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
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let client = DownloadClient::new().map_err(|e| e.to_string())?;
                JavaProvisioner::ensure_managed(&runtimes_root, major, &client)
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
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let client = DownloadClient::new().map_err(|e| e.to_string())?;
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
        if self.backup_busy || self.is_instance_busy(&inst.instance_id) {
            return;
        }
        self.backup_busy = true;
        let note = self.backup_note_input.trim().to_string();
        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let id = inst.instance_id.clone();
            let res = tokio::task::spawn_blocking(move || {
                BackupManager::new(&paths)
                    .create_backup(&inst.instance_id, note)
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
                let downloader = DownloadClient::new().map_err(|e| e.to_string())?;
                let cache_path = paths.cache_root.join("providers").join("api_cache.sqlite");
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let cf = CurseForgeClient::from_root(&paths.project_root, cache).ok();
                let opt_name = if name_override.is_empty() {
                    None
                } else {
                    Some(name_override.as_str())
                };
                ModpackInstaller::install_from_archive_with_progress(
                    &paths,
                    &archive,
                    opt_name,
                    &downloader,
                    cf.as_ref(),
                    Some(progress),
                )
                .await
                .map_err(|e| e.to_string())
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

    pub(crate) fn start_modrinth_modpacks_search(&mut self, ctx: &egui::Context) {
        self.pack_browser_searching = true;
        let query = self.pack_browser_query.clone();
        let loader_opt = if self.pack_loader_filter.is_empty() || self.pack_loader_filter == "all" {
            None
        } else {
            Some(self.pack_loader_filter.clone())
        };
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
                let search = mr
                    .search(&ModrinthSearchParams {
                        query,
                        project_type: "modpack".to_string(),
                        loader: loader_opt,
                        game_version: None,
                        limit: 18,
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<Vec<ModrinthSearchHit>, String>(search.hits)
            }
            .await;
            let _ = tx.send(AppEvent::ModrinthModpacksSearchFinished(res));
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

                let downloader = DownloadClient::new().map_err(|e| e.to_string())?;
                cb(0, 1, format!("Downloading {}...", primary.filename));
                downloader
                    .download_file(&mcw_net::DownloadRequest::new(
                        primary.url.clone(),
                        archive_path.clone(),
                    ))
                    .await
                    .map_err(|e| e.to_string())?;

                ModpackInstaller::install_from_archive_with_progress(
                    &paths,
                    &archive_path,
                    Some(&title),
                    &downloader,
                    None,
                    Some(cb),
                )
                .await
                .map_err(|e| e.to_string())
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

    pub(crate) fn start_ftb_search(&mut self, ctx: &egui::Context) {
        self.pack_browser_searching = true;
        let query = self.pack_browser_query.clone();
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
                let ftb = FtbClient::new(cache).map_err(|e| e.to_string())?;
                ftb.search_packs(&query, 15)
                    .await
                    .map_err(|e| e.to_string())
            }
            .await;
            let _ = tx.send(AppEvent::FtbSearchFinished(res));
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_atlauncher_search(&mut self, ctx: &egui::Context) {
        self.pack_browser_searching = true;
        let query = self.pack_browser_query.clone();
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
                let atl = AtLauncherClient::new(cache).map_err(|e| e.to_string())?;
                atl.search(&query, 15).await.map_err(|e| e.to_string())
            }
            .await;
            let _ = tx.send(AppEvent::AtLauncherSearchFinished(res));
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

    pub(crate) fn poll_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                AppEvent::VersionsLoaded(res) => {
                    self.versions_loading = false;
                    if let Ok(list) = res {
                        self.available_versions = list;
                        let latest = self.latest_mc_version(self.create_show_snapshots);
                        if !latest.is_empty()
                            && (self.create_version.is_empty() || !self.show_create_modal)
                        {
                            self.create_version = latest;
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
                                self.modpack_dialog.error = None;
                                self.modpack_dialog.versions = list;
                                self.sync_modpack_dialog_selection();
                            }
                            Err(err) => {
                                self.modpack_dialog.error = Some(err);
                            }
                        }
                    }
                }
                AppEvent::ModrinthModpacksSearchFinished(res) => {
                    self.pack_browser_searching = false;
                    match res {
                        Ok(hits) => self.mr_pack_results = hits,
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("Modrinth modpack search failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ThumbnailLoaded { url, size, rgba } => {
                    self.loading_thumbs.remove(&url);
                    let img = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                    let handle = ctx.load_texture(url.clone(), img, egui::TextureOptions::LINEAR);
                    self.thumb_textures.insert(url, handle);
                }
                AppEvent::ModVersionsLoaded {
                    target_key,
                    versions,
                } => {
                    if self.version_picker_key == target_key {
                        self.version_picker_loading = false;
                        match versions {
                            Ok(list) => {
                                self.version_picker_items = list;
                                self.version_picker_error = None;
                                self.sync_mod_version_picker_selection();
                            }
                            Err(err) => {
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
                                && self.active_tab == DetailTab::Mods
                            {
                                self.load_mods_async(&instance_id, ctx, true);
                            }
                            self.notice_banner = Some((format!("Installed '{file_name}'."), false));
                        }
                        Err(err) => {
                            self.notice_banner = Some((format!("Mod install failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ModUpdatesChecked {
                    instance_id,
                    candidates,
                } => {
                    self.checking_mod_updates = false;
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        let avail = candidates.iter().filter(|c| c.update_available).count();
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
                            self.notice_banner =
                                Some((format!("Updated {count} mod(s) successfully."), false));
                        }
                        Err(err) => {
                            self.notice_banner = Some((format!("Mod update failed: {err}"), true));
                        }
                    }
                }
                AppEvent::LanHostingPrepared {
                    task_id,
                    instance_id,
                    result,
                } => {
                    self.finish_task(task_id);
                    match result {
                        Ok(msg) => {
                            self.mods_cache.remove(&instance_id);
                            self.notice_banner = Some((msg, false));
                        }
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("LAN hosting setup failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ModsLoaded { instance_id, mods } => {
                    self.loading_mods.remove(&instance_id);
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        self.selected_mods = mods.clone();
                    }
                    self.mods_cache.insert(instance_id, mods);
                }
                AppEvent::BackupsLoaded {
                    instance_id,
                    backups,
                } => {
                    self.loading_backups.remove(&instance_id);
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
                        self.selected_backups = backups.clone();
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
                    if self.selected_instance_id.as_deref() == Some(instance_id.as_str())
                        && self.packs_kind == kind
                    {
                        self.selected_content_packs = packs.clone();
                    }
                    self.content_packs_cache.insert((instance_id, kind), packs);
                }
                AppEvent::ContentPackSearchFinished {
                    provider_is_cf,
                    modrinth,
                    curseforge,
                } => {
                    self.packs_searching = false;
                    if provider_is_cf {
                        match curseforge {
                            Ok(mods) => self.packs_cf_results = mods,
                            Err(err) => {
                                self.notice_banner =
                                    Some((format!("CurseForge pack search failed: {err}"), true));
                            }
                        }
                    } else {
                        match modrinth {
                            Ok(hits) => self.packs_mr_results = hits,
                            Err(err) => {
                                self.notice_banner =
                                    Some((format!("Modrinth pack search failed: {err}"), true));
                            }
                        }
                    }
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
                            if self.selected_instance_id.as_deref() == Some(instance_id.as_str())
                                && self.active_tab == DetailTab::Packs
                                && self.packs_kind == kind
                            {
                                self.load_content_packs_async(&instance_id, kind, ctx, true);
                            }
                            self.notice_banner = Some((format!("Installed '{file_name}'."), false));
                        }
                        Err(err) => {
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
                            self.notice_banner = Some((
                                format!(
                                    "Uploaded sanitized log to {} (copied to clipboard).",
                                    res.url
                                ),
                                false,
                            ));
                        }
                        Err(err) => {
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
                    if let Some(task) = self.active_tasks.iter_mut().find(|t| t.id == task_id) {
                        task.message = message;
                        task.current = current;
                        task.total = total;
                    }
                }
                AppEvent::LaunchProgress { instance_id, event } => {
                    if let Some(&tid) = self.launch_task_ids.get(&instance_id) {
                        if let Some(task) = self.active_tasks.iter_mut().find(|t| t.id == tid) {
                            task.message = format!("[{:?}] {}", event.stage, event.message);
                            task.current = event.current;
                            task.total = event.total;
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
                    if let Some(tid) = self.launch_task_ids.remove(&instance_id) {
                        self.finish_task(tid);
                    }
                    self.launch_states
                        .insert(instance_id, LaunchState::Running { pid, control });
                }
                AppEvent::LogReceived { instance_id, line } => {
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
                    if let Some(tid) = self.launch_task_ids.remove(&instance_id) {
                        self.finish_task(tid);
                    }
                    self.launch_states.insert(instance_id, LaunchState::Idle);
                    self.reload_instances();
                    match report {
                        Ok(rep) => match rep.classification {
                            ExitClassification::Normal => {
                                self.notice_banner = Some((
                                    format!("Game closed cleanly ({}s).", rep.duration_secs),
                                    false,
                                ));
                            }
                            ExitClassification::StoppedByUser => {
                                self.notice_banner =
                                    Some(("Game stopped by user.".to_string(), false));
                            }
                            ExitClassification::Crashed => {
                                let diag = LaunchErrorPresenter::present(
                                    &format!("exit code {:?}", rep.exit_code),
                                    rep.unsupported_class_version,
                                );
                                self.notice_banner = Some((diag.banner_text(), true));
                            }
                        },
                        Err(err) => {
                            let diag = LaunchErrorPresenter::present(&err, false);
                            self.notice_banner = Some((diag.banner_text(), true));
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
                            self.reload_accounts();
                            self.notice_banner = Some((
                                format!("Signed in as Microsoft account '{}'.", acc.username),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("Microsoft sign-in failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ModrinthSearchFinished(res) => {
                    self.mod_searching = false;
                    match res {
                        Ok(hits) => self.modrinth_results = hits,
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("Modrinth search failed: {err}"), true));
                        }
                    }
                }
                AppEvent::CurseForgeSearchFinished(res) => {
                    self.mod_searching = false;
                    match res {
                        Ok(mods) => self.curseforge_results = mods,
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("CurseForge search failed: {err}"), true));
                        }
                    }
                }
                AppEvent::CurseForgeModpacksSearchFinished(res) => {
                    self.pack_browser_searching = false;
                    match res {
                        Ok(packs) => self.cf_pack_results = packs,
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("CurseForge modpack search failed: {err}"), true));
                        }
                    }
                }
                AppEvent::CurseForgeModInstallFinished {
                    task_id,
                    mod_id,
                    instance_id,
                    result,
                } => {
                    self.finish_task(task_id);
                    self.installing_cf_mods.remove(&mod_id);
                    match result {
                        Ok(file_name) => {
                            self.mods_cache.remove(&instance_id);
                            if self.selected_instance_id.as_deref() == Some(instance_id.as_str())
                                && self.active_tab == DetailTab::Mods
                            {
                                self.load_mods_async(&instance_id, ctx, true);
                            }
                            self.notice_banner =
                                Some((format!("Installed CurseForge mod '{file_name}'."), false));
                        }
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("CurseForge install failed: {err}"), true));
                        }
                    }
                }
                AppEvent::ModrinthInstallFinished {
                    task_id,
                    slug,
                    instance_id,
                    result,
                } => {
                    self.finish_task(task_id);
                    self.installing_modrinth_slugs.remove(&slug);
                    match result {
                        Ok(count) => {
                            self.mods_cache.remove(&instance_id);
                            if self.selected_instance_id.as_deref() == Some(instance_id.as_str())
                                && self.active_tab == DetailTab::Mods
                            {
                                self.load_mods_async(&instance_id, ctx, true);
                            }
                            self.notice_banner = Some((
                                format!("Installed {count} mod file(s) from Modrinth."),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.notice_banner = Some((format!("Mod install failed: {err}"), true));
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
                    match result {
                        Ok(info) => {
                            self.backup_note_input.clear();
                            self.backups_cache.remove(&instance_id);
                            if self.selected_instance_id.as_deref() == Some(instance_id.as_str())
                                && self.active_tab == DetailTab::Backups
                            {
                                self.load_backups_async(&instance_id, ctx, true);
                            }
                            self.notice_banner = Some((
                                format!(
                                    "Backup '{}' created ({}).",
                                    info.id,
                                    format_bytes(info.size_bytes)
                                ),
                                false,
                            ));
                        }
                        Err(err) => {
                            self.notice_banner = Some((format!("Backup failed: {err}"), true));
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
                            // Keep Create/Modpack install window open so user can keep interacting.
                            // Missing manual mods are flagged on the instance and shown when the user launches it.
                            self.reload_instances();
                            self.notice_banner = Some((
                                format!(
                                    "Installed modpack '{}' (MC {}, {} — {} files, {} overrides{}).",
                                    rep.instance_id,
                                    rep.game_version,
                                    rep.loader.name,
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
                AppEvent::FtbSearchFinished(res) => {
                    self.pack_browser_searching = false;
                    match res {
                        Ok(packs) => self.ftb_results = packs,
                        Err(err) => {
                            self.notice_banner = Some((format!("FTB search failed: {err}"), true));
                        }
                    }
                }
                AppEvent::AtLauncherSearchFinished(res) => {
                    self.pack_browser_searching = false;
                    match res {
                        Ok(packs) => self.atl_results = packs,
                        Err(err) => {
                            self.notice_banner =
                                Some((format!("ATLauncher search failed: {err}"), true));
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
                            self.notice_banner = Some((
                                format!(
                                    "Installed modpack '{}' (MC {}, {} — {} files).",
                                    rep.instance_id,
                                    rep.game_version,
                                    rep.loader.name,
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
            }
        }
    }

    pub(crate) fn sample_network_speed(&mut self) {
        let elapsed = self.last_net_sample_at.elapsed().as_secs_f64();
        if elapsed >= 0.5 {
            let now_bytes = mcw_net::total_downloaded_bytes();
            let delta = now_bytes.saturating_sub(self.last_net_sample_bytes);
            let instant_bps = delta as f64 / elapsed;
            // Smooth exponential moving average so speed readout doesn't jitter wildly
            self.current_download_bps = if self.current_download_bps <= 1.0 {
                instant_bps
            } else {
                self.current_download_bps * 0.4 + instant_bps * 0.6
            };
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
        let mut candidate_name = pack_title.trim().to_string();
        if candidate_name.is_empty() {
            candidate_name = "Modpack".to_string();
        }
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
        };

        // ATLauncher versions are already present in `pack.versions` — populate instantaneously!
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
        let min_ram = d.min_ram_mb.max(256).min(d.max_ram_mb.max(256));
        let max_ram = d.max_ram_mb.max(min_ram);
        let icon_url = d.icon_url.clone();

        let (task_id, task_ctrl) = self.begin_task_with_control(
            format!("Install {}", d.pack_title),
            format!("Preparing {}...", ver.label),
        );
        self.modpack_dialog.open = false;

        let paths = self.paths.clone();
        let cf_key = self.resolved_cf_api_key();
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
                let downloader = DownloadClient::new()
                    .map_err(|e| e.to_string())?
                    .with_task_control(Arc::clone(&task_ctrl));
                let cache = ApiCache::open(&paths.cache_root.join("providers.sqlite"))
                    .ok()
                    .map(Arc::new);

                let report = match provider {
                    ModpackInstallProvider::Modrinth { slug } => {
                        let url = ver
                            .download_url
                            .ok_or_else(|| "Selected Modrinth version has no download URL".to_string())?;
                        let fname = if ver.file_name.is_empty() {
                            format!("{slug}.mrpack")
                        } else {
                            ver.file_name
                        };
                        let archive_path = paths.cache_root.join("modpacks").join(fname);
                        cb(0, 1, "Downloading .mrpack archive...".to_string());
                        let req = mcw_net::DownloadRequest::new(url, &archive_path)
                            .with_size(ver.file_size);
                        downloader
                            .download_file(&req)
                            .await
                            .map_err(|e| e.to_string())?;
                        ModpackInstaller::install_from_archive_with_progress(
                            &paths,
                            &archive_path,
                            Some(&instance_name),
                            &downloader,
                            None,
                            Some(cb.clone()),
                        )
                        .await
                        .map_err(|e| e.to_string())?
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
                        let archive_path = paths.cache_root.join("modpacks").join(fname);
                        cb(0, 1, "Downloading CurseForge modpack archive...".to_string());
                        let req = mcw_net::DownloadRequest::new(dl_url, &archive_path)
                            .with_size(ver.file_size);
                        downloader
                            .download_file(&req)
                            .await
                            .map_err(|e| e.to_string())?;
                        ModpackInstaller::install_from_archive_with_progress(
                            &paths,
                            &archive_path,
                            Some(&instance_name),
                            &downloader,
                            Some(&cf),
                            Some(cb.clone()),
                        )
                        .await
                        .map_err(|e| e.to_string())?
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
                            .install_pack_version_with_progress(
                                &pack,
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

                // Apply configured RAM + Java settings to the newly created instance
                let store = InstanceStore::new(&paths);
                if let Ok(mut settings) = store.load_settings(&report.instance_id) {
                    settings.min_memory = min_ram;
                    settings.max_memory = max_ram;
                    if custom_java.is_some() {
                        settings.java_path = custom_java;
                    }
                    let _ = store.save_settings(&report.instance_id, &settings);
                }

                // Download & persist modpack artwork as `.mcw/instance-icon.<ext>` (matching Python!)
                if let Some(ref url) = icon_url {
                    if let Ok(resp) = downloader.http().get(url).send().await {
                        if resp.status().is_success() {
                            if let Ok(bytes) = resp.bytes().await {
                                let _ = store.set_instance_icon_bytes(&report.instance_id, &bytes);
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
}
