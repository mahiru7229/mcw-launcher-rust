mod controllers;
mod controllers_pages;
mod platform;
mod views;

use self::platform::default_user_downloads_dir;
use crate::background::BackgroundManager;
use crate::i18n::{LanguageManager, LauncherPrefs};
use crate::skin_render::{default_skin_face_rgba, render_skin_face_rgba};
use crate::state::{
    ActiveTask, AppEvent, BrowserCategory, BrowserProvider, ChannelLogSink, ChannelProgressSink,
    ModVersionChoice, ModpackChannel, ModpackVersionOption,
};
use crate::theme::{
    apply_custom_accent, apply_theme_palette, configure_fonts, format_hex_color, parse_hex_color,
    MotionMode, ThemeCatalog, ThemeMode, ThemePalette,
};
use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, Frame, Layout, RichText, ScrollArea, Stroke,
    TextEdit, Vec2,
};
use mcw_auth::{
    AccountSkinManager, AccountStore, DeviceCodeResponse, MicrosoftAuthClient, OfflineAuth,
    StoredAccount,
};
use mcw_minecraft::{
    is_quick_play_supported, ForgePreflightManager, LanAgentTargetResolver,
    LoaderDiagnosticsExporter, OptiFineManager, OptiFineState, RepairService, SavedServer,
    ScreenshotInfo, ScreenshotManager, ServerManager, VanillaLaunchPipeline,
    VersionManifestService, WorldInfo, WorldManager,
};
use mcw_models::account::AccountSource;
use mcw_models::instance::{Instance, InstanceSettings, ModLoader};
use mcw_models::progress::ProgressEvent;
use mcw_models::ManagedContentPolicy;
use mcw_mods::{
    is_loader_compatible, ContentKind, ContentPackInfo, ContentPackManager,
    ModCompatibilityAnalyzer, ModCompatibilityReport, ModInfo, ModIssueSeverity, ModManager,
};
use mcw_net::{DownloadClient, TaskControl};
use mcw_providers::{
    detect_installed_pack, load_curseforge_api_key, load_custom_curseforge_api_key,
    save_custom_curseforge_api_key, ApiCache, AtLauncherClient, AtLauncherPack, CurseForgeClient,
    CurseForgeMod, CurseForgeSearchParams, FtbClient, FtbPackSummary, LanHostingManager,
    ManualDownloadItem, ModProvenanceRegistry, ModUpdateCandidate, ModUpdateManager,
    ModpackExportMode, ModpackExportOptions, ModpackInstallOptions, ModpackInstaller,
    ModpackOptionalFile, ModpackProgressCallback, ModrinthClient, ModrinthSearchHit,
    ModrinthSearchParams, TrackedModEntry,
};
use mcw_runtime::{
    detect_preset, detect_total_memory_mb, find_latest_crash_report, get_preset_flags,
    recommended_modpack_ram_mb, DiagnosticsExporter, DiscordRpcService, ExitClassification,
    FirstRunRecommendation, FirstRunRecommendationService, GameWindowActivator, JavaDiagnostic,
    JavaDiagnosticsManager, JavaDiscovery, JavaInstallation, JavaProvisioner,
    JavaRecoveryDiagnostics, JvmPresetId, LanAgentManager, LaunchErrorPresenter, LogLine,
    LogStream, McLogsClient, ProcessControl, SupervisedProcess, DEFAULT_DISCORD_CLIENT_ID,
};
use mcw_storage::{
    BackupInfo, BackupManager, CachedVersionInfo, ContentStore, ContentStoreStats, HealthSeverity,
    InstanceHealthManager, InstanceHealthReport, InstanceHealthState, InstanceRunLock,
    InstanceStore, LauncherPaths, StartupRecoveryService, StorageCleanupService, StorageSummary,
    UnusedVersionCandidate,
};
use mcw_updater::{ReleaseInfo, UpdateChannel, UpdateCheckResult, UpdateChecker, UpdateInstaller};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_LOG_LINES_PER_INSTANCE: usize = 1500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppPage {
    Home,
    Servers,
    Versions,
    Logs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailTab {
    Mods,
    Worlds,
    ResourcePacks,
    Shaders,
    Screenshots,
    Backups,
    Logs,
    Settings,
}

impl DetailTab {
    pub(crate) fn content_kind(self) -> Option<ContentKind> {
        match self {
            Self::ResourcePacks => Some(ContentKind::ResourcePack),
            Self::Shaders => Some(ContentKind::ShaderPack),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreateInstanceTab {
    Custom,
    ImportArchive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LauncherSettingsTab {
    General,
    Downloads,
    Runtime,
    Appearance,
    Storage,
    Updates,
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
    include_optional_files: bool,
    optional_files: Vec<ModpackOptionalFile>,
    selected_optional_ids: HashSet<String>,
    loading_optional_files: bool,
}

pub struct McwApp {
    paths: LauncherPaths,
    runtime: Arc<tokio::runtime::Runtime>,
    tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,

    theme_mode: ThemeMode,
    theme_catalog: ThemeCatalog,
    motion_mode: MotionMode,
    custom_accent_input: String,
    prefs: LauncherPrefs,
    lang: LanguageManager,
    palette: ThemePalette,
    discord_rpc: DiscordRpcService,

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
    installing_java_majors: HashSet<u32>,

    // UI state
    active_page: AppPage,
    active_tab: DetailTab,
    notice_banner: Option<(String, bool)>, // (message, is_error)
    backup_busy: bool,
    backup_note_input: String,
    pending_restore_backup_id: Option<String>,

    // Create instance modal (Custom | Modpacks + Import Archive)
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

    // Launcher Settings in-app modal
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

    // Unified Content Browser OS Window (Mods / Modpacks / Resource / Shader / Data packs)
    show_content_browser: bool,
    browser_category: BrowserCategory,
    browser_provider: BrowserProvider,
    browser_query: String,
    browser_sort: String,
    browser_loader_filter: String,
    browser_searching: bool,
    browser_request_id: u64,
    browser_initialized_key: Option<String>,
    browser_selected_key: Option<String>,
    browser_mr_results: Vec<ModrinthSearchHit>,
    browser_cf_results: Vec<CurseForgeMod>,
    browser_ftb_results: Vec<FtbPackSummary>,
    browser_atl_results: Vec<AtLauncherPack>,
    browser_datapack_world: String,

    // v0.12 Pages state (Servers, Versions, Logs)
    selected_servers: Vec<SavedServer>,
    servers_cache: HashMap<String, Vec<SavedServer>>,
    loading_servers: HashSet<String>,
    add_server_name: String,
    add_server_ip: String,
    cached_versions: Vec<CachedVersionInfo>,
    cached_versions_loading: bool,
    versions_search: String,
    versions_type_filter: String,
    logs_page_source: String,
    logs_page_filter: String,

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

    // Group & Tags centered OS window
    show_group_tags_modal: bool,

    // Stage 3: Account 2D Avatars, Auto-Updater, First-Run Wizard & Storage Inspector
    account_avatars: HashMap<String, egui::TextureHandle>,
    account_avatars_loading: HashSet<String>,
    update_checking: bool,
    update_downloading: bool,
    update_status_text: String,
    last_update_check: Option<UpdateCheckResult>,
    show_update_dialog: bool,
    show_first_run_wizard: bool,
    first_run_scanning: bool,
    first_run_rec: Option<FirstRunRecommendation>,
    first_run_min_ram: u32,
    first_run_max_ram: u32,
    first_run_language: String,
    first_run_auto_updates: bool,
    first_run_discord_rpc: bool,
    storage_scanning: bool,
    storage_cleaning: bool,
    storage_summary: Option<StorageSummary>,
    storage_unused_versions: Vec<UnusedVersionCandidate>,

    // Stage 4: Startup recovery, Mod compatibility, Repair Center, Global Logs, About & Window tracking
    global_logs: VecDeque<String>,
    global_logs_filter: String,
    show_global_logs_window: bool,
    show_about_window: bool,
    show_repair_center_window: bool,
    repair_center_report: Option<InstanceHealthReport>,
    mod_compat_report: Option<ModCompatibilityReport>,
    account_integrity_ok: bool,
    cf_api_key_input: String,
    cf_api_key_revealed: bool,
    last_window_size: [f32; 2],

    // Stage 5: Java Diagnostics, LAN Agent Log dialog, Sidebar Context Menu,
    // Optional modpack files, Shared Content Store deduplication & OptiFine tracking
    java_diagnostics: Vec<JavaDiagnostic>,
    java_scanning: bool,
    show_lan_agent_log_window: bool,
    lan_agent_log_instance_id: String,
    lan_agent_log_text: String,
    sidebar_context_menu: Option<(String, egui::Pos2)>,
    archive_optional_files: Vec<ModpackOptionalFile>,
    archive_selected_optional_ids: HashSet<String>,
    archive_include_optional: bool,
    content_store_stats: Option<ContentStoreStats>,
    dedup_running: bool,
    optifine_installing: bool,

    // v0.11 shell: background picture, icon rail flyout, in-app Settings modal, page fade
    background: BackgroundManager,
    show_instance_flyout: bool,
    flyout_toggle_rect: Option<egui::Rect>,
    settings_search: String,
    settings_open_since: Option<Instant>,
    fade_page: AppPage,
    fade_tab: DetailTab,
    fade_since: Instant,

    // v0.13 polish: global Ctrl+K quick search / command palette
    show_quick_search: bool,
    quick_search_query: String,
    quick_search_open_since: Option<Instant>,
    quick_search_focus_pending: bool,
}

impl McwApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: LauncherPaths) -> Self {
        configure_fonts(&cc.egui_ctx);
        let prefs = LauncherPrefs::load(&paths);
        mcw_net::set_global_bandwidth_limit_mbps(prefs.download_limit_mbps);
        let cf_api_key_input =
            load_custom_curseforge_api_key(&paths.config_root).unwrap_or_default();
        let account_integrity_ok = AccountStore::new(&paths).verify_integrity();
        let last_window_size = [
            prefs.window_width.clamp(840.0, 3840.0),
            prefs.window_height.clamp(560.0, 2160.0),
        ];
        let theme_catalog = ThemeCatalog::load(&paths.theme_root);
        let motion_mode = MotionMode::parse(&prefs.motion_mode);
        let custom_accent_input = prefs.custom_accent_hex.clone();
        let active_def = theme_catalog.find(&prefs.theme).clone();
        let theme_mode = active_def.base_mode;
        let mut active_pal = active_def.palette;
        if !custom_accent_input.trim().is_empty() {
            let _ = apply_custom_accent(&mut active_pal, &custom_accent_input);
        }
        let lang = LanguageManager::load(&paths, &prefs.normalized_locale());
        let palette = apply_theme_palette(&cc.egui_ctx, theme_mode, active_pal, motion_mode)
            .glass(prefs.panel_opacity);
        let init_show_snapshots = prefs.show_snapshots;
        let first_run_language = prefs.normalized_locale();
        let first_run_auto_updates = prefs.auto_check_updates;
        let first_run_discord_rpc = prefs.discord_rpc_enabled;
        let discord_rpc =
            DiscordRpcService::new(prefs.discord_rpc_enabled, prefs.discord_client_id.clone());

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
            theme_catalog,
            motion_mode,
            custom_accent_input,
            prefs,
            lang,
            palette,
            discord_rpc,
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
            installing_java_majors: HashSet::new(),
            active_page: AppPage::Home,
            active_tab: DetailTab::Mods,
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
            show_content_browser: false,
            browser_category: BrowserCategory::Mods,
            browser_provider: BrowserProvider::Modrinth,
            browser_query: String::new(),
            browser_sort: "relevance".to_string(),
            browser_loader_filter: "fabric".to_string(),
            browser_searching: false,
            browser_request_id: 0,
            browser_initialized_key: None,
            browser_selected_key: None,
            browser_mr_results: Vec::new(),
            browser_cf_results: Vec::new(),
            browser_ftb_results: Vec::new(),
            browser_atl_results: Vec::new(),
            browser_datapack_world: String::new(),
            selected_servers: Vec::new(),
            servers_cache: HashMap::new(),
            loading_servers: HashSet::new(),
            add_server_name: String::new(),
            add_server_ip: String::new(),
            cached_versions: Vec::new(),
            cached_versions_loading: false,
            versions_search: String::new(),
            versions_type_filter: "release".to_string(),
            logs_page_source: "__global__".to_string(),
            logs_page_filter: String::new(),
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
            show_group_tags_modal: false,
            account_avatars: HashMap::new(),
            account_avatars_loading: HashSet::new(),
            update_checking: false,
            update_downloading: false,
            update_status_text: if mcw_updater::UPDATES_ENABLED {
                "Not checked yet".to_string()
            } else {
                "Updates are currently disabled for MCW Launcher: Rust Edition.".to_string()
            },
            last_update_check: None,
            show_update_dialog: false,
            show_first_run_wizard: false,
            first_run_scanning: false,
            first_run_rec: None,
            first_run_min_ram: 1024,
            first_run_max_ram: 2048,
            first_run_language,
            first_run_auto_updates,
            first_run_discord_rpc,
            storage_scanning: false,
            storage_cleaning: false,
            storage_summary: None,
            storage_unused_versions: Vec::new(),
            global_logs: VecDeque::with_capacity(256),
            global_logs_filter: String::new(),
            show_global_logs_window: false,
            show_about_window: false,
            show_repair_center_window: false,
            repair_center_report: None,
            mod_compat_report: None,
            account_integrity_ok,
            cf_api_key_input,
            cf_api_key_revealed: false,
            last_window_size,
            java_diagnostics: Vec::new(),
            java_scanning: false,
            show_lan_agent_log_window: false,
            lan_agent_log_instance_id: String::new(),
            lan_agent_log_text: String::new(),
            sidebar_context_menu: None,
            archive_optional_files: Vec::new(),
            archive_selected_optional_ids: HashSet::new(),
            archive_include_optional: true,
            content_store_stats: None,
            dedup_running: false,
            optifine_installing: false,
            background: BackgroundManager::new(),
            show_instance_flyout: false,
            flyout_toggle_rect: None,
            settings_search: String::new(),
            settings_open_since: None,
            fade_page: AppPage::Home,
            fade_tab: DetailTab::Mods,
            fade_since: Instant::now(),
            show_quick_search: false,
            quick_search_query: String::new(),
            quick_search_open_since: None,
            quick_search_focus_pending: false,
        };

        app.content_store_stats = Some(ContentStore::stats(&app.paths));

        app.push_global_log(format!(
            "MCW Launcher: Rust Edition v{} started.",
            mcw_models::APP_VERSION
        ));
        let rec = StartupRecoveryService::reconcile(&app.paths);
        if rec.recovered_count() > 0 {
            app.push_global_log(format!(
                "[INFO] Startup recovery reconciled {} stale lock(s) and {} orphan temp file(s).",
                rec.stale_locks.len(),
                rec.orphan_temp_files
            ));
        }
        if !app.account_integrity_ok {
            app.push_global_log(
                "[WARN] Account store HMAC integrity signature mismatch detected.".to_string(),
            );
        }

        app.reload_instances();
        app.reload_accounts();
        app.reload_java();
        app.start_java_diagnostics_scan(&cc.egui_ctx);
        app.fetch_versions(&cc.egui_ctx);
        app.ensure_account_avatars_loaded(&cc.egui_ctx);

        if !app.prefs.first_run_completed && app.instances.is_empty() {
            app.open_first_run_wizard(&cc.egui_ctx);
        }
        if mcw_updater::UPDATES_ENABLED && app.prefs.auto_check_updates {
            app.start_update_check(false, &cc.egui_ctx);
        }
        app
    }
}

impl Drop for McwApp {
    fn drop(&mut self) {
        if self.prefs.remember_window_size
            && self.last_window_size[0] >= 840.0
            && self.last_window_size[1] >= 560.0
        {
            self.prefs.window_width = self.last_window_size[0];
            self.prefs.window_height = self.last_window_size[1];
            self.save_prefs();
        }
        self.discord_rpc.shutdown();
    }
}

impl eframe::App for McwApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            let size = rect.size();
            if size.x >= 840.0 && size.y >= 560.0 {
                self.last_window_size = [size.x, size.y];
            }
        }
        self.poll_events(ctx);
        self.ensure_account_avatars_loaded(ctx);
        self.handle_dropped_files(ctx);

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

        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
            self.show_quick_search = !self.show_quick_search;
            if self.show_quick_search {
                self.quick_search_query.clear();
                self.quick_search_open_since = Some(Instant::now());
                self.quick_search_focus_pending = true;
                self.show_instance_flyout = false;
            }
        }
        if !self.show_launcher_settings {
            self.settings_open_since = None;
        }
        if !self.show_quick_search {
            self.quick_search_open_since = None;
        }
        self.paint_background(ctx);
        self.render_top_bar(ctx);
        self.render_tasks_bar(ctx);
        self.render_instance_rail(ctx);
        self.render_active_page(ctx);
        self.render_instance_flyout(ctx);
        self.render_modals(ctx);
        self.render_quick_search_modal(ctx);
    }
}
