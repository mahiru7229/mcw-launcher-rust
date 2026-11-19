use crate::state::{ActiveTask, AppEvent, ChannelLogSink, ChannelProgressSink};
use crate::theme::{apply_theme, configure_fonts, ThemeMode, ThemePalette};
use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, Frame, Layout, RichText, ScrollArea, Stroke,
    TextEdit, Vec2,
};
use mcw_auth::{
    AccountStore, DeviceCodeResponse, MicrosoftAuthClient, OfflineAuth, StoredAccount,
};
use mcw_minecraft::{RepairService, VanillaLaunchPipeline, VersionManifestService};
use mcw_models::account::AccountSource;
use mcw_models::instance::{Instance, InstanceSettings, ModLoader};
use mcw_models::progress::ProgressEvent;
use mcw_mods::{is_loader_compatible, ModInfo, ModManager};
use mcw_net::DownloadClient;
use mcw_providers::{
    load_curseforge_api_key, ApiCache, AtLauncherClient, AtLauncherPack, CurseForgeClient,
    CurseForgeMod, CurseForgeSearchParams, FtbClient, FtbPackSummary, ManualDownloadItem,
    ModpackInstaller, ModpackProgressCallback, ModrinthClient, ModrinthSearchHit,
    ModrinthSearchParams,
};
use mcw_runtime::{
    ExitClassification, JavaDiscovery, JavaInstallation, JavaProvisioner, LogLine, LogStream,
    ProcessControl, SupervisedProcess,
};
use mcw_storage::{BackupInfo, BackupManager, InstanceRunLock, InstanceStore, LauncherPaths};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_LOG_LINES_PER_INSTANCE: usize = 1500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    Overview,
    Mods,
    Backups,
    Logs,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModProviderTab {
    Modrinth,
    CurseForge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreateInstanceTab {
    Custom,
    ImportArchive,
    CurseForgePacks,
    Ftb,
    AtLauncher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LauncherSettingsTab {
    Appearance,
}

enum LaunchState {
    Idle,
    Preparing(ProgressEvent),
    Running { pid: u32, control: ProcessControl },
}

pub struct McwApp {
    paths: LauncherPaths,
    runtime: Arc<tokio::runtime::Runtime>,
    tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,

    theme_mode: ThemeMode,
    palette: ThemePalette,

    // Core data (lightweight metadata + lazy per-instance caches so switching is instant)
    instances: Vec<Instance>,
    selected_instance_id: Option<String>,
    selected_settings: Option<InstanceSettings>,
    selected_mods: Vec<ModInfo>,
    selected_backups: Vec<BackupInfo>,
    settings_cache: HashMap<String, InstanceSettings>,
    mods_cache: HashMap<String, Vec<ModInfo>>,
    backups_cache: HashMap<String, Vec<BackupInfo>>,
    loading_mods: HashSet<String>,
    loading_backups: HashSet<String>,
    accounts: Vec<StoredAccount>,
    java_installations: Vec<JavaInstallation>,
    available_versions: Vec<String>,
    versions_loading: bool,

    // Active runtime state per instance
    launch_states: HashMap<String, LaunchState>,
    instance_logs: HashMap<String, Vec<LogLine>>,
    logs_errors_only: bool,

    // Concurrent background install tasks (shown live on global bottom progress bar)
    next_task_id: u64,
    active_tasks: Vec<ActiveTask>,
    installing_cf_packs: HashSet<u64>,
    installing_archive_count: usize,
    installing_cf_mods: HashSet<u64>,
    installing_modrinth_slugs: HashSet<String>,
    installing_java_majors: HashSet<u32>,

    // UI state
    active_tab: DetailTab,
    notice_banner: Option<(String, bool)>, // (message, is_error)
    backup_busy: bool,
    backup_note_input: String,
    pending_restore_backup_id: Option<String>,

    // Create instance modal (Custom | Import Archive | CurseForge Packs | FTB | ATLauncher)
    show_create_modal: bool,
    create_tab: CreateInstanceTab,
    create_name: String,
    create_version: String,
    create_loader: String,
    create_error: Option<String>,
    modpack_path_input: String,
    modpack_name_input: String,
    modpack_manual_downloads: Vec<ManualDownloadItem>,
    pack_browser_query: String,
    pack_browser_searching: bool,
    cf_pack_results: Vec<CurseForgeMod>,
    ftb_results: Vec<FtbPackSummary>,
    atl_results: Vec<AtLauncherPack>,

    // Manual Downloads dedicated per-instance windows (CurseForge opt-out mods)
    pending_manual_counts: HashMap<String, usize>,
    open_manual_modals: Vec<String>,
    manual_downloads_cache: HashMap<String, Vec<ManualDownloadItem>>,
    manual_downloads_dir_input: String,
    manual_auto_scan: bool,
    last_manual_scan_at: Option<Instant>,
    manual_scan_status: HashMap<String, String>,

    // Launcher Settings separate window (gear icon in top-right)
    show_launcher_settings: bool,
    launcher_settings_tab: LauncherSettingsTab,

    // Rename & Delete modals
    show_rename_modal: bool,
    rename_input: String,
    show_delete_modal: bool,

    // Account modal
    show_account_modal: bool,
    offline_username_input: String,
    ms_login_in_progress: bool,
    ms_login_mode: Option<&'static str>,
    ms_login_task: Option<tokio::task::JoinHandle<()>>,
    ms_device_code: Option<DeviceCodeResponse>,

    // Mods browser inside Mods tab (Modrinth | CurseForge)
    mod_provider_tab: ModProviderTab,
    mod_search_query: String,
    modrinth_results: Vec<ModrinthSearchHit>,
    curseforge_results: Vec<CurseForgeMod>,
    mod_searching: bool,
}

impl McwApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: LauncherPaths) -> Self {
        configure_fonts(&cc.egui_ctx);
        let theme_mode = ThemeMode::Dark;
        let palette = apply_theme(&cc.egui_ctx, theme_mode);

        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to initialize Tokio runtime"),
        );
        let (tx, rx) = mpsc::channel();

        let default_downloads_dir = default_user_downloads_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        let mut app = Self {
            paths,
            runtime,
            tx,
            rx,
            theme_mode,
            palette,
            instances: Vec::new(),
            selected_instance_id: None,
            selected_settings: None,
            selected_mods: Vec::new(),
            selected_backups: Vec::new(),
            settings_cache: HashMap::new(),
            mods_cache: HashMap::new(),
            backups_cache: HashMap::new(),
            loading_mods: HashSet::new(),
            loading_backups: HashSet::new(),
            accounts: Vec::new(),
            java_installations: Vec::new(),
            available_versions: Vec::new(),
            versions_loading: false,
            launch_states: HashMap::new(),
            instance_logs: HashMap::new(),
            logs_errors_only: false,
            next_task_id: 1,
            active_tasks: Vec::new(),
            installing_cf_packs: HashSet::new(),
            installing_archive_count: 0,
            installing_cf_mods: HashSet::new(),
            installing_modrinth_slugs: HashSet::new(),
            installing_java_majors: HashSet::new(),
            active_tab: DetailTab::Overview,
            notice_banner: None,
            backup_busy: false,
            backup_note_input: String::new(),
            pending_restore_backup_id: None,
            show_create_modal: false,
            create_tab: CreateInstanceTab::Custom,
            create_name: String::new(),
            create_version: "1.21.4".to_string(),
            create_loader: "vanilla".to_string(),
            create_error: None,
            modpack_path_input: String::new(),
            modpack_name_input: String::new(),
            modpack_manual_downloads: Vec::new(),
            pack_browser_query: String::new(),
            pack_browser_searching: false,
            cf_pack_results: Vec::new(),
            ftb_results: Vec::new(),
            atl_results: Vec::new(),
            pending_manual_counts: HashMap::new(),
            open_manual_modals: Vec::new(),
            manual_downloads_cache: HashMap::new(),
            manual_downloads_dir_input: default_downloads_dir,
            manual_auto_scan: true,
            last_manual_scan_at: None,
            manual_scan_status: HashMap::new(),
            show_launcher_settings: false,
            launcher_settings_tab: LauncherSettingsTab::Appearance,
            show_rename_modal: false,
            rename_input: String::new(),
            show_delete_modal: false,
            show_account_modal: false,
            offline_username_input: String::new(),
            ms_login_in_progress: false,
            ms_login_mode: None,
            ms_login_task: None,
            ms_device_code: None,
            mod_provider_tab: ModProviderTab::Modrinth,
            mod_search_query: String::new(),
            modrinth_results: Vec::new(),
            curseforge_results: Vec::new(),
            mod_searching: false,
        };

        app.reload_instances();
        app.reload_accounts();
        app.reload_java();
        app.fetch_versions(&cc.egui_ctx);
        app
    }

    fn reload_instances(&mut self) {
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

    fn refresh_pending_manual_counts(&mut self) {
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

    fn open_manual_downloads_for(&mut self, instance_id: &str) {
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

    fn run_manual_downloads_scan(&mut self, inst_id: &str, manual_trigger: bool) {
        let current_items = self
            .manual_downloads_cache
            .get(inst_id)
            .cloned()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| ModpackInstaller::load_pending_manual_downloads(&self.paths, inst_id));
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
    fn sync_selected_from_cache(&mut self) {
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
    }

    fn ensure_active_tab_loaded(&mut self, ctx: &egui::Context) {
        let Some(id) = self.selected_instance_id.clone() else {
            return;
        };
        match self.active_tab {
            DetailTab::Mods => self.load_mods_async(&id, ctx, false),
            DetailTab::Backups => self.load_backups_async(&id, ctx, false),
            _ => {}
        }
    }

    fn load_mods_async(&mut self, instance_id: &str, ctx: &egui::Context, force: bool) {
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

    fn load_backups_async(&mut self, instance_id: &str, ctx: &egui::Context, force: bool) {
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

    fn reload_accounts(&mut self) {
        let store = AccountStore::new(&self.paths);
        self.accounts = store.list().unwrap_or_default();
    }

    fn reload_java(&mut self) {
        self.java_installations = JavaDiscovery::discover(Some(&self.paths.runtimes_root));
    }

    fn active_account(&self) -> Option<&StoredAccount> {
        self.accounts
            .iter()
            .find(|a| a.active)
            .or_else(|| self.accounts.first())
    }

    fn selected_instance(&self) -> Option<&Instance> {
        let id = self.selected_instance_id.as_ref()?;
        self.instances.iter().find(|i| &i.instance_id == id)
    }

    fn is_instance_busy(&self, instance_id: &str) -> bool {
        !matches!(
            self.launch_states.get(instance_id),
            None | Some(LaunchState::Idle)
        )
    }

    fn begin_task(&mut self, label: impl Into<String>, message: impl Into<String>) -> u64 {
        let id = self.next_task_id;
        self.next_task_id += 1;
        self.active_tasks.push(ActiveTask {
            id,
            label: label.into(),
            message: message.into(),
            current: 0,
            total: 0,
        });
        id
    }

    fn finish_task(&mut self, task_id: u64) {
        self.active_tasks.retain(|t| t.id != task_id);
    }

    fn fetch_versions(&mut self, ctx: &egui::Context) {
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
                let releases: Vec<String> = manifest
                    .versions
                    .into_iter()
                    .filter(|v| v.version_type == "release")
                    .map(|v| v.id)
                    .collect();
                Ok::<Vec<String>, String>(releases)
            }
            .await;
            let _ = tx.send(AppEvent::VersionsLoaded(res));
            ctx.request_repaint();
        });
    }

    fn start_launch(&mut self, ctx: &egui::Context) {
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
                let lock =
                    InstanceRunLock::acquire(&paths, &inst.instance_id).map_err(|e| e.to_string())?;

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

                let client = DownloadClient::new().map_err(|e| e.to_string())?;
                let pipeline = VanillaLaunchPipeline::new(&paths, &client);
                let progress_sink = Arc::new(ChannelProgressSink {
                    instance_id: inst.instance_id.clone(),
                    tx: tx.clone(),
                    ctx: ctx.clone(),
                });

                let prepared = pipeline
                    .prepare(&inst, &settings, &auth, source, progress_sink)
                    .await
                    .map_err(|e| e.to_string())?;

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

    fn cancel_microsoft_login(&mut self) {
        if let Some(handle) = self.ms_login_task.take() {
            handle.abort();
        }
        self.ms_login_in_progress = false;
        self.ms_login_mode = None;
        self.ms_device_code = None;
        self.notice_banner = Some(("Microsoft sign-in cancelled.".to_string(), false));
    }

    fn start_microsoft_login(&mut self, browser_pkce: bool, ctx: &egui::Context) {
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
                    store.store_refresh_token(&uuid, rt).map_err(|e| e.to_string())?;
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

    fn start_modrinth_search(&mut self, ctx: &egui::Context) {
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

        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let client = ModrinthClient::new(cache).map_err(|e| e.to_string())?;
                let loader = if inst.mod_loader.is_vanilla() {
                    None
                } else {
                    Some(inst.mod_loader.name.clone())
                };
                let search = client
                    .search(&ModrinthSearchParams {
                        query,
                        project_type: "mod".to_string(),
                        loader,
                        game_version: Some(inst.version_id.clone()),
                        limit: 12,
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

    fn resolved_cf_api_key(&self) -> String {
        load_curseforge_api_key(Some(&self.paths.project_root)).unwrap_or_default()
    }

    fn start_curseforge_search(&mut self, ctx: &egui::Context) {
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

        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let client = CurseForgeClient::new(api_key, cache).map_err(|e| e.to_string())?;
                let loader = if inst.mod_loader.is_vanilla() {
                    None
                } else {
                    Some(inst.mod_loader.name.clone())
                };
                let search = client
                    .search(&CurseForgeSearchParams {
                        query,
                        kind: "mod".to_string(),
                        loader,
                        game_version: Some(inst.version_id.clone()),
                        page_size: 12,
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

    fn start_curseforge_mod_install(
        &mut self,
        mod_id: u64,
        slug: String,
        ctx: &egui::Context,
    ) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        if !self.installing_cf_mods.insert(mod_id) {
            return;
        }
        let task_id = self.begin_task(
            format!("Mod (CurseForge): {slug}"),
            format!("Downloading into {}...", inst.name),
        );
        let api_key = self.resolved_cf_api_key();
        let mods_dir = self.paths.minecraft_dir(&inst.instance_id).join("mods");
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
                let client = CurseForgeClient::new(api_key, cache).map_err(|e| e.to_string())?;
                let downloader = DownloadClient::new().map_err(|e| e.to_string())?;
                let path = client
                    .install_mod(
                        mod_id,
                        Some(&slug),
                        &inst.version_id,
                        &inst.mod_loader.name,
                        &mods_dir,
                        &downloader,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<String, String>(
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("mod.jar")
                        .to_string(),
                )
            }
            .await;
            let _ = tx.send(AppEvent::CurseForgeModInstallFinished {
                task_id,
                mod_id,
                instance_id: inst.instance_id,
                result: res,
            });
            ctx.request_repaint();
        });
    }

    fn start_curseforge_modpack_search(&mut self, ctx: &egui::Context) {
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

        self.runtime.spawn(async move {
            let res = async {
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let client = CurseForgeClient::new(api_key, cache).map_err(|e| e.to_string())?;
                let search = client
                    .search(&CurseForgeSearchParams {
                        query,
                        kind: "modpack".to_string(),
                        page_size: 12,
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

    fn start_curseforge_modpack_install(
        &mut self,
        mod_id: u64,
        slug: String,
        pack_name: String,
        ctx: &egui::Context,
    ) {
        if !self.installing_cf_packs.insert(mod_id) {
            return;
        }
        self.modpack_manual_downloads.clear();
        let task_id = self.begin_task(
            format!("Modpack: {pack_name}"),
            "Fetching modpack metadata...",
        );
        let api_key = self.resolved_cf_api_key();
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
                let cache_path = paths.cache_root.join("providers").join("api_cache.sqlite");
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let client = CurseForgeClient::new(api_key, cache).map_err(|e| e.to_string())?;
                let downloader = DownloadClient::new().map_err(|e| e.to_string())?;
                client
                    .install_modpack_with_progress(
                        mod_id,
                        Some(&slug),
                        Some(&pack_name),
                        &paths,
                        &downloader,
                        Some(progress),
                    )
                    .await
                    .map_err(|e| e.to_string())
            }
            .await;
            let _ = tx.send(AppEvent::ModpackInstalled {
                task_id,
                cf_pack_id: Some(mod_id),
                result: res,
            });
            ctx.request_repaint();
        });
    }

    fn start_modrinth_install(&mut self, slug: String, ctx: &egui::Context) {
        let Some(inst) = self.selected_instance().cloned() else {
            return;
        };
        if !self.installing_modrinth_slugs.insert(slug.clone()) {
            return;
        }
        let task_id = self.begin_task(
            format!("Mod (Modrinth): {slug}"),
            format!("Resolving & downloading into {}...", inst.name),
        );
        let mods_dir = self.paths.minecraft_dir(&inst.instance_id).join("mods");
        let cache_path = self
            .paths
            .cache_root
            .join("providers")
            .join("api_cache.sqlite");
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                std::fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
                let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
                let mr = ModrinthClient::new(cache).map_err(|e| e.to_string())?;
                let downloader = DownloadClient::new().map_err(|e| e.to_string())?;
                let installed = mr
                    .install_with_dependencies(
                        &slug,
                        &inst.version_id,
                        &inst.mod_loader.name,
                        &mods_dir,
                        &downloader,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<usize, String>(installed.len())
            }
            .await;
            let _ = tx.send(AppEvent::ModrinthInstallFinished {
                task_id,
                slug,
                instance_id: inst.instance_id,
                result: res,
            });
            ctx.request_repaint();
        });
    }

    fn start_java_install(&mut self, major: u32, ctx: &egui::Context) {
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

    fn start_repair(&mut self, ctx: &egui::Context) {
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

    fn start_backup_create(&mut self, ctx: &egui::Context) {
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

    fn start_backup_restore(&mut self, backup_id: String, ctx: &egui::Context) {
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

    fn start_modpack_install(&mut self, ctx: &egui::Context) {
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

    fn start_ftb_search(&mut self, ctx: &egui::Context) {
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
                let ids = ftb.search_ids(&query, 10).await.map_err(|e| e.to_string())?;
                let mut packs = Vec::new();
                for id in ids.into_iter().take(10) {
                    if let Ok(p) = ftb.get_pack(id).await {
                        packs.push(p);
                    }
                }
                Ok::<Vec<FtbPackSummary>, String>(packs)
            }
            .await;
            let _ = tx.send(AppEvent::FtbSearchFinished(res));
            ctx.request_repaint();
        });
    }

    fn start_atlauncher_search(&mut self, ctx: &egui::Context) {
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

    fn poll_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                AppEvent::VersionsLoaded(res) => {
                    self.versions_loading = false;
                    if let Ok(list) = res {
                        if let Some(first) = list.first() {
                            if self.create_version.is_empty() {
                                self.create_version = first.clone();
                            }
                        }
                        self.available_versions = list;
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
                    self.launch_states
                        .insert(instance_id, LaunchState::Preparing(event));
                }
                AppEvent::LaunchStarted {
                    instance_id,
                    pid,
                    control,
                } => {
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
                                let hint = if rep.unsupported_class_version {
                                    " A mod requires a newer Java version — check Settings -> Java."
                                } else {
                                    " Inspect the Logs tab for details."
                                };
                                self.notice_banner = Some((
                                    format!(
                                        "Game crashed (exit code {:?}).{hint}",
                                        rep.exit_code
                                    ),
                                    true,
                                ));
                            }
                        },
                        Err(err) => {
                            self.notice_banner = Some((format!("Launch failed: {err}"), true));
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
                            self.notice_banner = Some((
                                format!("Installed CurseForge mod '{file_name}'."),
                                false,
                            ));
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
                            self.notice_banner =
                                Some((format!("Mod install failed: {err}"), true));
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
            }
        }
    }

    fn render_top_bar(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        egui::TopBottomPanel::top("mcw_top_bar")
            .exact_height(42.0)
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new("MCW Launcher: Rust Edition")
                            .strong()
                            .size(14.0)
                            .color(p.text),
                    );
                    ui.label(
                        RichText::new(format!("v{}", mcw_models::APP_VERSION))
                            .monospace()
                            .size(11.0)
                            .color(p.text_muted),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Gear icon button for Launcher Settings (separate window)
                        if gear_icon_button(ui, p, self.show_launcher_settings)
                            .on_hover_text("Launcher Settings")
                            .clicked()
                        {
                            self.show_launcher_settings = !self.show_launcher_settings;
                        }

                        ui.separator();

                        // Account switcher button
                        let acc_label = match self.active_account() {
                            Some(a) => {
                                let kind = match a.account_type {
                                    AccountSource::Microsoft => "MSA",
                                    AccountSource::Offline => "Offline",
                                };
                                format!("Account: {} ({kind})", a.username)
                            }
                            None => "Sign in / Add Account".to_string(),
                        };
                        if ui.button(acc_label).clicked() {
                            self.show_account_modal = true;
                        }
                    });
                });
            });
    }

    fn render_sidebar(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        egui::SidePanel::left("mcw_instance_sidebar")
            .default_width(250.0)
            .min_width(210.0)
            .max_width(340.0)
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!("INSTANCES ({})", self.instances.len()))
                        .strong()
                        .size(11.5)
                        .color(p.text_muted),
                );
                ui.add_space(4.0);
                let new_btn = Button::new(
                    RichText::new("+ New Instance / Modpack")
                        .color(p.accent_text)
                        .strong(),
                )
                .fill(p.accent)
                .min_size(Vec2::new(ui.available_width(), 28.0))
                .corner_radius(CornerRadius::same(3));
                if ui.add(new_btn).clicked() {
                    self.create_tab = CreateInstanceTab::Custom;
                    self.create_name.clear();
                    self.create_error = None;
                    self.modpack_manual_downloads.clear();
                    if self.create_version.is_empty() {
                        self.create_version = self
                            .available_versions
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "1.21.4".to_string());
                    }
                    self.show_create_modal = true;
                }

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                if self.instances.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(24.0);
                        ui.label(
                            RichText::new("No instances yet")
                                .strong()
                                .color(p.text_muted),
                        );
                        ui.label(
                            RichText::new("Create an instance to get started.")
                                .size(12.0)
                                .color(p.text_muted),
                        );
                    });
                    return;
                }

                let mut newly_selected: Option<String> = None;
                ScrollArea::vertical().show(ui, |ui| {
                    for inst in &self.instances {
                        let is_selected =
                            self.selected_instance_id.as_deref() == Some(inst.instance_id.as_str());
                        let missing_manual = self
                            .pending_manual_counts
                            .get(&inst.instance_id)
                            .copied()
                            .unwrap_or(0);
                        let (status_text, status_color, is_running) =
                            match self.launch_states.get(&inst.instance_id) {
                                Some(LaunchState::Running { .. }) => {
                                    ("Running".to_string(), p.accent, true)
                                }
                                Some(LaunchState::Preparing(_)) => {
                                    ("Preparing".to_string(), p.warning, true)
                                }
                                _ if missing_manual > 0 => (
                                    format!("Incomplete ({missing_manual})"),
                                    p.warning,
                                    false,
                                ),
                                _ => match inst.last_launch_state.as_str() {
                                    "crashed" => ("Crashed".to_string(), p.danger, false),
                                    "finished" => ("Finished".to_string(), p.text_muted, false),
                                    _ => ("Ready".to_string(), p.text_muted, false),
                                },
                            };

                        let bg = if is_selected { p.elevated } else { p.surface };
                        let border_col = if is_selected {
                            p.accent
                        } else if missing_manual > 0 {
                            p.warning
                        } else {
                            p.border
                        };

                        let resp = Frame::new()
                            .fill(bg)
                            .stroke(Stroke::new(1.0_f32, border_col))
                            .corner_radius(CornerRadius::same(3))
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    // Signature Moment #1: Left vertical accent bar (Emerald or Warning if incomplete)
                                    if is_selected || is_running || missing_manual > 0 {
                                        let bar_col = if is_running {
                                            p.accent
                                        } else if missing_manual > 0 && !is_selected {
                                            p.warning
                                        } else {
                                            p.accent
                                        };
                                        let (rect, _) = ui.allocate_exact_size(
                                            Vec2::new(3.0, 32.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(
                                            rect,
                                            CornerRadius::same(1),
                                            bar_col,
                                        );
                                    }
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(&inst.name)
                                                .strong()
                                                .size(13.5)
                                                .color(p.text),
                                        );
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} · {}",
                                                    inst.version_id, inst.mod_loader.name
                                                ))
                                                .monospace()
                                                .size(11.0)
                                                .color(p.text_muted),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(status_text)
                                                            .size(11.0)
                                                            .color(status_color),
                                                    );
                                                },
                                            );
                                        });
                                    });
                                });
                            })
                            .response
                            .interact(egui::Sense::click());

                        if resp.clicked() {
                            newly_selected = Some(inst.instance_id.clone());
                        }
                        ui.add_space(4.0);
                    }
                });

                if let Some(id) = newly_selected {
                    self.selected_instance_id = Some(id);
                    self.sync_selected_from_cache();
                    self.ensure_active_tab_loaded(ctx);
                }
            });
    }

    fn render_detail_pane(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(p.canvas)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                // Notice / Error banner
                if let Some((msg, is_err)) = self.notice_banner.clone() {
                    let banner_stroke = if is_err { p.danger } else { p.accent };
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, banner_stroke))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(msg).color(if is_err {
                                    p.danger
                                } else {
                                    p.text
                                }));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button("Dismiss").clicked() {
                                        self.notice_banner = None;
                                    }
                                });
                            });
                        });
                    ui.add_space(8.0);
                }

                let Some(inst) = self.selected_instance().cloned() else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.label(
                            RichText::new("Select or create a Minecraft instance")
                                .size(16.0)
                                .strong(),
                        );
                        ui.add_space(8.0);
                        if ui.button("+ Create Instance").clicked() {
                            self.show_create_modal = true;
                        }
                    });
                    return;
                };

                // Detail Header + Primary Launch/Stop button
                let busy = self.is_instance_busy(&inst.instance_id);
                let has_account = self.active_account().is_some();

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&inst.name).size(20.0).strong().color(p.text));
                        ui.label(
                            RichText::new(format!(
                                "Minecraft {}  |  Loader: {}  |  Playtime: {}",
                                inst.version_id,
                                inst.mod_loader.name,
                                format_duration(inst.total_playtime_seconds)
                            ))
                            .monospace()
                            .size(11.5)
                            .color(p.text_muted),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        match self.launch_states.get(&inst.instance_id) {
                            Some(LaunchState::Running { control, .. }) => {
                                let stop_btn = Button::new(
                                    RichText::new("Stop Game").color(Color32::WHITE).strong(),
                                )
                                .fill(p.danger)
                                .min_size(Vec2::new(120.0, 34.0))
                                .corner_radius(CornerRadius::same(3));
                                if ui.add(stop_btn).clicked() {
                                    control.request_stop();
                                }
                            }
                            Some(LaunchState::Preparing(_)) => {
                                let prep_btn = Button::new(
                                    RichText::new("Preparing...").color(p.text_muted),
                                )
                                .min_size(Vec2::new(120.0, 34.0))
                                .corner_radius(CornerRadius::same(3));
                                ui.add_enabled(false, prep_btn);
                            }
                            _ => {
                                let missing_manual = self
                                    .pending_manual_counts
                                    .get(&inst.instance_id)
                                    .copied()
                                    .unwrap_or(0);
                                if missing_manual > 0 {
                                    let warn_btn = Button::new(
                                        RichText::new(format!("Missing Mods ({missing_manual})"))
                                            .color(Color32::BLACK)
                                            .strong(),
                                    )
                                    .fill(p.warning)
                                    .min_size(Vec2::new(140.0, 34.0))
                                    .corner_radius(CornerRadius::same(3));
                                    if ui
                                        .add(warn_btn)
                                        .on_hover_text(
                                            "This modpack still has missing manual downloads. Click to complete them before launching.",
                                        )
                                        .clicked()
                                    {
                                        self.open_manual_downloads_for(&inst.instance_id);
                                    }
                                } else {
                                    let launch_btn = Button::new(
                                        RichText::new("Launch").color(p.accent_text).strong(),
                                    )
                                    .fill(p.accent)
                                    .min_size(Vec2::new(120.0, 34.0))
                                    .corner_radius(CornerRadius::same(3));
                                    let resp = ui.add_enabled(has_account, launch_btn);
                                    if !has_account {
                                        resp.on_disabled_hover_text(
                                            "Add or sign in to an account first",
                                        );
                                    } else if resp.clicked() {
                                        self.start_launch(ctx);
                                    }
                                }
                            }
                        }
                    });
                });

                ui.add_space(8.0);

                // Persistent flag banner whenever a modpack instance still has missing manual downloads
                let pending_manual_count = self
                    .pending_manual_counts
                    .get(&inst.instance_id)
                    .copied()
                    .unwrap_or(0);
                if pending_manual_count > 0 {
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.warning))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("INCOMPLETE MODPACK")
                                        .strong()
                                        .monospace()
                                        .size(11.0)
                                        .color(p.warning),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{pending_manual_count} manual mod(s) have not been downloaded yet."
                                    ))
                                    .size(12.0)
                                    .color(p.text),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let open_btn = Button::new(
                                        RichText::new("Open Manual Downloads")
                                            .color(Color32::BLACK)
                                            .strong(),
                                    )
                                    .fill(p.warning);
                                    if ui.add(open_btn).clicked() {
                                        self.open_manual_downloads_for(&inst.instance_id);
                                    }
                                    if ui.button("Scan Downloads Now").clicked() {
                                        self.run_manual_downloads_scan(&inst.instance_id, true);
                                    }
                                });
                            });
                        });
                    ui.add_space(8.0);
                }

                // Signature Moment #2: Truthful Launch Status Strip
                match self.launch_states.get(&inst.instance_id) {
                    Some(LaunchState::Preparing(ev)) => {
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, p.accent))
                            .corner_radius(CornerRadius::same(3))
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("[{:?}]", ev.stage))
                                            .monospace()
                                            .color(p.accent),
                                    );
                                    ui.label(&ev.message);
                                });
                                if ev.total > 0 {
                                    let frac = (ev.current as f32 / ev.total as f32).clamp(0.0, 1.0);
                                    ui.add(
                                        egui::ProgressBar::new(frac)
                                            .fill(p.accent)
                                            .show_percentage(),
                                    );
                                }
                            });
                        ui.add_space(8.0);
                    }
                    Some(LaunchState::Running { pid, .. }) => {
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, p.accent))
                            .corner_radius(CornerRadius::same(3))
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("RUNNING")
                                            .strong()
                                            .monospace()
                                            .color(p.accent),
                                    );
                                    ui.label(
                                        RichText::new(format!("PID {pid} — streaming live logs"))
                                            .color(p.text_muted),
                                    );
                                });
                            });
                        ui.add_space(8.0);
                    }
                    _ => {}
                }

                // Detail Tabs
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (DetailTab::Overview, "Overview"),
                        (DetailTab::Mods, "Mods"),
                        (DetailTab::Backups, "Backups"),
                        (DetailTab::Logs, "Logs"),
                        (DetailTab::Settings, "Settings"),
                    ] {
                        let active = self.active_tab == tab;
                        let btn = Button::new(
                            RichText::new(label).color(if active {
                                p.accent
                            } else {
                                p.text_muted
                            }),
                        )
                        .fill(if active { p.elevated } else { p.surface })
                        .stroke(Stroke::new(
                            1.0_f32,
                            if active { p.accent } else { p.border },
                        ))
                        .corner_radius(CornerRadius::same(3));
                        if ui.add(btn).clicked() {
                            self.active_tab = tab;
                            self.ensure_active_tab_loaded(ctx);
                        }
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                match self.active_tab {
                    DetailTab::Overview => self.render_overview_tab(ui, ctx, &inst, busy),
                    DetailTab::Mods => self.render_mods_tab(ui, ctx, &inst),
                    DetailTab::Backups => self.render_backups_tab(ui, ctx, &inst, busy),
                    DetailTab::Logs => self.render_logs_tab(ui, &inst),
                    DetailTab::Settings => self.render_settings_tab(ui, ctx, &inst),
                }
            });
    }

    fn render_overview_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
        busy: bool,
    ) {
        let p = self.palette;
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let mut trigger_repair = false;

        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(3))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Instance Details").strong().color(p.text));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Game Directory:").color(p.text_muted));
                    ui.label(RichText::new(mc_dir.display().to_string()).monospace());
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Last Status:").color(p.text_muted));
                    ui.label(format!(
                        "{} (exit code: {:?})",
                        inst.last_launch_state, inst.last_exit_code
                    ));
                });
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui.button("Open .minecraft Folder").clicked() {
                        let _ = std::fs::create_dir_all(&mc_dir);
                        open_external(&mc_dir.display().to_string());
                    }

                    let repair_resp = ui.add_enabled(!busy, Button::new("Repair Files"));
                    if busy {
                        repair_resp.on_disabled_hover_text("Stop the instance before repairing");
                    } else if repair_resp.clicked() {
                        trigger_repair = true;
                    }

                    let rename_resp = ui.add_enabled(!busy, Button::new("Rename Instance"));
                    if busy {
                        rename_resp.on_disabled_hover_text("Stop the instance before renaming");
                    } else if rename_resp.clicked() {
                        self.rename_input = inst.name.clone();
                        self.show_rename_modal = true;
                    }

                    let del_btn = Button::new(RichText::new("Delete Instance").color(p.danger))
                        .stroke(Stroke::new(1.0_f32, p.danger));
                    let del_resp = ui.add_enabled(!busy, del_btn);
                    if busy {
                        del_resp.on_disabled_hover_text("Stop the instance before deleting");
                    } else if del_resp.clicked() {
                        self.show_delete_modal = true;
                    }
                });
            });

        if trigger_repair {
            self.start_repair(ctx);
        }
    }

    fn render_backups_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
        busy: bool,
    ) {
        let p = self.palette;
        let disabled = busy || self.backup_busy;

        ui.horizontal(|ui| {
            ui.label(RichText::new("Backup Note:").strong());
            ui.add(
                TextEdit::singleline(&mut self.backup_note_input)
                    .hint_text("Optional note (e.g. before installing mods)...")
                    .desired_width(260.0),
            );
            let create_btn = Button::new(
                RichText::new(if self.backup_busy {
                    "Working..."
                } else {
                    "Create Backup"
                })
                .color(p.accent_text)
                .strong(),
            )
            .fill(p.accent);
            let resp = ui.add_enabled(!disabled, create_btn);
            if busy {
                resp.on_disabled_hover_text("Stop the instance before creating a backup");
            } else if resp.clicked() {
                self.start_backup_create(ctx);
            }
            if ui.button("Refresh").clicked() {
                self.load_backups_async(&inst.instance_id, ctx, true);
            }
        });

        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("Saved Backups ({})", self.selected_backups.len()))
                .strong()
                .color(p.text),
        );
        ui.add_space(4.0);

        if self.selected_backups.is_empty() {
            let msg = if self.loading_backups.contains(&inst.instance_id) {
                "Loading backups..."
            } else {
                "No backups created for this instance yet."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        let mut delete_id: Option<String> = None;
        ScrollArea::vertical()
            .id_salt("backups_list_scroll")
            .show(ui, |ui| {
                for b in &self.selected_backups {
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&b.id).monospace().strong());
                                    let note_str = if b.note.is_empty() {
                                        "No note".to_string()
                                    } else {
                                        b.note.clone()
                                    };
                                    ui.label(
                                        RichText::new(format!(
                                            "{}  ·  {}",
                                            format_bytes(b.size_bytes),
                                            note_str
                                        ))
                                        .size(11.5)
                                        .color(p.text_muted),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .add_enabled(
                                            !disabled,
                                            Button::new(RichText::new("Delete").color(p.danger)),
                                        )
                                        .clicked()
                                    {
                                        delete_id = Some(b.id.clone());
                                    }
                                    if ui
                                        .add_enabled(!disabled, Button::new("Restore"))
                                        .clicked()
                                    {
                                        self.pending_restore_backup_id = Some(b.id.clone());
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });

        if let Some(id) = delete_id {
            match BackupManager::new(&self.paths).delete_backup(&inst.instance_id, &id) {
                Ok(()) => {
                    self.backups_cache.remove(&inst.instance_id);
                    self.load_backups_async(&inst.instance_id, ctx, true);
                    self.notice_banner = Some((format!("Deleted backup '{id}'."), false));
                }
                Err(e) => {
                    self.notice_banner = Some((format!("Failed to delete backup: {e}"), true));
                }
            }
        }
    }

    fn render_mods_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, inst: &Instance) {
        let p = self.palette;
        let mods_dir = self.paths.minecraft_dir(&inst.instance_id).join("mods");

        // Mod Provider Search Bar (Modrinth | CurseForge)
        ui.horizontal(|ui| {
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
            let hint = match self.mod_provider_tab {
                ModProviderTab::Modrinth => "Search Modrinth mods (e.g. sodium, fabric-api)...",
                ModProviderTab::CurseForge => "Search CurseForge mods (requires CURSEFORGE_API_KEY)...",
            };
            let te = ui.add(
                TextEdit::singleline(&mut self.mod_search_query)
                    .hint_text(hint)
                    .desired_width(270.0),
            );
            let search_clicked = ui
                .add_enabled(!self.mod_searching, Button::new("Search"))
                .clicked();
            if search_clicked || (te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                match self.mod_provider_tab {
                    ModProviderTab::Modrinth => self.start_modrinth_search(ctx),
                    ModProviderTab::CurseForge => self.start_curseforge_search(ctx),
                }
            }
            if ui.button("Refresh Local").clicked() {
                self.load_mods_async(&inst.instance_id, ctx, true);
            }
        });

        if self.mod_provider_tab == ModProviderTab::CurseForge && !self.curseforge_results.is_empty()
        {
            ui.add_space(6.0);
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(3))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("CurseForge Results")
                                .strong()
                                .size(12.0)
                                .color(p.text_muted),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Close results").clicked() {
                                self.curseforge_results.clear();
                            }
                        });
                    });
                    ScrollArea::vertical()
                        .id_salt("curseforge_results_scroll")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            let mut cf_install: Option<(u64, String)> = None;
                            for cf_mod in &self.curseforge_results {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&cf_mod.name).strong());
                                    ui.label(
                                        RichText::new(format!(
                                            "#{} · {} downloads",
                                            cf_mod.id, cf_mod.download_count
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(p.text_muted),
                                    );
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            let is_this_installing =
                                                self.installing_cf_mods.contains(&cf_mod.id);
                                            let can_install = !is_this_installing
                                                && !inst.mod_loader.is_vanilla();
                                            let btn_label = if is_this_installing {
                                                "Installing..."
                                            } else {
                                                "Install"
                                            };
                                            let resp = ui.add_enabled(
                                                can_install,
                                                Button::new(btn_label),
                                            );
                                            if inst.mod_loader.is_vanilla() {
                                                resp.on_disabled_hover_text(
                                                    "Vanilla instances cannot install mods",
                                                );
                                            } else if resp.clicked() {
                                                cf_install =
                                                    Some((cf_mod.id, cf_mod.slug.clone()));
                                            }
                                            if let Some(url) = cf_mod
                                                .links
                                                .as_ref()
                                                .and_then(|l| l.website_url.as_ref())
                                            {
                                                if ui.button("Web").clicked() {
                                                    open_external(url);
                                                }
                                            }
                                        },
                                    );
                                });
                            }
                            if let Some((mid, slug)) = cf_install {
                                self.start_curseforge_mod_install(mid, slug, ctx);
                            }
                        });
                });
        }

        if self.mod_provider_tab == ModProviderTab::Modrinth && !self.modrinth_results.is_empty() {
            ui.add_space(6.0);
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border))
                .corner_radius(CornerRadius::same(3))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Modrinth Results")
                                .strong()
                                .size(12.0)
                                .color(p.text_muted),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Close results").clicked() {
                                self.modrinth_results.clear();
                            }
                        });
                    });
                    ScrollArea::vertical()
                        .id_salt("modrinth_results_scroll")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            let mut to_install: Option<String> = None;
                            for hit in &self.modrinth_results {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&hit.title).strong());
                                    ui.label(
                                        RichText::new(format!("({})", hit.slug))
                                            .monospace()
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let is_this_installing =
                                            self.installing_modrinth_slugs.contains(&hit.slug);
                                        let can_install =
                                            !is_this_installing && !inst.mod_loader.is_vanilla();
                                        let btn_label = if is_this_installing {
                                            "Installing..."
                                        } else {
                                            "Install"
                                        };
                                        let resp = ui.add_enabled(
                                            can_install,
                                            Button::new(btn_label),
                                        );
                                        if inst.mod_loader.is_vanilla() {
                                            resp.on_disabled_hover_text(
                                                "Vanilla instances cannot install Fabric/Quilt mods",
                                            );
                                        } else if resp.clicked() {
                                            to_install = Some(hit.slug.clone());
                                        }
                                    });
                                });
                            }
                            if let Some(slug) = to_install {
                                self.start_modrinth_install(slug, ctx);
                            }
                        });
                });
        }

        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("Installed Mods ({})", self.selected_mods.len()))
                .strong()
                .color(p.text),
        );
        ui.add_space(4.0);

        if self.selected_mods.is_empty() {
            let msg = if self.loading_mods.contains(&inst.instance_id) {
                "Scanning mods/ in background..."
            } else {
                "No mods installed in this instance's mods/ folder."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        let mut toggle_action: Option<(String, bool)> = None;
        let mut delete_action: Option<String> = None;

        ScrollArea::vertical()
            .id_salt("local_mods_scroll")
            .show(ui, |ui| {
                for m in &self.selected_mods {
                    let compat = is_loader_compatible(m.loader, &inst.mod_loader.name);
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let mut enabled = m.enabled;
                                if ui.checkbox(&mut enabled, "").changed() {
                                    toggle_action = Some((m.file_name.clone(), enabled));
                                }
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&m.name).strong());
                                        ui.label(
                                            RichText::new(&m.version)
                                                .monospace()
                                                .size(11.0)
                                                .color(p.text_muted),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "{:?} · Java {}+",
                                                m.loader, m.required_java_major
                                            ))
                                            .size(11.0)
                                            .color(p.text_muted),
                                        );
                                        if !compat {
                                            ui.label(
                                                RichText::new("INCOMPATIBLE")
                                                    .size(11.0)
                                                    .color(p.warning),
                                            );
                                        }
                                    });
                                    ui.label(
                                        RichText::new(&m.file_name)
                                            .monospace()
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .button(RichText::new("Remove").color(p.danger))
                                        .clicked()
                                    {
                                        delete_action = Some(m.file_name.clone());
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });

        if let Some((file_name, enable)) = toggle_action {
            if let Err(e) = ModManager::new(&mods_dir).set_enabled(&file_name, enable) {
                self.notice_banner = Some((format!("Failed to toggle mod: {e}"), true));
            }
            self.mods_cache.remove(&inst.instance_id);
            self.load_mods_async(&inst.instance_id, ctx, true);
        }
        if let Some(file_name) = delete_action {
            if let Err(e) = ModManager::new(&mods_dir).delete(&file_name) {
                self.notice_banner = Some((format!("Failed to delete mod: {e}"), true));
            }
            self.mods_cache.remove(&inst.instance_id);
            self.load_mods_async(&inst.instance_id, ctx, true);
        }
    }

    fn render_logs_tab(&mut self, ui: &mut egui::Ui, inst: &Instance) {
        let p = self.palette;
        let log_file = self
            .paths
            .logs_root
            .join(format!("{}-latest.log", inst.instance_id));

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.logs_errors_only, "Stderr / Errors only");
            if ui.button("Copy Logs").clicked() {
                if let Some(lines) = self.instance_logs.get(&inst.instance_id) {
                    let text: String = lines
                        .iter()
                        .map(|l| l.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.ctx().copy_text(text);
                }
            }
            if ui.button("Clear").clicked() {
                self.instance_logs.insert(inst.instance_id.clone(), Vec::new());
            }
            if log_file.is_file() && ui.button("Open Log File").clicked() {
                open_external(&log_file.display().to_string());
            }
        });

        ui.add_space(6.0);

        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(3))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let lines = self.instance_logs.get(&inst.instance_id);
                if lines.map(|l| l.is_empty()).unwrap_or(true) {
                    ui.label(
                        RichText::new("No live logs captured for this session yet.")
                            .monospace()
                            .color(p.text_muted),
                    );
                    return;
                }
                ScrollArea::vertical()
                    .id_salt("live_logs_scroll")
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in lines.unwrap() {
                            let is_err = line.stream == LogStream::Stderr
                                || line.text.contains("ERROR")
                                || line.text.contains("Exception");
                            if self.logs_errors_only && !is_err {
                                continue;
                            }
                            let color = if is_err { p.danger } else { p.text };
                            ui.label(RichText::new(&line.text).monospace().size(11.5).color(color));
                        }
                    });
            });
    }

    fn render_settings_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, inst: &Instance) {
        let p = self.palette;
        let Some(ref mut settings) = self.selected_settings else {
            return;
        };

        let mut save_clicked = false;
        let mut install_java_major: Option<u32> = None;

        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(3))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Memory & Window").strong());
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("Min RAM (MB):");
                    ui.add(egui::DragValue::new(&mut settings.min_memory).range(256..=32768));
                    ui.label("Max RAM (MB):");
                    ui.add(egui::DragValue::new(&mut settings.max_memory).range(512..=65536));
                });

                ui.horizontal(|ui| {
                    ui.label("Width:");
                    ui.add(egui::DragValue::new(&mut settings.width).range(320..=7680));
                    ui.label("Height:");
                    ui.add(egui::DragValue::new(&mut settings.height).range(240..=4320));
                    ui.checkbox(&mut settings.fullscreen, "Fullscreen");
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                ui.label(RichText::new("Java Runtime").strong());
                ui.add_space(6.0);

                let current_java_label = settings
                    .java_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "Automatic (recommended)".to_string());

                egui::ComboBox::from_label("Java Executable")
                    .selected_text(current_java_label)
                    .width(420.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut settings.java_path, None, "Automatic (recommended)");
                        for j in &self.java_installations {
                            let label = format!(
                                "Java {} ({}) — {}",
                                j.major,
                                j.version_string,
                                j.path.display()
                            );
                            ui.selectable_value(
                                &mut settings.java_path,
                                Some(j.path.clone()),
                                label,
                            );
                        }
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let installing_21 = self.installing_java_majors.contains(&21);
                    if ui
                        .add_enabled(
                            !installing_21,
                            Button::new(if installing_21 {
                                "Installing Java 21..."
                            } else {
                                "Install Temurin Java 21"
                            }),
                        )
                        .clicked()
                    {
                        install_java_major = Some(21);
                    }
                    let installing_17 = self.installing_java_majors.contains(&17);
                    if ui
                        .add_enabled(
                            !installing_17,
                            Button::new(if installing_17 {
                                "Installing Java 17..."
                            } else {
                                "Install Temurin Java 17"
                            }),
                        )
                        .clicked()
                    {
                        install_java_major = Some(17);
                    }
                });

                ui.add_space(12.0);
                let save_btn = Button::new(RichText::new("Save Settings").color(p.accent_text).strong())
                    .fill(p.accent)
                    .corner_radius(CornerRadius::same(3));
                if ui.add(save_btn).clicked() {
                    save_clicked = true;
                }
            });

        if save_clicked {
            if let Some(ref s) = self.selected_settings {
                let store = InstanceStore::new(&self.paths);
                match store.save_settings(&inst.instance_id, s) {
                    Ok(()) => {
                        self.settings_cache
                            .insert(inst.instance_id.clone(), s.clone());
                        self.notice_banner = Some(("Settings saved.".to_string(), false));
                    }
                    Err(e) => {
                        self.notice_banner = Some((format!("Failed to save settings: {e}"), true));
                    }
                }
            }
        }
        if let Some(major) = install_java_major {
            self.start_java_install(major, ctx);
        }
    }

    fn render_modals(&mut self, ctx: &egui::Context) {
        let p = self.palette;

        // 1. Unified Create Instance / Modpack Modal (Custom | Import Archive | FTB | ATLauncher)
        if self.show_create_modal {
            egui::Window::new("Create Instance")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(460.0);
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.create_tab,
                            CreateInstanceTab::Custom,
                            "Custom",
                        );
                        ui.selectable_value(
                            &mut self.create_tab,
                            CreateInstanceTab::ImportArchive,
                            "Import Archive",
                        );
                        ui.selectable_value(
                            &mut self.create_tab,
                            CreateInstanceTab::CurseForgePacks,
                            "CurseForge Packs",
                        );
                        ui.selectable_value(
                            &mut self.create_tab,
                            CreateInstanceTab::Ftb,
                            "FTB Modpacks",
                        );
                        ui.selectable_value(
                            &mut self.create_tab,
                            CreateInstanceTab::AtLauncher,
                            "ATLauncher",
                        );
                    });
                    ui.separator();
                    ui.add_space(6.0);

                    match self.create_tab {
                        CreateInstanceTab::Custom => {
                            ui.label("Instance Name:");
                            ui.text_edit_singleline(&mut self.create_name);
                            ui.add_space(6.0);

                            ui.label("Minecraft Version:");
                            egui::ComboBox::from_id_salt("create_mc_version")
                                .selected_text(&self.create_version)
                                .width(240.0)
                                .show_ui(ui, |ui| {
                                    for v in self.available_versions.iter().take(40) {
                                        ui.selectable_value(&mut self.create_version, v.clone(), v);
                                    }
                                });
                            ui.add_space(6.0);

                            ui.label("Mod Loader:");
                            ui.horizontal(|ui| {
                                for loader in ["vanilla", "fabric", "quilt", "forge", "neoforge"] {
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

                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                let create_btn = Button::new(
                                    RichText::new("Create Instance")
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
                                        Ok((created, _)) => {
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
                                if ui.button("Cancel").clicked() {
                                    self.show_create_modal = false;
                                }
                            });
                        }
                        CreateInstanceTab::ImportArchive => {
                            ui.label("Modpack Archive Path (.mrpack or CurseForge .zip):");
                            ui.add(
                                TextEdit::singleline(&mut self.modpack_path_input)
                                    .hint_text(r"C:\Users\...\Downloads\modpack.mrpack")
                                    .desired_width(430.0),
                            );
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
                                        ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
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
                                        "Import Modpack"
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
                            ui.horizontal(|ui| {
                                let te = ui.add(
                                    TextEdit::singleline(&mut self.pack_browser_query)
                                        .hint_text("Search CurseForge modpacks (e.g. RLcraft, ATM10)...")
                                        .desired_width(330.0),
                                );
                                let go = ui
                                    .add_enabled(!self.pack_browser_searching, Button::new("Search"))
                                    .clicked();
                                if go
                                    || (te.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    self.start_curseforge_modpack_search(ctx);
                                }
                            });
                            ui.add_space(6.0);
                            let mut install_cf_pack: Option<(u64, String, String)> = None;
                            ScrollArea::vertical()
                                .id_salt("create_cf_packs_scroll")
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    if self.cf_pack_results.is_empty() {
                                        ui.label(
                                            RichText::new("Search CurseForge modpacks to browse and install.")
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
                                                    ui.set_width(ui.available_width());
                                                    ui.horizontal(|ui| {
                                                        ui.label(RichText::new(&pack.name).strong());
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "#{} · {} downloads",
                                                                pack.id, pack.download_count
                                                            ))
                                                            .monospace()
                                                            .size(11.0)
                                                            .color(p.text_muted),
                                                        );
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
                                                                    ));
                                                                }
                                                            },
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
                                            ui.add_space(4.0);
                                        }
                                    }
                                });
                            if let Some((mid, slug, name)) = install_cf_pack {
                                self.start_curseforge_modpack_install(mid, slug, name, ctx);
                            }
                            ui.add_space(8.0);
                            if ui.button("Close").clicked() {
                                self.show_create_modal = false;
                            }
                        }
                        CreateInstanceTab::Ftb => {
                            ui.horizontal(|ui| {
                                let te = ui.add(
                                    TextEdit::singleline(&mut self.pack_browser_query)
                                        .hint_text("Search FTB modpacks (e.g. stoneblock, skies)...")
                                        .desired_width(330.0),
                                );
                                let go = ui
                                    .add_enabled(!self.pack_browser_searching, Button::new("Search"))
                                    .clicked();
                                if go
                                    || (te.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    self.start_ftb_search(ctx);
                                }
                            });
                            ui.add_space(6.0);
                            ScrollArea::vertical()
                                .id_salt("create_ftb_scroll")
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    if self.ftb_results.is_empty() {
                                        ui.label(
                                            RichText::new("Enter a search term to browse FTB modpacks.")
                                                .color(p.text_muted),
                                        );
                                    } else {
                                        for pack in &self.ftb_results {
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
                            ui.add_space(8.0);
                            if ui.button("Close").clicked() {
                                self.show_create_modal = false;
                            }
                        }
                        CreateInstanceTab::AtLauncher => {
                            ui.horizontal(|ui| {
                                let te = ui.add(
                                    TextEdit::singleline(&mut self.pack_browser_query)
                                        .hint_text("Search ATLauncher packs (or leave empty to list)...")
                                        .desired_width(330.0),
                                );
                                let go = ui
                                    .add_enabled(!self.pack_browser_searching, Button::new("Search"))
                                    .clicked();
                                if go
                                    || (te.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    self.start_atlauncher_search(ctx);
                                }
                            });
                            ui.add_space(6.0);
                            ScrollArea::vertical()
                                .id_salt("create_atl_scroll")
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    if self.atl_results.is_empty() {
                                        ui.label(
                                            RichText::new("Click Search to browse ATLauncher modpacks.")
                                                .color(p.text_muted),
                                        );
                                    } else {
                                        for pack in &self.atl_results {
                                            Frame::new()
                                                .fill(p.elevated)
                                                .stroke(Stroke::new(1.0_f32, p.border))
                                                .corner_radius(CornerRadius::same(3))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(RichText::new(&pack.name).strong());
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
                            ui.add_space(8.0);
                            if ui.button("Close").clicked() {
                                self.show_create_modal = false;
                            }
                        }
                    }
                });
        }

        // 2. Rename Instance Modal
        if self.show_rename_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                egui::Window::new("Rename Instance")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                    .show(ctx, |ui| {
                        ui.label(format!("New name for '{}':", inst.name));
                        ui.text_edit_singleline(&mut self.rename_input);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Rename").clicked() {
                                let store = InstanceStore::new(&self.paths);
                                match store.rename_instance(&inst.instance_id, &self.rename_input) {
                                    Ok(updated) => {
                                        self.selected_instance_id = Some(updated.instance_id);
                                        self.show_rename_modal = false;
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
                                self.show_rename_modal = false;
                            }
                        });
                    });
            }
        }

        // 3. Delete Instance Confirmation Modal (Data Safety Gate)
        if self.show_delete_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                egui::Window::new("Delete Instance")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                    .show(ctx, |ui| {
                        ui.label(
                            RichText::new(format!("Permanently delete '{}'?", inst.name))
                                .strong()
                                .color(p.danger),
                        );
                        ui.label(
                            "This will permanently delete the instance folder including all saves, mods, and configs inside .minecraft/.",
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            let confirm_btn = Button::new(
                                RichText::new("Delete Permanently").color(Color32::WHITE).strong(),
                            )
                            .fill(p.danger);
                            if ui.add(confirm_btn).clicked() {
                                let store = InstanceStore::new(&self.paths);
                                match store.delete_instance(&inst.instance_id) {
                                    Ok(()) => {
                                        self.show_delete_modal = false;
                                        self.selected_instance_id = None;
                                        self.reload_instances();
                                        self.notice_banner = Some((
                                            format!("Deleted instance '{}'.", inst.name),
                                            false,
                                        ));
                                    }
                                    Err(e) => {
                                        self.show_delete_modal = false;
                                        self.notice_banner =
                                            Some((format!("Delete failed: {e}"), true));
                                    }
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                self.show_delete_modal = false;
                            }
                        });
                    });
            }
        }

        // 4. Account Manager Modal
        if self.show_account_modal {
            egui::Window::new("Accounts")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(400.0);
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
                        if self.ms_login_in_progress {
                            self.cancel_microsoft_login();
                        }
                        self.show_account_modal = false;
                    }
                });
        }

        // 5. Restore Backup Confirmation Modal
        if let Some(backup_id) = self.pending_restore_backup_id.clone() {
            egui::Window::new("Restore Backup")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(380.0);
                    ui.label(
                        RichText::new(format!("Restore backup '{backup_id}'?"))
                            .strong()
                            .color(p.warning),
                    );
                    ui.label(
                        "Current .minecraft/ contents will be replaced using a safe staging & rollback swap (original data is preserved on rollback if restore fails).",
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let btn = Button::new(
                            RichText::new("Confirm Restore").color(p.accent_text).strong(),
                        )
                        .fill(p.accent);
                        if ui.add(btn).clicked() {
                            self.pending_restore_backup_id = None;
                            self.start_backup_restore(backup_id, ctx);
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_restore_backup_id = None;
                        }
                    });
                });
        }

        // 6. Launcher Settings Separate Window (opened via top-right gear icon)
        if self.show_launcher_settings {
            let mut open = self.show_launcher_settings;
            egui::Window::new("Launcher Settings")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(520.0)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(480.0);
                    ui.set_min_height(240.0);
                    ui.horizontal_top(|ui| {
                        // Left big tab rail
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .corner_radius(CornerRadius::same(3))
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.set_min_width(136.0);
                                ui.set_min_height(210.0);
                                ui.vertical(|ui| {
                                    let is_app = self.launcher_settings_tab
                                        == LauncherSettingsTab::Appearance;
                                    let tab_btn = Button::new(
                                        RichText::new("Appearance")
                                            .strong()
                                            .size(13.5)
                                            .color(if is_app { p.accent_text } else { p.text }),
                                    )
                                    .fill(if is_app { p.accent } else { p.elevated })
                                    .min_size(Vec2::new(120.0, 34.0))
                                    .corner_radius(CornerRadius::same(3));
                                    if ui.add(tab_btn).clicked() {
                                        self.launcher_settings_tab =
                                            LauncherSettingsTab::Appearance;
                                    }
                                });
                            });

                        ui.add_space(10.0);

                        // Right content panel
                        ui.vertical(|ui| {
                            match self.launcher_settings_tab {
                                LauncherSettingsTab::Appearance => {
                                    ui.label(
                                        RichText::new("Appearance")
                                            .strong()
                                            .size(15.0)
                                            .color(p.text),
                                    );
                                    ui.label(
                                        RichText::new(
                                            "Select the launcher color theme (Default: Dark).",
                                        )
                                        .size(12.0)
                                        .color(p.text_muted),
                                    );
                                    ui.add_space(12.0);

                                    ui.horizontal(|ui| {
                                        let dark_selected = self.theme_mode == ThemeMode::Dark;
                                        let dark_btn = Button::new(
                                            RichText::new("Dark (Default)")
                                                .strong()
                                                .size(13.0)
                                                .color(if dark_selected {
                                                    p.accent_text
                                                } else {
                                                    p.text
                                                }),
                                        )
                                        .fill(if dark_selected { p.accent } else { p.elevated })
                                        .stroke(Stroke::new(
                                            1.0_f32,
                                            if dark_selected { p.accent } else { p.border },
                                        ))
                                        .min_size(Vec2::new(145.0, 42.0))
                                        .corner_radius(CornerRadius::same(3));
                                        if ui.add(dark_btn).clicked() && !dark_selected {
                                            self.theme_mode = ThemeMode::Dark;
                                            self.palette = apply_theme(ctx, self.theme_mode);
                                        }

                                        ui.add_space(8.0);

                                        let light_selected = self.theme_mode == ThemeMode::Light;
                                        let light_btn = Button::new(
                                            RichText::new("Light")
                                                .strong()
                                                .size(13.0)
                                                .color(if light_selected {
                                                    p.accent_text
                                                } else {
                                                    p.text
                                                }),
                                        )
                                        .fill(if light_selected { p.accent } else { p.elevated })
                                        .stroke(Stroke::new(
                                            1.0_f32,
                                            if light_selected { p.accent } else { p.border },
                                        ))
                                        .min_size(Vec2::new(145.0, 42.0))
                                        .corner_radius(CornerRadius::same(3));
                                        if ui.add(light_btn).clicked() && !light_selected {
                                            self.theme_mode = ThemeMode::Light;
                                            self.palette = apply_theme(ctx, self.theme_mode);
                                        }
                                    });
                                }
                            }
                        });
                    });
                });
            self.show_launcher_settings = open;
        }

        // 7. Dedicated Per-Instance Manual Downloads Windows (CurseForge opt-out mods)
        let open_ids = self.open_manual_modals.clone();
        let mut to_close: Vec<String> = Vec::new();
        let mut to_scan: Vec<String> = Vec::new();
        let mut launch_anyway_id: Option<String> = None;

        for (idx, inst_id) in open_ids.into_iter().enumerate() {
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

            let offset = (idx as f32) * 26.0;
            let mut win_open = true;
            egui::Window::new(format!(
                "Manual Downloads — {} ({} remaining)",
                inst_name,
                items.len()
            ))
            .id(egui::Id::new(("manual_downloads_window", &inst_id)))
            .open(&mut win_open)
            .collapsible(false)
            .resizable(true)
            .default_width(580.0)
            .default_pos(egui::pos2(160.0 + offset, 110.0 + offset))
            .show(ctx, |ui| {
                ui.set_min_width(540.0);
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

    fn render_tasks_bar(&self, ctx: &egui::Context) {
        if self.active_tasks.is_empty() {
            return;
        }
        let p = self.palette;
        egui::TopBottomPanel::bottom("mcw_tasks_bar")
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("ACTIVE INSTALLS ({})", self.active_tasks.len()))
                            .strong()
                            .size(11.0)
                            .color(p.accent),
                    );
                });
                ui.add_space(4.0);
                ScrollArea::vertical()
                    .id_salt("mcw_active_tasks_scroll")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for task in &self.active_tasks {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&task.label).strong().size(12.0).color(p.text));
                                ui.label(
                                    RichText::new(&task.message)
                                        .size(11.5)
                                        .color(p.text_muted),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let right_text = if task.total > 0 {
                                        let pct = ((task.current.min(task.total) as f64
                                            / task.total as f64)
                                            * 100.0)
                                            .round() as u64;
                                        format!("{}/{} ({pct}%)", task.current, task.total)
                                    } else {
                                        "Working...".to_string()
                                    };
                                    ui.label(
                                        RichText::new(right_text)
                                            .monospace()
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                });
                            });
                            let fraction = if task.total > 0 {
                                (task.current as f32 / task.total as f32).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            let bar = egui::ProgressBar::new(fraction)
                                .desired_height(6.0)
                                .fill(p.accent)
                                .corner_radius(CornerRadius::same(2))
                                .animate(task.total == 0);
                            ui.add(bar);
                            ui.add_space(4.0);
                        }
                    });
            });
    }
}

