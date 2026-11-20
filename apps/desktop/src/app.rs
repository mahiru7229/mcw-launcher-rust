mod controllers;
mod platform;
mod views;

use self::platform::default_user_downloads_dir;
use crate::i18n::{LanguageManager, LauncherPrefs};
use crate::state::{
    ActiveTask, AppEvent, ChannelLogSink, ChannelProgressSink, ModVersionChoice, ModpackChannel,
    ModpackVersionOption,
};
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
use mcw_net::{DownloadClient, TaskControl};
use mcw_providers::{
    load_curseforge_api_key, ApiCache, AtLauncherClient, AtLauncherPack, CurseForgeClient,
    CurseForgeMod, CurseForgeSearchParams, FtbClient, FtbPackSummary, LanHostingManager,
    ManualDownloadItem, ModProvenanceRegistry, ModUpdateCandidate, ModUpdateManager,
    ModpackExportMode, ModpackExportOptions, ModpackInstaller, ModpackProgressCallback,
    ModrinthClient, ModrinthSearchHit, ModrinthSearchParams, TrackedModEntry,
};
use mcw_runtime::{
    detect_preset, detect_total_memory_mb, find_latest_crash_report, get_preset_flags,
    recommended_modpack_ram_mb, DiagnosticsExporter, ExitClassification, JavaDiscovery,
    JavaInstallation, JavaProvisioner, JvmPresetId, LanAgentManager, LaunchErrorPresenter, LogLine,
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
    Preparing(#[allow(dead_code)] ProgressEvent),
    Running { pid: u32, control: ProcessControl },
}

#[derive(Debug, Clone)]
pub(crate) enum ModpackInstallProvider {
    CurseForge { mod_id: u64, slug: String },
    Modrinth { slug: String },
    Ftb { pack_id: u64 },
    AtLauncher { pack: AtLauncherPack },
}

#[derive(Debug, Clone, Default)]
struct ModpackInstallDialogState {
    open: bool,
    request_id: u64,
    provider: Option<ModpackInstallProvider>,
    provider_label: String,
    pack_title: String,
    icon_url: Option<String>,
    website_url: Option<String>,
    instance_name: String,
    default_loader: String,
    versions: Vec<ModpackVersionOption>,
    selected_version_id: String,
    include_beta: bool,
    include_alpha: bool,
    loading_versions: bool,
    error: Option<String>,
    min_ram_mb: u32,
    max_ram_mb: u32,
    ram_user_edited: bool,
    java_custom: bool,
    java_path: String,
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

    // Rename, Clone & Delete modals
    show_rename_modal: bool,
    rename_input: String,
    show_clone_modal: bool,
    clone_name_input: String,
    clone_include_saves: bool,
    show_delete_modal: bool,

    // Sidebar filter & Instance runtime/metadata/arguments edit buffers
    sidebar_search_query: String,
    edit_runtime_version: String,
    edit_runtime_loader: String,
    edit_group_input: String,
    edit_tags_input: String,
    edit_jvm_args_text: String,
    edit_game_args_text: String,

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

    // Local instance icon texture cache (keyed by "<instance_id>|<icon>")
    instance_icon_textures: HashMap<String, Option<egui::TextureHandle>>,

    // Detected system physical RAM (MB) for Forge (min(PC_RAM, 8192)) vs general (min(PC_RAM, 4096))
    pc_ram_mb: u32,
    // Mapping from instance_id -> ActiveTask id for Launch tasks (unified bottom progress bar)
    launch_task_ids: HashMap<String, u64>,

    // Dedicated Modpack Install Configuration Dialog (isolated OS window)
    modpack_dialog: ModpackInstallDialogState,
    next_modpack_req_id: u64,

    // Version Picker modal when clicking Install on a Mod
    version_picker_open: bool,
    version_picker_title: String,
    version_picker_icon: Option<String>,
    version_picker_website_url: Option<String>,
    version_picker_key: String,
    version_picker_is_pack: bool,
    version_picker_loading: bool,
    version_picker_items: Vec<ModVersionChoice>,
    version_picker_error: Option<String>,
    version_picker_include_beta: bool,
    version_picker_include_alpha: bool,
    version_picker_mc_filter: String,
    version_picker_loader_filter: String,
    version_picker_selected_id: String,

    // OptiFine JAR path input inside Instance Settings tab
    optifine_jar_input: String,

    // Mod Provenance & Update Manager state
    mod_update_candidates: Vec<ModUpdateCandidate>,
    checking_mod_updates: bool,

    // Auto-dismiss notification tracking (default 8s, configurable in Launcher Settings)
    notice_banner_last_text: Option<String>,
    notice_banner_since: Option<Instant>,

    // Dedicated Isolated OS Windows for Mod Browser, Pack Browser, Modpack Provider Browser, and Group & Tags
    show_mod_browser_window: bool,
    show_pack_browser_window: bool,
    show_modpack_provider_window: Option<CreateInstanceTab>,
    show_group_tags_modal: bool,
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
            create_version: String::new(),
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
            show_clone_modal: false,
            clone_name_input: String::new(),
            clone_include_saves: false,
            show_delete_modal: false,
            sidebar_search_query: String::new(),
            edit_runtime_version: String::new(),
            edit_runtime_loader: "vanilla".to_string(),
            edit_group_input: String::new(),
            edit_tags_input: String::new(),
            edit_jvm_args_text: String::new(),
            edit_game_args_text: String::new(),
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
            instance_icon_textures: HashMap::new(),
            pc_ram_mb: detect_total_memory_mb(),
            launch_task_ids: HashMap::new(),
            modpack_dialog: ModpackInstallDialogState::default(),
            next_modpack_req_id: 1,
            version_picker_open: false,
            version_picker_title: String::new(),
            version_picker_icon: None,
            version_picker_website_url: None,
            version_picker_key: String::new(),
            version_picker_is_pack: false,
            version_picker_loading: false,
            version_picker_items: Vec::new(),
            version_picker_error: None,
            version_picker_include_beta: false,
            version_picker_include_alpha: false,
            version_picker_mc_filter: "all".to_string(),
            version_picker_loader_filter: "all".to_string(),
            version_picker_selected_id: String::new(),
            optifine_jar_input: String::new(),
            mod_update_candidates: Vec::new(),
            checking_mod_updates: false,
            notice_banner_last_text: None,
            notice_banner_since: None,
            show_mod_browser_window: false,
            show_pack_browser_window: false,
            show_modpack_provider_window: None,
            show_group_tags_modal: false,
        };

        app.reload_instances();
        app.reload_accounts();
        app.reload_java();
        app.fetch_versions(&cc.egui_ctx);
        app
    }
}

impl eframe::App for McwApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events(ctx);

        // Auto-dismiss notice_banner after `notification_timeout_secs` (default 8s; 0 = disabled)
        let current_notice_msg = self.notice_banner.as_ref().map(|(m, _)| m.clone());
        if current_notice_msg != self.notice_banner_last_text {
            self.notice_banner_last_text = current_notice_msg.clone();
            self.notice_banner_since = current_notice_msg.as_ref().map(|_| Instant::now());
        }
        let timeout_secs = self.prefs.notification_timeout_secs;
        if timeout_secs > 0 {
            if let Some(since) = self.notice_banner_since {
                let limit = Duration::from_secs(timeout_secs as u64);
                let elapsed = since.elapsed();
                if elapsed >= limit {
                    self.notice_banner = None;
                    self.notice_banner_last_text = None;
                    self.notice_banner_since = None;
                } else {
                    ctx.request_repaint_after(limit - elapsed);
                }
            }
        }

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
