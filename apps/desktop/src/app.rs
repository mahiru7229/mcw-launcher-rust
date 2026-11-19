use crate::i18n::{LanguageManager, LauncherPrefs};
use crate::state::{ActiveTask, AppEvent, ChannelLogSink, ChannelProgressSink, ModVersionChoice};
use crate::theme::{apply_theme, configure_fonts, ThemeMode, ThemePalette};
use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, Frame, Layout, RichText, ScrollArea, Stroke,
    TextEdit, Vec2,
};
use mcw_auth::{AccountStore, DeviceCodeResponse, MicrosoftAuthClient, OfflineAuth, StoredAccount};
use mcw_minecraft::{
    is_quick_play_supported, OptiFineManager, RepairService, ScreenshotInfo, ScreenshotManager,
    VanillaLaunchPipeline, VersionManifestService, WorldInfo, WorldManager,
};
use mcw_models::account::AccountSource;
use mcw_models::instance::{Instance, InstanceSettings, ModLoader};
use mcw_models::progress::ProgressEvent;
use mcw_mods::{
    is_loader_compatible, ContentKind, ContentPackInfo, ContentPackManager, ModInfo, ModManager,
};
use mcw_net::DownloadClient;
use mcw_providers::{
    load_curseforge_api_key, ApiCache, AtLauncherClient, AtLauncherPack, CurseForgeClient,
    CurseForgeMod, CurseForgeSearchParams, FtbClient, FtbPackSummary, ManualDownloadItem,
    ModpackExportMode, ModpackExportOptions, ModpackInstaller, ModpackProgressCallback,
    ModrinthClient, ModrinthSearchHit, ModrinthSearchParams,
};
use mcw_runtime::{
    ExitClassification, JavaDiscovery, JavaInstallation, JavaProvisioner, LanAgentManager, LogLine,
    LogStream, McLogsClient, ProcessControl, SupervisedProcess,
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
    Packs,
    Worlds,
    Screenshots,
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
    ModrinthPacks,
    CurseForgePacks,
    Ftb,
    AtLauncher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LauncherSettingsTab {
    General,
    Downloads,
    Runtime,
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
    prefs: LauncherPrefs,
    lang: LanguageManager,
    palette: ThemePalette,

    // Core data (lightweight metadata + lazy per-instance caches so switching is instant)
    instances: Vec<Instance>,
    selected_instance_id: Option<String>,
    selected_settings: Option<InstanceSettings>,
    selected_mods: Vec<ModInfo>,
    selected_backups: Vec<BackupInfo>,
    selected_worlds: Vec<WorldInfo>,
    selected_screenshots: Vec<ScreenshotInfo>,
    selected_content_packs: Vec<ContentPackInfo>,
    settings_cache: HashMap<String, InstanceSettings>,
    mods_cache: HashMap<String, Vec<ModInfo>>,
    backups_cache: HashMap<String, Vec<BackupInfo>>,
    worlds_cache: HashMap<String, Vec<WorldInfo>>,
    screenshots_cache: HashMap<String, Vec<ScreenshotInfo>>,
    content_packs_cache: HashMap<(String, ContentKind), Vec<ContentPackInfo>>,
    loading_mods: HashSet<String>,
    loading_backups: HashSet<String>,
    loading_worlds: HashSet<String>,
    loading_screenshots: HashSet<String>,
    loading_content_packs: HashSet<(String, ContentKind)>,
    pending_delete_world: Option<(String, String)>,
    uploading_mclogs: bool,
    last_mclogs_url: Option<String>,
    // Resource Packs & Shader Packs browser
    packs_kind: ContentKind,
    packs_provider: ModProviderTab,
    packs_query: String,
    packs_searching: bool,
    packs_mr_results: Vec<ModrinthSearchHit>,
    packs_cf_results: Vec<CurseForgeMod>,
    installing_content_packs: HashSet<String>,
    accounts: Vec<StoredAccount>,
    java_installations: Vec<JavaInstallation>,
    available_versions: Vec<(String, String)>,
    versions_loading: bool,

    // Active runtime state per instance
    launch_states: HashMap<String, LaunchState>,
    instance_logs: HashMap<String, Vec<LogLine>>,
    logs_errors_only: bool,

    // Concurrent background install tasks (shown live on global bottom progress bar)
    next_task_id: u64,
    active_tasks: Vec<ActiveTask>,
    show_all_tasks_modal: bool,
    last_net_sample_bytes: u64,
    last_net_sample_at: Instant,
    current_download_bps: f64,
    installing_cf_packs: HashSet<u64>,
    installing_ext_packs: HashSet<String>,
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

    // Create instance modal (Custom | Import Archive | Modrinth | CurseForge | FTB | ATLauncher)
    show_create_modal: bool,
    create_tab: CreateInstanceTab,
    create_name: String,
    create_version: String,
    create_show_snapshots: bool,
    create_loader: String,
    create_error: Option<String>,
    modpack_path_input: String,
    modpack_name_input: String,
    modpack_manual_downloads: Vec<ManualDownloadItem>,
    pack_browser_query: String,
    pack_loader_filter: String,
    pack_browser_searching: bool,
    mr_pack_results: Vec<ModrinthSearchHit>,
    cf_pack_results: Vec<CurseForgeMod>,
    ftb_results: Vec<FtbPackSummary>,
    atl_results: Vec<AtLauncherPack>,

    // 3-Mode Export Modpack Modal (matching Python ModpackExportDialog)
    show_export_modal: bool,
    export_mode: ModpackExportMode,
    export_portable_full: bool,
    export_include_saves: bool,

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
    mod_loader_filter: String,
    mod_search_query: String,
    modrinth_results: Vec<ModrinthSearchHit>,
    curseforge_results: Vec<CurseForgeMod>,
    mod_searching: bool,

    // Async icon thumbnail texture cache for Mods, Packs, and Modpacks
    thumb_textures: HashMap<String, egui::TextureHandle>,
    loading_thumbs: HashSet<String>,

    // Version Picker modal when clicking Install on a Mod or Modrinth Modpack
    version_picker_open: bool,
    version_picker_title: String,
    version_picker_icon: Option<String>,
    version_picker_key: String,
    version_picker_is_pack: bool,
    version_picker_loading: bool,
    version_picker_items: Vec<ModVersionChoice>,
    version_picker_error: Option<String>,

    // OptiFine JAR path input inside Instance Settings tab
    optifine_jar_input: String,
}

impl McwApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: LauncherPaths) -> Self {
        configure_fonts(&cc.egui_ctx);
        let prefs = LauncherPrefs::load(&paths);
        let theme_mode = prefs.theme_mode();
        let lang = LanguageManager::load(&paths, &prefs.normalized_locale());
        let palette = apply_theme(&cc.egui_ctx, theme_mode);
        let init_show_snapshots = prefs.show_snapshots;

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
            prefs,
            lang,
            palette,
            instances: Vec::new(),
            selected_instance_id: None,
            selected_settings: None,
            selected_mods: Vec::new(),
            selected_backups: Vec::new(),
            selected_worlds: Vec::new(),
            selected_screenshots: Vec::new(),
            selected_content_packs: Vec::new(),
            settings_cache: HashMap::new(),
            mods_cache: HashMap::new(),
            backups_cache: HashMap::new(),
            worlds_cache: HashMap::new(),
            screenshots_cache: HashMap::new(),
            content_packs_cache: HashMap::new(),
            loading_mods: HashSet::new(),
            loading_backups: HashSet::new(),
            loading_worlds: HashSet::new(),
            loading_screenshots: HashSet::new(),
            loading_content_packs: HashSet::new(),
            pending_delete_world: None,
            uploading_mclogs: false,
            last_mclogs_url: None,
            packs_kind: ContentKind::ResourcePack,
            packs_provider: ModProviderTab::Modrinth,
            packs_query: String::new(),
            packs_searching: false,
            packs_mr_results: Vec::new(),
            packs_cf_results: Vec::new(),
            installing_content_packs: HashSet::new(),
            accounts: Vec::new(),
            java_installations: Vec::new(),
            available_versions: Vec::new(),
            versions_loading: false,
            launch_states: HashMap::new(),
            instance_logs: HashMap::new(),
            logs_errors_only: false,
            next_task_id: 1,
            active_tasks: Vec::new(),
            show_all_tasks_modal: false,
            last_net_sample_bytes: mcw_net::total_downloaded_bytes(),
            last_net_sample_at: Instant::now(),
            current_download_bps: 0.0,
            installing_cf_packs: HashSet::new(),
            installing_ext_packs: HashSet::new(),
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
            create_show_snapshots: init_show_snapshots,
            create_loader: "vanilla".to_string(),
            create_error: None,
            modpack_path_input: String::new(),
            modpack_name_input: String::new(),
            modpack_manual_downloads: Vec::new(),
            pack_browser_query: String::new(),
            pack_loader_filter: "fabric".to_string(),
            pack_browser_searching: false,
            mr_pack_results: Vec::new(),
            cf_pack_results: Vec::new(),
            ftb_results: Vec::new(),
            atl_results: Vec::new(),
            show_export_modal: false,
            export_mode: ModpackExportMode::Mrpack,
            export_portable_full: false,
            export_include_saves: false,
            pending_manual_counts: HashMap::new(),
            open_manual_modals: Vec::new(),
            manual_downloads_cache: HashMap::new(),
            manual_downloads_dir_input: default_downloads_dir,
            manual_auto_scan: true,
            last_manual_scan_at: None,
            manual_scan_status: HashMap::new(),
            show_launcher_settings: false,
            launcher_settings_tab: LauncherSettingsTab::General,
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
            mod_loader_filter: "fabric".to_string(),
            mod_search_query: String::new(),
            modrinth_results: Vec::new(),
            curseforge_results: Vec::new(),
            mod_searching: false,
            thumb_textures: HashMap::new(),
            loading_thumbs: HashSet::new(),
            version_picker_open: false,
            version_picker_title: String::new(),
            version_picker_icon: None,
            version_picker_key: String::new(),
            version_picker_is_pack: false,
            version_picker_loading: false,
            version_picker_items: Vec::new(),
            version_picker_error: None,
            optifine_jar_input: String::new(),
        };

        app.reload_instances();
        app.reload_accounts();
        app.reload_java();
        app.fetch_versions(&cc.egui_ctx);
        app
    }

    fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.lang.tr(key)
    }

    fn save_prefs(&mut self) {
        self.prefs.theme = match self.theme_mode {
            ThemeMode::Dark => "dark".to_string(),
            ThemeMode::Light => "light".to_string(),
        };
        self.prefs.language = self.lang.current_locale().to_string();
        self.prefs.save(&self.paths);
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
        self.selected_worlds = self.worlds_cache.get(id).cloned().unwrap_or_default();
        self.selected_screenshots = self.screenshots_cache.get(id).cloned().unwrap_or_default();
        self.selected_content_packs = self
            .content_packs_cache
            .get(&(id.clone(), self.packs_kind))
            .cloned()
            .unwrap_or_default();
    }

    fn ensure_active_tab_loaded(&mut self, ctx: &egui::Context) {
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

    fn load_content_packs_async(
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

    fn start_content_pack_search(&mut self, ctx: &egui::Context) {
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

    fn start_content_pack_install(
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

    fn load_worlds_async(&mut self, instance_id: &str, ctx: &egui::Context, force: bool) {
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

    fn load_screenshots_async(&mut self, instance_id: &str, ctx: &egui::Context, force: bool) {
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

    fn start_mclogs_upload(&mut self, content: String, ctx: &egui::Context) {
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

    fn start_launch(&mut self, ctx: &egui::Context) {
        self.start_launch_with_world(None, ctx);
    }

    fn start_launch_with_world(&mut self, quick_play_world: Option<String>, ctx: &egui::Context) {
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

                let client = DownloadClient::new().map_err(|e| e.to_string())?;
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

    fn ensure_thumbnail(&mut self, url: &str, ctx: &egui::Context) {
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

    fn render_thumbnail(&self, ui: &mut egui::Ui, url: Option<&str>, size: f32) {
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

    fn open_modrinth_version_picker(
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
        self.version_picker_key = target_key.clone();
        self.version_picker_is_pack = is_pack;
        self.version_picker_loading = true;
        self.version_picker_items.clear();
        self.version_picker_error = None;

        let (loader_opt, gv_opt) = if is_pack {
            let l = if self.pack_loader_filter.is_empty() || self.pack_loader_filter == "all" {
                None
            } else {
                Some(self.pack_loader_filter.clone())
            };
            (l, None)
        } else {
            let inst = self.selected_instance();
            let l = if self.mod_loader_filter.is_empty() || self.mod_loader_filter == "all" {
                None
            } else {
                Some(self.mod_loader_filter.clone())
            };
            (l, inst.map(|i| i.version_id.clone()))
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
                let versions = mr
                    .list_versions(&slug, loader_opt.as_deref(), gv_opt.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                let choices: Vec<ModVersionChoice> = versions
                    .into_iter()
                    .take(25)
                    .filter_map(|v| {
                        let primary = v
                            .files
                            .iter()
                            .find(|f| f.primary)
                            .or_else(|| v.files.first())?;
                        Some(ModVersionChoice {
                            id: v.id,
                            title: v.name,
                            version_number: v.version_number,
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

    fn open_curseforge_version_picker(
        &mut self,
        mod_id: u64,
        title: String,
        icon_url: Option<String>,
        ctx: &egui::Context,
    ) {
        let target_key = format!("cf:{mod_id}");
        self.version_picker_open = true;
        self.version_picker_title = title;
        self.version_picker_icon = icon_url;
        self.version_picker_key = target_key.clone();
        self.version_picker_is_pack = false;
        self.version_picker_loading = true;
        self.version_picker_items.clear();
        self.version_picker_error = None;

        let inst = self.selected_instance();
        let gv_opt = inst.map(|i| i.version_id.clone());
        let loader_opt = if self.mod_loader_filter.is_empty() || self.mod_loader_filter == "all" {
            None
        } else {
            Some(self.mod_loader_filter.clone())
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
                let files = cf
                    .get_mod_files(mod_id, gv_opt.as_deref(), loader_opt.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                let choices: Vec<ModVersionChoice> = files
                    .into_iter()
                    .take(25)
                    .map(|f| {
                        let sha1 = f
                            .hashes
                            .iter()
                            .find(|h| h.algo == 1)
                            .map(|h| h.value.clone());
                        ModVersionChoice {
                            id: f.id.to_string(),
                            title: if f.display_name.is_empty() {
                                f.file_name.clone()
                            } else {
                                f.display_name
                            },
                            version_number: format!("File #{}", f.id),
                            game_versions: f.game_versions,
                            loaders: Vec::new(),
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

    fn start_direct_mod_file_install(&mut self, choice: ModVersionChoice, ctx: &egui::Context) {
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
        let task_id = self.begin_task(
            format!("Mod: {}", choice.file_name),
            format!("Downloading into {}...", inst.name),
        );
        let dest = self
            .paths
            .minecraft_dir(&inst.instance_id)
            .join("mods")
            .join(&choice.file_name);
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let res = async {
                let downloader = DownloadClient::new().map_err(|e| e.to_string())?;
                let req = mcw_net::DownloadRequest::new(url, dest)
                    .with_sha1(choice.sha1)
                    .with_size(choice.file_size);
                downloader
                    .download_file(&req)
                    .await
                    .map_err(|e| e.to_string())?;
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

    #[allow(dead_code)]
    fn start_curseforge_mod_install(&mut self, mod_id: u64, slug: String, ctx: &egui::Context) {
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

    #[allow(dead_code)]
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

    fn start_modrinth_modpacks_search(&mut self, ctx: &egui::Context) {
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

    fn start_modrinth_pack_install(
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
                ftb.search_packs(&query, 15)
                    .await
                    .map_err(|e| e.to_string())
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

    fn start_ftb_pack_install(&mut self, pack_id: u64, pack_name: String, ctx: &egui::Context) {
        let pack_key = format!("ftb:{pack_id}");
        if !self.installing_ext_packs.insert(pack_key.clone()) {
            return;
        }
        let task_id = self.begin_task(
            format!("FTB Modpack: {pack_name}"),
            "Fetching FTB manifest...",
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
            let cb: mcw_providers::modpack::ModpackProgressCallback =
                Arc::new(move |cur, tot, msg| {
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
                let ftb = FtbClient::new(cache).map_err(|e| e.to_string())?;
                let downloader = DownloadClient::new().map_err(|e| e.to_string())?;
                ftb.install_pack_with_progress(pack_id, &pack_name, &paths, &downloader, Some(cb))
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

    fn start_atlauncher_pack_install(&mut self, pack: AtLauncherPack, ctx: &egui::Context) {
        let pack_key = format!("atl:{}", pack.id);
        if !self.installing_ext_packs.insert(pack_key.clone()) {
            return;
        }
        let task_id = self.begin_task(
            format!("ATLauncher Modpack: {}", pack.name),
            "Fetching ATLauncher manifest...",
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
            let cb: mcw_providers::modpack::ModpackProgressCallback =
                Arc::new(move |cur, tot, msg| {
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
                let atl = AtLauncherClient::new(cache).map_err(|e| e.to_string())?;
                let downloader = DownloadClient::new().map_err(|e| e.to_string())?;
                atl.install_pack_with_progress(&pack, &paths, &downloader, Some(cb))
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

    fn start_instance_export(
        &mut self,
        instance_id: String,
        options: ModpackExportOptions,
        ctx: &egui::Context,
    ) {
        let filename = match options.mode {
            ModpackExportMode::Mrpack => format!("{instance_id}.mrpack"),
            ModpackExportMode::ProviderProfile => format!("{instance_id}-MCW-Profile.zip"),
            ModpackExportMode::Portable { .. } => format!("{instance_id}.mcwpack"),
        };
        let task_id = self.begin_task(format!("Export: {filename}"), "Packaging instance files...");
        let paths = self.paths.clone();
        let output_path = self.paths.backups_root.join(&filename);
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

    fn poll_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                AppEvent::VersionsLoaded(res) => {
                    self.versions_loading = false;
                    if let Ok(list) = res {
                        if let Some((first_id, _)) = list
                            .iter()
                            .find(|(_, t)| t == "release")
                            .or_else(|| list.first())
                        {
                            if self.create_version.is_empty() {
                                self.create_version = first_id.clone();
                            }
                        }
                        self.available_versions = list;
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
                                    format!("Game crashed (exit code {:?}).{hint}", rep.exit_code),
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
                            .on_hover_text(self.t("Launcher Settings"))
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
                            None => self.t("Sign in / Add Account").to_string(),
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
                            .iter()
                            .find(|(_, t)| t == "release")
                            .or_else(|| self.available_versions.first())
                            .map(|(id, _)| id.clone())
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
                                _ if missing_manual > 0 => {
                                    (format!("Incomplete ({missing_manual})"), p.warning, false)
                                }
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
                                    RichText::new(self.t("Stop Game"))
                                        .color(Color32::WHITE)
                                        .strong(),
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
                                    RichText::new(self.t("Preparing...")).color(p.text_muted),
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
                                        RichText::new(self.t("Launch"))
                                            .color(p.accent_text)
                                            .strong(),
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
                                    if self.current_download_bps >= 1.0 {
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "↓ {}",
                                                        format_speed(self.current_download_bps)
                                                    ))
                                                    .monospace()
                                                    .size(11.5)
                                                    .color(p.accent),
                                                );
                                            },
                                        );
                                    }
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
                let mut switched_tab: Option<DetailTab> = None;
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (DetailTab::Overview, self.t("Overview")),
                        (DetailTab::Mods, self.t("Mods")),
                        (DetailTab::Packs, self.t("Packs")),
                        (DetailTab::Worlds, self.t("Worlds")),
                        (DetailTab::Screenshots, self.t("Screenshots")),
                        (DetailTab::Backups, self.t("Backups")),
                        (DetailTab::Logs, self.t("navigation.logs")),
                        (DetailTab::Settings, self.t("navigation.instance_settings")),
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
                            switched_tab = Some(tab);
                        }
                    }
                });
                if let Some(tab) = switched_tab {
                    self.active_tab = tab;
                    self.ensure_active_tab_loaded(ctx);
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                match self.active_tab {
                    DetailTab::Overview => self.render_overview_tab(ui, ctx, &inst, busy),
                    DetailTab::Mods => self.render_mods_tab(ui, ctx, &inst),
                    DetailTab::Packs => self.render_packs_tab(ui, ctx, &inst),
                    DetailTab::Worlds => self.render_worlds_tab(ui, ctx, &inst, busy),
                    DetailTab::Screenshots => self.render_screenshots_tab(ui, ctx, &inst),
                    DetailTab::Backups => self.render_backups_tab(ui, ctx, &inst, busy),
                    DetailTab::Logs => self.render_logs_tab(ui, ctx, &inst),
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

                let export_label = self.t("modpack_package.export.title").to_string();
                let mut open_export = false;
                ui.horizontal(|ui| {
                    if ui.button("Open .minecraft Folder").clicked() {
                        let _ = std::fs::create_dir_all(&mc_dir);
                        open_external(&mc_dir.display().to_string());
                    }

                    let export_resp = ui.add_enabled(!busy, Button::new(&export_label));
                    if busy {
                        export_resp.on_disabled_hover_text("Stop the instance before exporting");
                    } else if export_resp.clicked() {
                        open_export = true;
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
                if open_export {
                    self.show_export_modal = true;
                }
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
                                    if ui.add_enabled(!disabled, Button::new("Restore")).clicked() {
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

        // Mod Provider Search Bar (Modrinth | CurseForge) + Loader Filter
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_value(
                    &mut self.mod_provider_tab,
                    ModProviderTab::Modrinth,
                    "Modrinth",
                )
                .clicked()
            {
                self.mod_loader_filter = "fabric".to_string();
            }
            if ui
                .selectable_value(
                    &mut self.mod_provider_tab,
                    ModProviderTab::CurseForge,
                    "CurseForge",
                )
                .clicked()
            {
                self.mod_loader_filter = "forge".to_string();
            }
            egui::ComboBox::from_id_salt("mod_browser_loader_filter")
                .selected_text(&self.mod_loader_filter)
                .width(88.0)
                .show_ui(ui, |ui| {
                    for l in ["fabric", "forge", "neoforge", "quilt", "all"] {
                        ui.selectable_value(&mut self.mod_loader_filter, l.to_string(), l);
                    }
                });
            let hint = match self.mod_provider_tab {
                ModProviderTab::Modrinth => "Search Modrinth mods (e.g. sodium, fabric-api)...",
                ModProviderTab::CurseForge => "Search CurseForge mods (e.g. jei, create)...",
            };
            let te = ui.add(
                TextEdit::singleline(&mut self.mod_search_query)
                    .hint_text(hint)
                    .desired_width(240.0),
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

        if self.mod_provider_tab == ModProviderTab::CurseForge
            && !self.curseforge_results.is_empty()
        {
            let urls: Vec<String> = self
                .curseforge_results
                .iter()
                .filter_map(|m| m.icon_url().map(str::to_string))
                .collect();
            for u in urls {
                self.ensure_thumbnail(&u, ctx);
            }
            ui.add_space(6.0);
            let mut cf_pick: Option<(u64, String, Option<String>)> = None;
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
                    ui.add_space(4.0);
                    ScrollArea::vertical()
                        .id_salt("curseforge_results_scroll")
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for cf_mod in &self.curseforge_results {
                                Frame::new()
                                    .fill(p.elevated)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .corner_radius(CornerRadius::same(3))
                                    .inner_margin(egui::Margin::same(6))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            self.render_thumbnail(ui, cf_mod.icon_url(), 44.0);
                                            ui.add_space(6.0);
                                            ui.vertical(|ui| {
                                                ui.horizontal(|ui| {
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
                                                    let can_install = !inst.mod_loader.is_vanilla();
                                                    let btn = Button::new(
                                                        RichText::new("Install...")
                                                            .color(p.accent_text)
                                                            .strong(),
                                                    )
                                                    .fill(p.accent);
                                                    let resp = ui.add_enabled(can_install, btn);
                                                    if inst.mod_loader.is_vanilla() {
                                                        resp.on_disabled_hover_text(
                                                            "Vanilla instances cannot install mods",
                                                        );
                                                    } else if resp.clicked() {
                                                        cf_pick = Some((
                                                            cf_mod.id,
                                                            cf_mod.name.clone(),
                                                            cf_mod.icon_url().map(str::to_string),
                                                        ));
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
                                    });
                                ui.add_space(4.0);
                            }
                        });
                });
            if let Some((mid, name, icon)) = cf_pick {
                self.open_curseforge_version_picker(mid, name, icon, ctx);
            }
        }

        if self.mod_provider_tab == ModProviderTab::Modrinth && !self.modrinth_results.is_empty() {
            let urls: Vec<String> = self
                .modrinth_results
                .iter()
                .filter_map(|h| h.icon_url.clone())
                .collect();
            for u in urls {
                self.ensure_thumbnail(&u, ctx);
            }
            ui.add_space(6.0);
            let mut mr_pick: Option<(String, String, Option<String>)> = None;
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
                    ui.add_space(4.0);
                    ScrollArea::vertical()
                        .id_salt("modrinth_results_scroll")
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for hit in &self.modrinth_results {
                                Frame::new()
                                    .fill(p.elevated)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .corner_radius(CornerRadius::same(3))
                                    .inner_margin(egui::Margin::same(6))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            self.render_thumbnail(
                                                ui,
                                                hit.icon_url.as_deref(),
                                                44.0,
                                            );
                                            ui.add_space(6.0);
                                            ui.vertical(|ui| {
                                                ui.horizontal(|ui| {
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
                                                    let can_install = !inst.mod_loader.is_vanilla();
                                                    let btn = Button::new(
                                                        RichText::new("Install...")
                                                            .color(p.accent_text)
                                                            .strong(),
                                                    )
                                                    .fill(p.accent);
                                                    let resp = ui.add_enabled(can_install, btn);
                                                    if inst.mod_loader.is_vanilla() {
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
                        });
                });
            if let Some((slug, title, icon)) = mr_pick {
                self.open_modrinth_version_picker(slug, title, icon, false, ctx);
            }
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
                                    if ui.button(RichText::new("Remove").color(p.danger)).clicked()
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

    fn render_packs_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, inst: &Instance) {
        let p = self.palette;
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let prev_kind = self.packs_kind;

        // Sub-selector: Resource Packs | Shader Packs
        ui.horizontal(|ui| {
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
            ui.separator();
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
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let folder_dir = mc_dir.join(self.packs_kind.folder_name());
                if ui.button("Open Folder").clicked() {
                    let _ = std::fs::create_dir_all(&folder_dir);
                    open_external(&folder_dir.display().to_string());
                }
                if ui.button("Refresh").clicked() {
                    self.load_content_packs_async(&inst.instance_id, self.packs_kind, ctx, true);
                }
            });
        });

        if self.packs_kind != prev_kind {
            self.packs_mr_results.clear();
            self.packs_cf_results.clear();
            self.selected_content_packs = self
                .content_packs_cache
                .get(&(inst.instance_id.clone(), self.packs_kind))
                .cloned()
                .unwrap_or_default();
            self.load_content_packs_async(&inst.instance_id, self.packs_kind, ctx, false);
        }

        ui.add_space(6.0);

        // Online search bar
        ui.horizontal(|ui| {
            let hint = match self.packs_kind {
                ContentKind::ResourcePack => "Search resource packs (e.g. Faithful, Bare Bones)...",
                ContentKind::ShaderPack => "Search shaders (e.g. Complementary, BSL)...",
            };
            let te = ui.add(
                TextEdit::singleline(&mut self.packs_query)
                    .hint_text(hint)
                    .desired_width(340.0),
            );
            let go = ui
                .add_enabled(!self.packs_searching, Button::new("Search"))
                .clicked();
            if go || (te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                self.start_content_pack_search(ctx);
            }
        });

        // Modrinth search results
        if self.packs_provider == ModProviderTab::Modrinth && !self.packs_mr_results.is_empty() {
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
                                self.packs_mr_results.clear();
                            }
                        });
                    });
                    let mut to_install: Option<(String, String)> = None;
                    ScrollArea::vertical()
                        .id_salt("packs_mr_scroll")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for hit in &self.packs_mr_results {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&hit.title).strong());
                                    ui.label(
                                        RichText::new(format!("({})", hit.slug))
                                            .monospace()
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let busy_item =
                                            self.installing_content_packs.contains(&hit.slug);
                                        let label = if busy_item {
                                            "Installing..."
                                        } else {
                                            "Install"
                                        };
                                        if ui.add_enabled(!busy_item, Button::new(label)).clicked()
                                        {
                                            to_install =
                                                Some((hit.slug.clone(), hit.title.clone()));
                                        }
                                    });
                                });
                            }
                        });
                    if let Some((slug, title)) = to_install {
                        self.start_content_pack_install(false, slug, title, None, ctx);
                    }
                });
        }

        // CurseForge search results
        if self.packs_provider == ModProviderTab::CurseForge && !self.packs_cf_results.is_empty() {
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
                                self.packs_cf_results.clear();
                            }
                        });
                    });
                    let mut to_install: Option<(u64, String, String)> = None;
                    ScrollArea::vertical()
                        .id_salt("packs_cf_scroll")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for cf in &self.packs_cf_results {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&cf.name).strong());
                                    ui.label(
                                        RichText::new(format!(
                                            "#{} · {} downloads",
                                            cf.id, cf.download_count
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(p.text_muted),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let key = format!("cf-{}", cf.id);
                                        let busy_item =
                                            self.installing_content_packs.contains(&key);
                                        let label = if busy_item {
                                            "Installing..."
                                        } else {
                                            "Install"
                                        };
                                        if ui.add_enabled(!busy_item, Button::new(label)).clicked()
                                        {
                                            to_install =
                                                Some((cf.id, cf.slug.clone(), cf.name.clone()));
                                        }
                                    });
                                });
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
                });
        }

        ui.add_space(8.0);
        let section_title = match self.packs_kind {
            ContentKind::ResourcePack => {
                format!(
                    "Installed Resource Packs ({})",
                    self.selected_content_packs.len()
                )
            }
            ContentKind::ShaderPack => {
                format!(
                    "Installed Shader Packs ({})",
                    self.selected_content_packs.len()
                )
            }
        };
        ui.label(RichText::new(section_title).strong().color(p.text));
        ui.add_space(4.0);

        if self.selected_content_packs.is_empty() {
            let msg = if self
                .loading_content_packs
                .contains(&(inst.instance_id.clone(), self.packs_kind))
            {
                "Scanning folder in background..."
            } else {
                "No packs installed in this folder yet."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        let mut toggle_action: Option<(String, bool)> = None;
        let mut delete_action: Option<String> = None;

        ScrollArea::vertical()
            .id_salt("local_content_packs_scroll")
            .show(ui, |ui| {
                for pack in &self.selected_content_packs {
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let mut en = pack.enabled;
                                if ui.checkbox(&mut en, "").changed() {
                                    toggle_action = Some((pack.file_name.clone(), en));
                                }
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&pack.display_name).strong());
                                    let meta = if pack.is_dir {
                                        format!("{} · Folder", pack.file_name)
                                    } else {
                                        format!(
                                            "{} · {}",
                                            pack.file_name,
                                            format_bytes(pack.size_bytes)
                                        )
                                    };
                                    ui.label(
                                        RichText::new(meta)
                                            .monospace()
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button(RichText::new("Remove").color(p.danger)).clicked()
                                    {
                                        delete_action = Some(pack.file_name.clone());
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });

        let kind = self.packs_kind;
        if let Some((file_name, enable)) = toggle_action {
            if let Err(e) = ContentPackManager::set_enabled(&mc_dir, kind, &file_name, enable) {
                self.notice_banner = Some((format!("Failed to toggle pack: {e}"), true));
            }
            self.content_packs_cache
                .remove(&(inst.instance_id.clone(), kind));
            self.load_content_packs_async(&inst.instance_id, kind, ctx, true);
        }
        if let Some(file_name) = delete_action {
            if let Err(e) = ContentPackManager::delete(&mc_dir, kind, &file_name) {
                self.notice_banner = Some((format!("Failed to delete pack: {e}"), true));
            }
            self.content_packs_cache
                .remove(&(inst.instance_id.clone(), kind));
            self.load_content_packs_async(&inst.instance_id, kind, ctx, true);
        }
    }

    fn render_worlds_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        inst: &Instance,
        busy: bool,
    ) {
        let p = self.palette;
        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let saves_dir = mc_dir.join("saves");
        let quick_play_ok = is_quick_play_supported(&inst.version_id);
        let missing_manual = self
            .pending_manual_counts
            .get(&inst.instance_id)
            .copied()
            .unwrap_or(0);
        let has_account = self.active_account().is_some();

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "Singleplayer Worlds ({})",
                    self.selected_worlds.len()
                ))
                .strong()
                .color(p.text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Open saves/ Folder").clicked() {
                    let _ = std::fs::create_dir_all(&saves_dir);
                    open_external(&saves_dir.display().to_string());
                }
                if ui.button("Refresh").clicked() {
                    self.load_worlds_async(&inst.instance_id, ctx, true);
                }
            });
        });

        ui.add_space(6.0);

        if self.selected_worlds.is_empty() {
            let msg = if self.loading_worlds.contains(&inst.instance_id) {
                "Scanning saves/ in background..."
            } else {
                "No singleplayer worlds found in this instance's saves/ folder."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        let mut quick_play_target: Option<String> = None;
        let mut duplicate_target: Option<String> = None;
        let mut delete_target: Option<String> = None;

        ScrollArea::vertical()
            .id_salt("worlds_list_scroll")
            .show(ui, |ui| {
                for w in &self.selected_worlds {
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(&w.display_name).strong().size(13.5),
                                        );
                                        let mode_color =
                                            if w.is_hardcore { p.danger } else { p.accent };
                                        ui.label(
                                            RichText::new(w.game_mode.to_uppercase())
                                                .monospace()
                                                .size(10.5)
                                                .color(mode_color),
                                        );
                                        if !w.version_name.is_empty() {
                                            ui.label(
                                                RichText::new(format!("MC {}", w.version_name))
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(p.text_muted),
                                            );
                                        }
                                    });
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · {}",
                                            w.folder_name,
                                            format_bytes(w.size_bytes)
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(p.text_muted),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .add_enabled(
                                            !busy,
                                            Button::new(RichText::new("Delete").color(p.danger)),
                                        )
                                        .clicked()
                                    {
                                        delete_target = Some(w.folder_name.clone());
                                    }
                                    if ui.button("Folder").clicked() {
                                        open_external(&w.folder_path.display().to_string());
                                    }
                                    if ui.add_enabled(!busy, Button::new("Duplicate")).clicked() {
                                        duplicate_target = Some(w.folder_name.clone());
                                    }
                                    if quick_play_ok {
                                        let can_play = !busy && has_account && missing_manual == 0;
                                        let play_btn = Button::new(
                                            RichText::new("Quick Play")
                                                .color(p.accent_text)
                                                .strong(),
                                        )
                                        .fill(p.accent);
                                        let resp = ui.add_enabled(can_play, play_btn);
                                        if resp.clicked() {
                                            quick_play_target = Some(w.folder_name.clone());
                                        }
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });

        if let Some(folder) = quick_play_target {
            self.start_launch_with_world(Some(folder), ctx);
        }
        if let Some(folder) = duplicate_target {
            match WorldManager::duplicate_world(&mc_dir, &folder) {
                Ok(dup) => {
                    self.worlds_cache.remove(&inst.instance_id);
                    self.load_worlds_async(&inst.instance_id, ctx, true);
                    self.notice_banner = Some((
                        format!("Duplicated world '{}' -> '{}'.", folder, dup.folder_name),
                        false,
                    ));
                }
                Err(e) => {
                    self.notice_banner = Some((format!("Duplicate world failed: {e}"), true));
                }
            }
        }
        if let Some(folder) = delete_target {
            self.pending_delete_world = Some((inst.instance_id.clone(), folder));
        }
    }

    fn render_screenshots_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, inst: &Instance) {
        let p = self.palette;
        let shots_dir = self
            .paths
            .minecraft_dir(&inst.instance_id)
            .join("screenshots");

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Screenshots ({})", self.selected_screenshots.len()))
                    .strong()
                    .color(p.text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Open screenshots/ Folder").clicked() {
                    let _ = std::fs::create_dir_all(&shots_dir);
                    open_external(&shots_dir.display().to_string());
                }
                if ui.button("Refresh").clicked() {
                    self.load_screenshots_async(&inst.instance_id, ctx, true);
                }
            });
        });

        ui.add_space(6.0);

        if self.selected_screenshots.is_empty() {
            let msg = if self.loading_screenshots.contains(&inst.instance_id) {
                "Scanning screenshots/ in background..."
            } else {
                "No screenshots captured in this instance yet (press F2 in-game)."
            };
            ui.label(RichText::new(msg).color(p.text_muted));
            return;
        }

        let mut delete_path: Option<PathBuf> = None;

        ScrollArea::vertical()
            .id_salt("screenshots_list_scroll")
            .show(ui, |ui| {
                for s in &self.selected_screenshots {
                    Frame::new()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&s.file_name).monospace().strong());
                                    ui.label(
                                        RichText::new(format_bytes(s.file_size_bytes))
                                            .monospace()
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button(RichText::new("Delete").color(p.danger)).clicked()
                                    {
                                        delete_path = Some(s.path.clone());
                                    }
                                    if ui.button("Copy Path").clicked() {
                                        ctx.copy_text(s.path.display().to_string());
                                    }
                                    let open_btn = Button::new(
                                        RichText::new("Open Image").color(p.accent_text).strong(),
                                    )
                                    .fill(p.accent);
                                    if ui.add(open_btn).clicked() {
                                        open_external(&s.path.display().to_string());
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });

        if let Some(path) = delete_path {
            match ScreenshotManager::delete_screenshot(&path) {
                Ok(()) => {
                    self.screenshots_cache.remove(&inst.instance_id);
                    self.load_screenshots_async(&inst.instance_id, ctx, true);
                    self.notice_banner = Some(("Deleted screenshot.".to_string(), false));
                }
                Err(e) => {
                    self.notice_banner = Some((format!("Delete screenshot failed: {e}"), true));
                }
            }
        }
    }

    fn render_logs_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, inst: &Instance) {
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
                self.instance_logs
                    .insert(inst.instance_id.clone(), Vec::new());
            }
            if log_file.is_file() && ui.button("Open Log File").clicked() {
                open_external(&log_file.display().to_string());
            }
            let upload_label = if self.uploading_mclogs {
                "Uploading..."
            } else {
                "Upload to mclo.gs"
            };
            let upload_btn = Button::new(RichText::new(upload_label).color(p.accent_text).strong())
                .fill(p.accent);
            if ui
                .add_enabled(!self.uploading_mclogs, upload_btn)
                .on_hover_text(
                    "Automatically redacts tokens, emails, and user paths before uploading",
                )
                .clicked()
            {
                let live_text = self
                    .instance_logs
                    .get(&inst.instance_id)
                    .map(|lines| {
                        lines
                            .iter()
                            .map(|l| l.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                let content = if !live_text.trim().is_empty() {
                    live_text
                } else {
                    std::fs::read_to_string(&log_file).unwrap_or_default()
                };
                if content.trim().is_empty() {
                    self.notice_banner = Some((
                        "No live logs or latest.log file found to upload.".to_string(),
                        true,
                    ));
                } else {
                    self.start_mclogs_upload(content, ctx);
                }
            }
            if let Some(ref url) = self.last_mclogs_url {
                if ui.button("Open mclo.gs").clicked() {
                    open_external(url);
                }
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
                            ui.label(
                                RichText::new(&line.text)
                                    .monospace()
                                    .size(11.5)
                                    .color(color),
                            );
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
        let mut optifine_install_mods = false;
        let mut optifine_launch_installer = false;

        let mc_dir = self.paths.minecraft_dir(&inst.instance_id);
        let inst_dir = self.paths.instance_dir(&inst.instance_id);
        let is_forge_family = matches!(
            inst.mod_loader.name.to_ascii_lowercase().as_str(),
            "forge" | "neoforge"
        );

        ScrollArea::vertical()
            .id_salt("instance_settings_scroll")
            .show(ui, |ui| {
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
                                ui.selectable_value(
                                    &mut settings.java_path,
                                    None,
                                    "Automatic (recommended)",
                                );
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

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.label(RichText::new("MCW LAN Agent (Offline LAN Hosting)").strong());
                        ui.add_space(4.0);
                        let mut lan_on = LanAgentManager::is_enabled(
                            settings.offline_multiplayer_enabled,
                            &settings.lan_auth_mode,
                        );
                        if ui
                            .checkbox(
                                &mut lan_on,
                                "Enable MCW LAN Agent (allow offline players to join your LAN worlds)",
                            )
                            .changed()
                        {
                            settings.offline_multiplayer_enabled = lan_on;
                            settings.lan_auth_mode = if lan_on {
                                "private_offline".to_string()
                            } else {
                                "mojang".to_string()
                            };
                        }
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(
                                    "Attaches the verified embedded mcw-lan-agent.jar (-javaagent) on launch.",
                                )
                                .size(11.5)
                                .color(p.text_muted),
                            );
                            if ui.button("Open LAN Agent Log").clicked() {
                                let log_file = LanAgentManager::log_path(&inst_dir);
                                if log_file.is_file() {
                                    open_external(&log_file.display().to_string());
                                } else if let Some(parent) = log_file.parent() {
                                    let _ = std::fs::create_dir_all(parent);
                                    open_external(&parent.display().to_string());
                                }
                            }
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.label(RichText::new("OptiFine").strong());
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!(
                                "Paste path to an official OptiFine_{}_*.jar file to validate and install.",
                                inst.version_id
                            ))
                            .size(11.5)
                            .color(p.text_muted),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                TextEdit::singleline(&mut self.optifine_jar_input)
                                    .hint_text("C:\\Users\\...\\Downloads\\OptiFine_1.20.4_HD_U_I7.jar")
                                    .desired_width(360.0),
                            );
                            if is_forge_family {
                                if ui.button("Install to mods/").clicked() {
                                    optifine_install_mods = true;
                                }
                            } else if ui.button("Run OptiFine Installer").clicked() {
                                optifine_launch_installer = true;
                            }
                            if ui.button("optifine.net").clicked() {
                                open_external("https://optifine.net/downloads");
                            }
                        });

                        ui.add_space(12.0);
                        let save_btn =
                            Button::new(RichText::new("Save Settings").color(p.accent_text).strong())
                                .fill(p.accent)
                                .corner_radius(CornerRadius::same(3));
                        if ui.add(save_btn).clicked() {
                            save_clicked = true;
                        }
                    });
            });

        if optifine_install_mods {
            let jar_path = PathBuf::from(self.optifine_jar_input.trim().trim_matches('"'));
            match OptiFineManager::install_into_mods(&jar_path, &mc_dir, Some(&inst.version_id)) {
                Ok(dest) => {
                    self.mods_cache.remove(&inst.instance_id);
                    self.optifine_jar_input.clear();
                    self.notice_banner =
                        Some((format!("Installed OptiFine into {}", dest.display()), false));
                }
                Err(e) => {
                    self.notice_banner = Some((format!("OptiFine install failed: {e}"), true));
                }
            }
        }
        if optifine_launch_installer {
            let jar_path = PathBuf::from(self.optifine_jar_input.trim().trim_matches('"'));
            let java_exe = self
                .selected_settings
                .as_ref()
                .and_then(|s| s.java_path.clone())
                .or_else(|| self.java_installations.first().map(|j| j.path.clone()));
            match java_exe {
                Some(java) => match OptiFineManager::launch_official_installer(
                    &jar_path,
                    &java,
                    Some(&inst.version_id),
                ) {
                    Ok(()) => {
                        self.notice_banner = Some((
                            "Launched official OptiFine installer window.".to_string(),
                            false,
                        ));
                    }
                    Err(e) => {
                        self.notice_banner =
                            Some((format!("OptiFine installer failed: {e}"), true));
                    }
                },
                None => {
                    self.notice_banner = Some((
                        "No Java runtime found. Install Java 17 or 21 first.".to_string(),
                        true,
                    ));
                }
            }
        }

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
                    ui.set_min_width(540.0);
                    ui.horizontal_wrapped(|ui| {
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
                        if ui
                            .selectable_value(
                                &mut self.create_tab,
                                CreateInstanceTab::ModrinthPacks,
                                "Modrinth Packs",
                            )
                            .clicked()
                        {
                            self.pack_loader_filter = "fabric".to_string();
                        }
                        if ui
                            .selectable_value(
                                &mut self.create_tab,
                                CreateInstanceTab::CurseForgePacks,
                                "CurseForge Packs",
                            )
                            .clicked()
                        {
                            self.pack_loader_filter = "forge".to_string();
                        }
                        if ui
                            .selectable_value(
                                &mut self.create_tab,
                                CreateInstanceTab::Ftb,
                                "FTB Modpacks",
                            )
                            .clicked()
                        {
                            self.pack_loader_filter = "forge".to_string();
                        }
                        if ui
                            .selectable_value(
                                &mut self.create_tab,
                                CreateInstanceTab::AtLauncher,
                                "ATLauncher",
                            )
                            .clicked()
                        {
                            self.pack_loader_filter = "forge".to_string();
                        }
                    });
                    ui.separator();
                    ui.add_space(6.0);

                    match self.create_tab {
                        CreateInstanceTab::Custom => {
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
                                            let _ = store
                                                .save_settings(&created.instance_id, &inst_settings);
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
                        }
                        CreateInstanceTab::ModrinthPacks => {
                            ui.horizontal(|ui| {
                                let te = ui.add(
                                    TextEdit::singleline(&mut self.pack_browser_query)
                                        .hint_text("Search Modrinth modpacks (e.g. Fabulously Optimized)...")
                                        .desired_width(260.0),
                                );
                                egui::ComboBox::from_id_salt("mr_pack_loader_filter")
                                    .selected_text(&self.pack_loader_filter)
                                    .width(90.0)
                                    .show_ui(ui, |ui| {
                                        for l in ["fabric", "forge", "neoforge", "quilt", "all"] {
                                            ui.selectable_value(
                                                &mut self.pack_loader_filter,
                                                l.to_string(),
                                                l,
                                            );
                                        }
                                    });
                                let go = ui
                                    .add_enabled(!self.pack_browser_searching, Button::new("Search"))
                                    .clicked();
                                if go
                                    || (te.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    self.start_modrinth_modpacks_search(ctx);
                                }
                            });
                            ui.add_space(6.0);
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
                                .id_salt("create_mr_packs_scroll")
                                .max_height(260.0)
                                .show(ui, |ui| {
                                    if self.mr_pack_results.is_empty() {
                                        ui.label(
                                            RichText::new("Click Search to browse Modrinth modpacks (default loader: Fabric).")
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
                                                    ui.set_width(ui.available_width());
                                                    ui.horizontal(|ui| {
                                                        self.render_thumbnail(
                                                            ui,
                                                            hit.icon_url.as_deref(),
                                                            40.0,
                                                        );
                                                        ui.add_space(6.0);
                                                        ui.vertical(|ui| {
                                                            ui.horizontal(|ui| {
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
                                self.open_modrinth_version_picker(slug, title, icon, true, ctx);
                            }
                            ui.add_space(8.0);
                            if ui.button("Close").clicked() {
                                self.show_create_modal = false;
                            }
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
                                        .desired_width(260.0),
                                );
                                egui::ComboBox::from_id_salt("cf_pack_loader_filter")
                                    .selected_text(&self.pack_loader_filter)
                                    .width(90.0)
                                    .show_ui(ui, |ui| {
                                        for l in ["forge", "fabric", "neoforge", "quilt", "all"] {
                                            ui.selectable_value(
                                                &mut self.pack_loader_filter,
                                                l.to_string(),
                                                l,
                                            );
                                        }
                                    });
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
                            let cf_urls: Vec<String> = self
                                .cf_pack_results
                                .iter()
                                .filter_map(|m| m.icon_url().map(str::to_string))
                                .collect();
                            for u in cf_urls {
                                self.ensure_thumbnail(&u, ctx);
                            }
                            let mut install_cf_pack: Option<(u64, String, String)> = None;
                            ScrollArea::vertical()
                                .id_salt("create_cf_packs_scroll")
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    if self.cf_pack_results.is_empty() {
                                        ui.label(
                                            RichText::new("Search CurseForge modpacks to browse and install (default loader: Forge).")
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
                                                        self.render_thumbnail(
                                                            ui,
                                                            pack.icon_url(),
                                                            40.0,
                                                        );
                                                        ui.add_space(6.0);
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
                            let mut ftb_install_req: Option<(u64, String)> = None;
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
                            if let Some((pid, pname)) = ftb_install_req {
                                self.start_ftb_pack_install(pid, pname, ctx);
                            }
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
                            let mut atl_install_req: Option<AtLauncherPack> = None;
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
                                self.start_atlauncher_pack_install(pack, ctx);
                            }
                            ui.add_space(8.0);
                            if ui.button("Close").clicked() {
                                self.show_create_modal = false;
                            }
                        }
                    }
                });
        }

        // 1b. 3-Mode Export Modpack Modal (mirroring Python ModpackExportDialog)
        if self.show_export_modal {
            if let Some(inst) = self.selected_instance().cloned() {
                let title = self.t("modpack_package.export.title").to_string();
                let mut trigger_export: Option<ModpackExportOptions> = None;
                let mut open = self.show_export_modal;
                egui::Window::new(title)
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                    .show(ctx, |ui| {
                        ui.set_min_width(480.0);
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

                        // Mode 1: Modrinth (.mrpack)
                        let is_mrpack = matches!(self.export_mode, ModpackExportMode::Mrpack);
                        if ui
                            .radio(is_mrpack, self.t("modpack_package.export.mrpack"))
                            .clicked()
                        {
                            self.export_mode = ModpackExportMode::Mrpack;
                        }
                        ui.label(
                            RichText::new(self.t("modpack_package.export.mrpack_help"))
                                .size(11.0)
                                .color(p.text_muted),
                        );
                        ui.add_space(6.0);

                        // Mode 2: Provider Profile (.zip)
                        let is_provider =
                            matches!(self.export_mode, ModpackExportMode::ProviderProfile);
                        if ui
                            .radio(
                                is_provider,
                                self.t("modpack_package.export.provider_profile"),
                            )
                            .clicked()
                        {
                            self.export_mode = ModpackExportMode::ProviderProfile;
                        }
                        ui.label(
                            RichText::new(self.t("modpack_package.export.provider_profile_help"))
                                .size(11.0)
                                .color(p.text_muted),
                        );
                        ui.add_space(6.0);

                        // Mode 3: Portable (.mcwpack) with Smart / Full
                        let is_portable =
                            matches!(self.export_mode, ModpackExportMode::Portable { .. });
                        if ui
                            .radio(is_portable, self.t("modpack_package.export.portable"))
                            .clicked()
                        {
                            self.export_mode = ModpackExportMode::Portable {
                                full: self.export_portable_full,
                            };
                        }
                        ui.label(
                            RichText::new(self.t("modpack_package.export.portable_help"))
                                .size(11.0)
                                .color(p.text_muted),
                        );

                        if is_portable {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                let smart_lbl = self.t("modpack_package.export.smart").to_string();
                                let full_lbl = self.t("modpack_package.export.full").to_string();
                                if ui
                                    .selectable_value(
                                        &mut self.export_portable_full,
                                        false,
                                        smart_lbl,
                                    )
                                    .clicked()
                                {
                                    self.export_mode = ModpackExportMode::Portable { full: false };
                                }
                                if ui
                                    .selectable_value(
                                        &mut self.export_portable_full,
                                        true,
                                        full_lbl,
                                    )
                                    .clicked()
                                {
                                    self.export_mode = ModpackExportMode::Portable { full: true };
                                }
                            });
                        }

                        ui.add_space(8.0);
                        let saves_enabled = is_mrpack || is_portable;
                        let saves_lbl = self.t("workspace.export.include_saves").to_string();
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
                        ui.add_space(10.0);

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
                                    include_saves: saves_enabled && self.export_include_saves,
                                });
                            }
                            if ui.button(self.t("common.cancel")).clicked() {
                                self.show_export_modal = false;
                            }
                        });
                    });
                self.show_export_modal = open;
                if let Some(opts) = trigger_export {
                    self.show_export_modal = false;
                    self.start_instance_export(inst.instance_id, opts, ctx);
                }
            }
        }

        // 1c. Mod / Modpack Version Picker Modal
        if self.version_picker_open {
            let mut open = self.version_picker_open;
            let mut chosen: Option<ModVersionChoice> = None;
            let title = format!("Select Version — {}", self.version_picker_title);
            egui::Window::new(title)
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(540.0)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        self.render_thumbnail(ui, self.version_picker_icon.as_deref(), 40.0);
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&self.version_picker_title)
                                    .strong()
                                    .size(14.5),
                            );
                            ui.label(
                                RichText::new(if self.version_picker_is_pack {
                                    "Choose a modpack version to download and install."
                                } else {
                                    "Choose a compatible version to install into mods/."
                                })
                                .size(11.5)
                                .color(p.text_muted),
                            );
                        });
                    });
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    if self.version_picker_loading {
                        ui.label(RichText::new("Loading versions...").color(p.accent));
                    } else if let Some(ref err) = self.version_picker_error {
                        ui.label(RichText::new(format!("Error: {err}")).color(p.danger));
                    } else if self.version_picker_items.is_empty() {
                        ui.label(
                            RichText::new(
                                "No compatible versions found for the selected Minecraft version / loader.",
                            )
                            .color(p.text_muted),
                        );
                    } else {
                        ScrollArea::vertical()
                            .id_salt("version_picker_scroll")
                            .max_height(320.0)
                            .show(ui, |ui| {
                                for item in &self.version_picker_items {
                                    Frame::new()
                                        .fill(p.elevated)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .corner_radius(CornerRadius::same(3))
                                        .inner_margin(egui::Margin::symmetric(8, 6))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.vertical(|ui| {
                                                    ui.label(
                                                        RichText::new(&item.title).strong(),
                                                    );
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
            self.version_picker_open = open;
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

        // 5b. Delete World Confirmation Modal (Data Safety Gate)
        if let Some((inst_id, folder_name)) = self.pending_delete_world.clone() {
            egui::Window::new("Delete World")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(360.0);
                    ui.label(
                        RichText::new(format!("Permanently delete world '{folder_name}'?"))
                            .strong()
                            .color(p.danger),
                    );
                    ui.label("This will permanently remove the world save folder from saves/.");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let confirm_btn = Button::new(
                            RichText::new("Delete World").color(Color32::WHITE).strong(),
                        )
                        .fill(p.danger);
                        if ui.add(confirm_btn).clicked() {
                            let mc_dir = self.paths.minecraft_dir(&inst_id);
                            match WorldManager::delete_world(&mc_dir, &folder_name) {
                                Ok(()) => {
                                    self.pending_delete_world = None;
                                    self.worlds_cache.remove(&inst_id);
                                    self.load_worlds_async(&inst_id, ctx, true);
                                    self.notice_banner =
                                        Some((format!("Deleted world '{folder_name}'."), false));
                                }
                                Err(e) => {
                                    self.pending_delete_world = None;
                                    self.notice_banner =
                                        Some((format!("Delete world failed: {e}"), true));
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_delete_world = None;
                        }
                    });
                });
        }

        // 6. Launcher Settings Separate Window (rebuilt from Python LauncherSettingsPage,
        // keeping only active Rust Edition sections & official lang/en-US.json + vi-VN.json keys)
        if self.show_launcher_settings {
            let mut open = self.show_launcher_settings;
            let mut prefs_dirty = false;
            let mut reset_defaults = false;
            let mut reload_langs = false;
            let mut scan_java = false;
            let mut install_java_major: Option<u32> = None;
            let win_title = self.t("launcher_settings.page.title").to_string();

            egui::Window::new(win_title)
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(680.0)
                .default_height(460.0)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(600.0);
                    ui.set_min_height(380.0);
                    ui.horizontal_top(|ui| {
                        // Left section navigation rail (matching Python's 4 active sections)
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .corner_radius(CornerRadius::same(3))
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.set_min_width(156.0);
                                ui.set_min_height(340.0);
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
                                ui.vertical(|ui| {
                                    for (tab, label) in nav_tabs {
                                        let active = self.launcher_settings_tab == tab;
                                        let btn = Button::new(
                                            RichText::new(label)
                                                .strong()
                                                .size(13.0)
                                                .color(if active { p.accent_text } else { p.text }),
                                        )
                                        .fill(if active { p.accent } else { p.elevated })
                                        .min_size(Vec2::new(140.0, 34.0))
                                        .corner_radius(CornerRadius::same(3));
                                        if ui.add(btn).clicked() {
                                            self.launcher_settings_tab = tab;
                                        }
                                        ui.add_space(6.0);
                                    }
                                });
                            });

                        ui.add_space(10.0);

                        // Right scrollable content panel
                        ScrollArea::vertical()
                            .id_salt("launcher_settings_content_scroll")
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
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
                                            });

                                        ui.add_space(8.0);

                                        // Card 2: Language (official Python LanguageManager)
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
                                                let mut picked_locale: Option<String> = None;
                                                ui.horizontal_wrapped(|ui| {
                                                    for (loc, name) in &available {
                                                        let selected = loc == &current_loc;
                                                        let label = format!("{name} ({loc})");
                                                        let btn = Button::new(
                                                            RichText::new(label)
                                                                .strong()
                                                                .size(12.5)
                                                                .color(if selected {
                                                                    p.accent_text
                                                                } else {
                                                                    p.text
                                                                }),
                                                        )
                                                        .fill(if selected {
                                                            p.accent
                                                        } else {
                                                            p.elevated
                                                        })
                                                        .stroke(Stroke::new(
                                                            1.0_f32,
                                                            if selected {
                                                                p.accent
                                                            } else {
                                                                p.border
                                                            },
                                                        ))
                                                        .corner_radius(CornerRadius::same(3));
                                                        if ui.add(btn).clicked() && !selected {
                                                            picked_locale = Some(loc.clone());
                                                        }
                                                    }
                                                });
                                                if let Some(loc) = picked_locale {
                                                    self.lang.set_language(&loc);
                                                    prefs_dirty = true;
                                                }
                                                ui.add_space(6.0);
                                                let reload_lbl = self
                                                    .t("launcher_settings.language.reload")
                                                    .to_string();
                                                if ui.button(reload_lbl).clicked() {
                                                    reload_langs = true;
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
                                                ui.horizontal(|ui| {
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
                                                ui.horizontal(|ui| {
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
                                                ui.horizontal(|ui| {
                                                    let dark_selected =
                                                        self.theme_mode == ThemeMode::Dark;
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
                                                    .fill(if dark_selected {
                                                        p.accent
                                                    } else {
                                                        p.elevated
                                                    })
                                                    .stroke(Stroke::new(
                                                        1.0_f32,
                                                        if dark_selected {
                                                            p.accent
                                                        } else {
                                                            p.border
                                                        },
                                                    ))
                                                    .min_size(Vec2::new(145.0, 40.0))
                                                    .corner_radius(CornerRadius::same(3));
                                                    if ui.add(dark_btn).clicked() && !dark_selected
                                                    {
                                                        self.theme_mode = ThemeMode::Dark;
                                                        self.palette =
                                                            apply_theme(ctx, self.theme_mode);
                                                        prefs_dirty = true;
                                                    }

                                                    ui.add_space(8.0);

                                                    let light_selected =
                                                        self.theme_mode == ThemeMode::Light;
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
                                                    .fill(if light_selected {
                                                        p.accent
                                                    } else {
                                                        p.elevated
                                                    })
                                                    .stroke(Stroke::new(
                                                        1.0_f32,
                                                        if light_selected {
                                                            p.accent
                                                        } else {
                                                            p.border
                                                        },
                                                    ))
                                                    .min_size(Vec2::new(145.0, 40.0))
                                                    .corner_radius(CornerRadius::same(3));
                                                    if ui.add(light_btn).clicked()
                                                        && !light_selected
                                                    {
                                                        self.theme_mode = ThemeMode::Light;
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

    fn sample_network_speed(&mut self) {
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

    fn render_single_task_card(
        ui: &mut egui::Ui,
        p: ThemePalette,
        task: &ActiveTask,
        speed_text: &str,
    ) {
        Frame::new()
            .fill(p.elevated)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(3))
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&task.label).strong().size(12.0).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let right_text = if task.total > 0 {
                            let pct = ((task.current.min(task.total) as f64 / task.total as f64)
                                * 100.0)
                                .round() as u64;
                            if speed_text.is_empty() {
                                format!("{}/{} ({pct}%)", task.current, task.total)
                            } else {
                                format!("{}/{} ({pct}%) · {speed_text}", task.current, task.total)
                            }
                        } else if speed_text.is_empty() {
                            "Working...".to_string()
                        } else {
                            speed_text.to_string()
                        };
                        ui.label(
                            RichText::new(right_text)
                                .monospace()
                                .size(11.0)
                                .color(p.accent),
                        );
                    });
                });
                if !task.message.is_empty() {
                    ui.label(RichText::new(&task.message).size(11.0).color(p.text_muted));
                }
                ui.add_space(3.0);
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
            });
    }

    fn render_tasks_bar(&mut self, ctx: &egui::Context) {
        let total_tasks = self.active_tasks.len();
        if total_tasks <= 2 {
            self.show_all_tasks_modal = false;
        }
        if total_tasks == 0 {
            return;
        }
        let p = self.palette;
        let speed_text = if self.current_download_bps >= 1.0 {
            format!("↓ {}", format_speed(self.current_download_bps))
        } else {
            String::new()
        };

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
                        RichText::new(format!("ACTIVE TASKS ({total_tasks})"))
                            .strong()
                            .size(11.0)
                            .color(p.accent),
                    );
                    if !speed_text.is_empty() {
                        ui.label(
                            RichText::new(&speed_text)
                                .monospace()
                                .size(11.0)
                                .color(p.text_muted),
                        );
                    }
                    if total_tasks > 2 {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let hidden = total_tasks - 2;
                            let btn_label = format!("View All ({total_tasks}, +{hidden} more)");
                            if ui.button(btn_label).clicked() {
                                self.show_all_tasks_modal = !self.show_all_tasks_modal;
                            }
                        });
                    }
                });
                ui.add_space(6.0);

                // Show at most 2 representative tasks in the bottom bar; responsive 2-column when wide
                let visible = &self.active_tasks[..total_tasks.min(2)];
                if visible.len() == 2 && ui.available_width() >= 760.0 {
                    ui.columns(2, |cols| {
                        Self::render_single_task_card(&mut cols[0], p, &visible[0], &speed_text);
                        Self::render_single_task_card(&mut cols[1], p, &visible[1], &speed_text);
                    });
                } else {
                    for (idx, task) in visible.iter().enumerate() {
                        Self::render_single_task_card(ui, p, task, &speed_text);
                        if idx + 1 < visible.len() {
                            ui.add_space(4.0);
                        }
                    }
                }
            });

        // Overflow modal when > 2 tasks are running and user clicked "View All"
        if self.show_all_tasks_modal && total_tasks > 2 {
            let mut open = self.show_all_tasks_modal;
            egui::Window::new(format!("All Active Tasks ({total_tasks})"))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(540.0)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    if !speed_text.is_empty() {
                        ui.label(
                            RichText::new(format!("Network Speed: {speed_text}"))
                                .monospace()
                                .size(11.5)
                                .color(p.accent),
                        );
                        ui.add_space(6.0);
                    }
                    ScrollArea::vertical()
                        .id_salt("all_active_tasks_modal_scroll")
                        .max_height(360.0)
                        .show(ui, |ui| {
                            for task in &self.active_tasks {
                                Self::render_single_task_card(ui, p, task, &speed_text);
                                ui.add_space(4.0);
                            }
                        });
                });
            self.show_all_tasks_modal = open;
        }
    }
}

impl eframe::App for McwApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events(ctx);

        // Live download speed sampling whenever any task or launch download is active
        let any_preparing = self
            .launch_states
            .values()
            .any(|s| matches!(s, LaunchState::Preparing(_)));
        if !self.active_tasks.is_empty() || any_preparing {
            self.sample_network_speed();
            ctx.request_repaint_after(Duration::from_millis(500));
        } else {
            self.current_download_bps = 0.0;
            self.last_net_sample_bytes = mcw_net::total_downloaded_bytes();
            self.last_net_sample_at = Instant::now();
        }

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

fn format_speed(bps: f64) -> String {
    if bps >= 1024.0 * 1024.0 {
        format!("{:.2} MB/s", bps / (1024.0 * 1024.0))
    } else if bps >= 1024.0 {
        format!("{:.1} KB/s", bps / 1024.0)
    } else {
        format!("{:.0} B/s", bps)
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