impl eframe::App for McwApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events(ctx);

        // Auto-scan Downloads & mods/ every 2s for all open Manual Downloads windows
        if !self.open_manual_modals.is_empty() && self.manual_auto_scan {
            let due = self
                .last_manual_scan_at
                .map(|t| t.elapsed() >= Duration::from_secs(2))
                .unwrap_or(true);
            if due {
                let ids = self.open_manual_modals.clone();
                for inst_id in ids {
                    self.run_manual_downloads_scan(&inst_id, false);
                }
            }
            ctx.request_repaint_after(Duration::from_secs(2));
        }

        self.render_top_bar(ctx);
        self.render_tasks_bar(ctx);
        self.render_sidebar(ctx);
        self.render_detail_pane(ctx);
        self.render_modals(ctx);
    }
}

/// Paints a crisp 8-tooth vector gear icon button (cross-platform, never relies on emoji/symbol font glyphs).
fn gear_icon_button(ui: &mut egui::Ui, p: ThemePalette, active: bool) -> egui::Response {
    let size = Vec2::new(28.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let bg = if active {
            p.accent
        } else if resp.hovered() {
            p.elevated
        } else {
            p.surface
        };
        let border = if active || resp.hovered() {
            p.accent
        } else {
            p.border
        };
        let icon_col = if active { p.accent_text } else { p.text };

        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(3),
            bg,
            Stroke::new(1.0_f32, border),
            egui::StrokeKind::Inside,
        );

        let center = rect.center();
        let outer_r = 7.2_f32;
        let inner_r = 4.6_f32;
        let hole_r = 2.2_f32;
        let teeth = 8;

        for i in 0..teeth {
            let angle = (i as f32) * std::f32::consts::TAU / (teeth as f32);
            let dir = Vec2::new(angle.cos(), angle.sin());
            let p1 = center + dir * (inner_r - 0.5);
            let p2 = center + dir * outer_r;
            painter.line_segment([p1, p2], Stroke::new(2.2_f32, icon_col));
        }
        painter.circle_stroke(center, inner_r, Stroke::new(1.8_f32, icon_col));
        painter.circle_filled(center, hole_r, bg);
    }
    resp
}

fn default_user_downloads_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Downloads"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads"))
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn open_external(target: &str) {
    if target.starts_with("http://") || target.starts_with("https://") {
        let _ = webbrowser::open(target);
        return;
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer.exe")
            .arg(target)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(target).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
    }
}

#[allow(dead_code)]
fn _keep_pathbuf_import(_: PathBuf) {}
