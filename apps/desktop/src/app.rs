mod controllers;
mod controllers_pages;
pub(crate) mod platform;
mod views;

pub(crate) use controllers::{
    format_modlist_export, matches_log_level, matches_mod_filter, sort_instances_for_grid,
};

use self::platform::default_user_downloads_dir;
use crate::background::{decode_user_image, BackgroundManager};
use crate::i18n::{LanguageManager, LauncherPrefs};
use crate::skin_render::{default_skin_face_rgba, render_skin_face_rgba};
use crate::state::{
    ActiveTask, AppEvent, BrowserCategory, BrowserProvider, CartModProvider, CartResolvedEntry,
    ChannelLogSink, ChannelProgressSink, ModCartItem, ModVersionChoice, ModpackChannel,
    ModpackVersionOption,
};
use crate::theme::{
    apply_custom_accent, apply_theme_palette, configure_fonts_with_spec, format_hex_color,
    parse_hex_color, resolve_custom_font_path, scan_system_fonts, MotionMode, SystemFontEntry,
    ThemeCatalog, ThemeMode, ThemePalette,
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
    is_loader_compatible, ContentKind, ContentPackInfo, ContentPackManager, InstalledContentItem,
    InstalledContentLibrary, ModCompatibilityAnalyzer, ModCompatibilityReport, ModInfo,
    ModIssueSeverity, ModManager, ModScanner,
};
use mcw_net::{ConnectivityMonitor, ConnectivityStatus, DownloadClient, TaskControl};
use mcw_providers::{
    detect_export_origin, detect_installed_pack, is_verified_or_imported_modpack,
    load_curseforge_api_key, load_custom_curseforge_api_key, save_custom_curseforge_api_key,
    ApiCache, AtLauncherClient, AtLauncherPack, ContentPackUpdateCandidate, CurseForgeClient,
    CurseForgeMod, CurseForgePackRegistry, CurseForgeSearchParams, ExportTreeEntry, FtbClient,
    FtbPackSummary, InstanceExportOrigin, ManualDownloadItem, ModProvenanceRegistry,
    ModUpdateCandidate, ModUpdateManager, ModpackExportMode, ModpackExportOptions,
    ModpackInstallOptions, ModpackInstaller, ModpackOptionalFile, ModpackProgressCallback,
    ModpackUpdateCheck, ModpackUpdateService, ModrinthClient, ModrinthPackRegistry,
    ModrinthSearchHit, ModrinthSearchParams, TechnicClient, TechnicModpackSummary, TrackedModEntry,
};
use mcw_runtime::{
    analyze_crash_cause, detect_preset, detect_total_memory_mb, find_latest_crash_report,
    get_preset_flags, recommended_modpack_ram_mb, run_lifecycle_hook, CrashCauseSummary,
    DiagnosticsExporter, DiscordRpcService, ExitClassification, FirstRunRecommendation,
    FirstRunRecommendationService, GameWindowActivator, IssueReportBuilder, IssueReportInput,
    JavaDiagnostic, JavaDiagnosticsManager, JavaDiscovery, JavaInstallation, JavaProvisioner,
    JavaRecoveryDiagnostics, JvmPresetId, LanAgentManager, LaunchErrorPresenter, LogLine,
    LogStream, McLogsClient, ProcessControl, SupervisedProcess, DEFAULT_DISCORD_CLIENT_ID,
};
use mcw_storage::{
    default_root_candidates_for_kind, detect_default_launcher_roots, import_external_instances,
    inspect_external_launcher, inspect_python_launcher, migrate_from_python_launcher, BackupInfo,
    BackupManager, CachedVersionInfo, ConflictStrategy, ContentStore, ContentStoreStats,
    DiscoveredExternalInstance, ExternalLauncherKind, HealthSeverity, InstanceHealthManager,
    InstanceHealthReport, InstanceHealthState, InstanceRunLock, InstanceStore, JunkScanReport,
    LauncherPaths, PythonMigrationInspection, PythonMigrationOptions, PythonMigrationResult,
    StartupRecoveryService, StorageCleanupService, StorageSummary, UnusedVersionCandidate,
};
use mcw_updater::{ReleaseInfo, UpdateChannel, UpdateCheckResult, UpdateChecker, UpdateInstaller};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_LOG_LINES_PER_INSTANCE: usize = 1500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupLaunchRequest {
    pub instance_id: String,
    pub quick_play_world: Option<String>,
    pub quick_play_server: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct StorageRelocationPrompt {
    pub(crate) old_root: PathBuf,
    pub(crate) new_root: PathBuf,
    pub(crate) has_user_data: bool,
    pub(crate) opened_at: Instant,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppPage {
    Home,
    Modpacks,
    Versions,
    Logs,
    Changelog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NavLocation {
    pub(crate) page: AppPage,
    pub(crate) instance_id: Option<String>,
    pub(crate) tab: DetailTab,
    pub(crate) browser_category: BrowserCategory,
    pub(crate) inline_browser: Option<BrowserCategory>,
}

#[derive(Debug, Clone)]
pub(crate) enum DuplicateModSource {
    Remote {
        choice: Box<ModVersionChoice>,
        provider: BrowserProvider,
        project_id: Option<String>,
        project_name: Option<String>,
    },
    LocalFile {
        source_path: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct PendingDuplicateModPrompt {
    pub(crate) instance_id: String,
    pub(crate) instance_name: String,
    pub(crate) incoming_file_name: String,
    pub(crate) existing_file_name: String,
    pub(crate) match_reason: String,
    pub(crate) source: DuplicateModSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DetailTab {
    Mods,
    Worlds,
    Servers,
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
    Technic { slug: String },
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
    jvm_preset: JvmPresetId,
    include_optional_files: bool,
    optional_files: Vec<ModpackOptionalFile>,
    selected_optional_ids: HashSet<String>,
    loading_optional_files: bool,
    enable_lan_agent: bool,
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
    export_disable_repair_and_updates: bool,
    export_disable_launch_checking: bool,
    export_tree_entries: Vec<ExportTreeEntry>,
    export_checked_paths: HashSet<String>,
    export_tree_expanded_dirs: HashSet<String>,
    export_tree_key: Option<(String, String, bool)>,
    last_connectivity_check_at: Instant,
    changelog_markdown: String,
    changelog_refreshing: bool,
    relocated_storage_prompt: Option<StorageRelocationPrompt>,
    relocating_storage_busy: bool,

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

    // Instance runtime/metadata/arguments edit buffers
    edit_runtime_version: String,
    edit_runtime_loader: String,
    edit_runtime_show_snapshots: bool,
    edit_group_input: String,
    edit_tags_input: String,
    edit_jvm_args_text: String,
    edit_game_args_text: String,
    edit_env_vars_text: String,

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
    browser_technic_results: Vec<TechnicModpackSummary>,
    browser_datapack_world: String,
    browser_global_mode: bool,
    browser_mc_version_filter: String,
    browser_page: u32,
    browser_total_hits: u32,

    // v0.12 / v0.13.1 Pages & Instance Servers tab state
    selected_servers: Vec<SavedServer>,
    servers_cache: HashMap<String, Vec<SavedServer>>,
    loading_servers: HashSet<String>,
    add_server_name: String,
    add_server_ip: String,
    cached_versions: Vec<CachedVersionInfo>,
    cached_versions_loading: bool,
    versions_cached_expanded: bool,
    versions_search: String,
    versions_type_filter: String,
    logs_page_source: String,
    logs_page_filter: String,
    logs_level_filter: String,
    logs_tab_search: String,
    crash_summaries: HashMap<String, CrashCauseSummary>,

    // Async icon thumbnail texture cache for Mods, Packs, and Modpacks
    thumb_textures: HashMap<String, egui::TextureHandle>,
    loading_thumbs: HashSet<String>,
    thumb_lru_order: Vec<String>,

    // Local instance icon texture cache (keyed by "<instance_id>|<icon>")
    instance_icon_textures: HashMap<String, Option<egui::TextureHandle>>,
    world_icon_textures: HashMap<PathBuf, Option<egui::TextureHandle>>,
    pack_icon_textures: HashMap<PathBuf, Option<egui::TextureHandle>>,
    checking_pack_updates: bool,
    pack_update_candidates: Vec<ContentPackUpdateCandidate>,
    pack_updates_kind: Option<ContentKind>,
    screenshot_thumbs: HashMap<PathBuf, Option<egui::TextureHandle>>,
    screenshot_thumbs_loading: HashSet<PathBuf>,
    screenshot_thumb_lru: Vec<PathBuf>,
    screenshot_preview_path: Option<PathBuf>,
    screenshot_preview_texture: Option<(PathBuf, egui::TextureHandle)>,
    screenshot_preview_rename_input: String,
    edit_instance_notes_text: String,
    instance_notes_baseline: String,
    mod_search_query: String,
    mod_status_filter: String,
    selected_mod_files: HashSet<String>,
    collapsed_home_groups: HashSet<String>,
    home_grid_search: String,
    mod_cart: Vec<ModCartItem>,
    show_mod_cart_modal: bool,
    mod_cart_resolving: bool,
    mod_cart_request_id: u64,
    mod_cart_preview: Vec<CartResolvedEntry>,
    mod_cart_target_instance_id: Option<String>,

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
    version_picker_global_mode: bool,
    version_picker_content_kind: Option<ContentKind>,

    // Step 2 Install Target OS Window (opened when clicking Install in global Version Picker)
    install_target_open: bool,
    install_target_title: String,
    install_target_icon: Option<String>,
    install_target_choice: Option<ModVersionChoice>,
    install_target_content_kind: Option<ContentKind>,
    install_target_create_new: bool,
    install_target_instance_id: String,
    install_target_new_name: String,
    install_target_new_mc_version: String,
    install_target_new_loader: String,
    install_target_datapack_world: String,

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
    first_run_page: usize,
    first_run_scanning: bool,
    first_run_rec: Option<FirstRunRecommendation>,
    first_run_min_ram: u32,
    first_run_max_ram: u32,
    first_run_language: String,
    first_run_auto_updates: bool,
    first_run_discord_rpc: bool,
    first_run_download_concurrency: u32,
    first_run_java_path: String,
    first_run_prefer_dedicated_gpu: bool,
    first_run_offline_username: String,
    first_run_offline_msg: Option<(String, bool)>,
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

    // Stage 5 / v0.13.2: Java Diagnostics OS Window & Latest Adoptium Release
    java_diagnostics: Vec<JavaDiagnostic>,
    java_scanning: bool,
    show_java_manager_window: bool,
    latest_java_major: Option<u32>,
    latest_java_checking: bool,
    latest_java_error: Option<String>,
    java_install_selected_major: u32,
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

    // v0.11+ shell: async background picture, in-app Settings modal, page fade
    background: BackgroundManager,
    bg_picker_open: bool,
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

    // v0.14 full Python parity: Connectivity, Content Library, Modpack Update, Preflight Confirm,
    // Issue Report, Theme Issues & Unsaved Instance Settings Guard
    connectivity_status: ConnectivityStatus,
    connectivity_checking: bool,
    show_content_library_window: bool,
    content_library_loading: bool,
    content_library_items: Vec<(String, String, InstalledContentItem)>,
    content_library_search: String,
    content_library_kind_filter: String,
    content_library_inst_filter: String,
    content_library_pinned_only: bool,
    show_modpack_update_window: bool,
    modpack_update_instance_id: String,
    modpack_update_loading: bool,
    modpack_update_check: Option<ModpackUpdateCheck>,
    modpack_update_selected_version: String,
    modpack_update_include_beta: bool,
    modpack_update_include_alpha: bool,
    modpack_update_error: Option<String>,
    preflight_confirm_instance_id: Option<String>,
    preflight_confirm_world: Option<String>,
    preflight_confirm_server: Option<String>,
    preflight_confirm_issues: Vec<String>,
    show_issue_report_window: bool,
    issue_report_summary: String,
    issue_report_markdown: String,
    show_theme_issues_window: bool,
    instance_settings_dirty: bool,
    pending_tab_switch: Option<DetailTab>,
    pending_instance_switch: Option<String>,
    show_unsaved_changes_window: bool,

    // v0.15: Window sizing stability, custom font selector & Python launcher importer
    window_ready_at: Instant,
    settled_window_size: Option<[f32; 2]>,
    system_fonts: Vec<SystemFontEntry>,
    show_python_import_window: bool,
    python_import_path_input: String,
    python_import_folder_picking: bool,
    python_import_inspecting: bool,
    python_import_inspection: Option<PythonMigrationInspection>,
    python_import_error: Option<String>,
    python_import_selected: HashSet<String>,
    python_import_conflict: ConflictStrategy,
    python_import_global_settings: bool,
    python_import_runtimes: bool,
    python_import_themes: bool,
    python_import_running: bool,
    python_import_result: Option<PythonMigrationResult>,
    show_universal_import_modal: bool,
    universal_import_kind: ExternalLauncherKind,
    universal_import_path: String,
    universal_import_folder_picking: bool,
    universal_import_scanning: bool,
    universal_import_importing: bool,
    universal_import_items: Vec<DiscoveredExternalInstance>,
    universal_import_selected: HashSet<String>,
    universal_import_error: Option<String>,

    // v1.0.0-rc.1: Branding logo, navigation history, scale apply, LAN agent creation toggles, splash screen
    app_logo_texture: Option<egui::TextureHandle>,
    nav_history: Vec<NavLocation>,
    nav_history_idx: usize,
    nav_suppress_record: bool,
    pending_ui_scale_pct: f32,
    pending_font_family: String,
    pending_font_size: f32,
    show_color_picker_modal: bool,
    color_picker_rgb: [u8; 3],
    color_picker_hex_input: String,
    create_enable_lan_agent: bool,
    archive_enable_lan_agent: bool,
    install_target_enable_lan_agent: bool,
    startup_splash_started_at: Instant,
    startup_splash_done: bool,
    instance_inline_browser: Option<BrowserCategory>,
    pending_duplicate_mod_prompts: Vec<PendingDuplicateModPrompt>,
    modpack_update_confirm_candidate: Option<mcw_providers::ModpackUpdateCandidate>,
    pending_backup_before_modpack_update: Option<(String, mcw_providers::ModpackUpdateCandidate)>,

    // v1.1.1: Sticky tab counts per instance + deduplicated task/launch Global Log tracking
    revealed_tab_counts: HashMap<DetailTab, usize>,
    revealed_tab_counts_instance: Option<String>,
    last_task_log_msg: HashMap<u64, String>,
    last_launch_log_msg: HashMap<String, String>,

    // v1.2.0: Shared download client, auto junk cleanup reminder, real-time folder watcher
    pub(crate) download_client: DownloadClient,
    show_junk_cleanup_modal: bool,
    junk_scan_report: Option<JunkScanReport>,
    junk_clean_temp: bool,
    junk_clean_logs: bool,
    junk_clean_caches: bool,
    junk_scanning: bool,
    junk_cleaning: bool,
    folder_watcher: Option<RecommendedWatcher>,
    watched_folder_path: Option<PathBuf>,
}

impl McwApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        paths: LauncherPaths,
        init_size: [f32; 2],
        relocated_from: Option<(PathBuf, bool)>,
        startup_launch: Option<StartupLaunchRequest>,
    ) -> Self {
        let mut prefs = LauncherPrefs::load(&paths);
        if prefs.window_size_schema_version < 2 {
            prefs.window_size_schema_version = 2;
            prefs.window_size_customized = false;
            prefs.window_width = init_size[0];
            prefs.window_height = init_size[1];
            prefs.save(&paths);
        }
        let system_fonts = scan_system_fonts();
        let custom_font_path = resolve_custom_font_path(&prefs.custom_font_family, &system_fonts);
        configure_fonts_with_spec(
            &cc.egui_ctx,
            &prefs.custom_font_family,
            custom_font_path.as_deref(),
        );
        cc.egui_ctx
            .set_zoom_factor(prefs.ui_scale.clamp(0.75, 1.75));
        let pending_ui_scale_pct = (prefs.ui_scale.clamp(0.75, 1.75) * 100.0).round();
        let pending_font_family = prefs.custom_font_family.clone();
        let pending_font_size = prefs.font_size.clamp(10.0, 22.0);
        let app_logo_texture = load_app_logo_texture(&cc.egui_ctx);

        mcw_net::set_global_bandwidth_limit_mbps(prefs.download_limit_mbps);
        mcw_net::set_global_segments_per_file(prefs.download_segments_per_file);
        mcw_net::set_global_proxy_config(mcw_net::ProxyConfig {
            mode: prefs.proxy_mode.clone(),
            host: prefs.proxy_host.clone(),
            port: prefs.proxy_port,
            username: prefs.proxy_username.clone(),
            password: prefs.proxy_password.clone(),
        });
        mcw_net::init_persistent_verification_cache(
            &paths.cache_root.join("verification_cache.json"),
        );
        let download_client = DownloadClient::new()
            .unwrap_or_else(|e| panic!("Failed to initialize HTTP download client: {e}"));
        let cf_api_key_input =
            load_custom_curseforge_api_key(&paths.config_root).unwrap_or_default();
        let relocated_storage_prompt =
            relocated_from.map(|(old_root, has_user_data)| StorageRelocationPrompt {
                old_root,
                new_root: paths.project_root.clone(),
                has_user_data,
                opened_at: Instant::now(),
                error: None,
            });
        let account_integrity_ok = AccountStore::new(&paths).verify_integrity();
        let last_window_size = init_size;
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
        let palette = apply_theme_palette(
            &cc.egui_ctx,
            theme_mode,
            active_pal,
            motion_mode,
            prefs.font_size,
        )
        .glass(prefs.panel_opacity);
        let init_show_snapshots = prefs.show_snapshots;
        let first_run_language = prefs.normalized_locale();
        let first_run_auto_updates = prefs.auto_check_updates;
        let first_run_discord_rpc = prefs.discord_rpc_enabled;
        let first_run_min_ram = prefs.default_min_memory;
        let first_run_max_ram = prefs.default_max_memory;
        let first_run_download_concurrency = prefs.download_concurrency;
        let first_run_java_path = prefs.default_java_path.clone();
        let first_run_prefer_dedicated_gpu = prefs.prefer_dedicated_gpu;
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
            export_disable_repair_and_updates: true,
            export_disable_launch_checking: true,
            export_tree_entries: Vec::new(),
            export_checked_paths: HashSet::new(),
            export_tree_expanded_dirs: HashSet::new(),
            export_tree_key: None,
            last_connectivity_check_at: Instant::now(),
            changelog_markdown: include_str!("../../../CHANGELOG.md").to_string(),
            changelog_refreshing: false,
            relocated_storage_prompt,
            relocating_storage_busy: false,
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
            edit_runtime_version: String::new(),
            edit_runtime_loader: "vanilla".to_string(),
            edit_runtime_show_snapshots: false,
            edit_group_input: String::new(),
            edit_tags_input: String::new(),
            edit_jvm_args_text: String::new(),
            edit_game_args_text: String::new(),
            edit_env_vars_text: String::new(),
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
            browser_loader_filter: "all".to_string(),
            browser_searching: false,
            browser_request_id: 0,
            browser_initialized_key: None,
            browser_selected_key: None,
            browser_mr_results: Vec::new(),
            browser_cf_results: Vec::new(),
            browser_ftb_results: Vec::new(),
            browser_atl_results: Vec::new(),
            browser_technic_results: Vec::new(),
            browser_datapack_world: String::new(),
            browser_global_mode: true,
            browser_mc_version_filter: String::new(),
            browser_page: 0,
            browser_total_hits: 0,
            selected_servers: Vec::new(),
            servers_cache: HashMap::new(),
            loading_servers: HashSet::new(),
            add_server_name: String::new(),
            add_server_ip: String::new(),
            cached_versions: Vec::new(),
            cached_versions_loading: false,
            versions_cached_expanded: false,
            versions_search: String::new(),
            versions_type_filter: "release".to_string(),
            logs_page_source: "__global__".to_string(),
            logs_page_filter: String::new(),
            logs_level_filter: "all".to_string(),
            logs_tab_search: String::new(),
            crash_summaries: HashMap::new(),
            thumb_textures: HashMap::new(),
            loading_thumbs: HashSet::new(),
            thumb_lru_order: Vec::new(),
            instance_inline_browser: None,
            pending_duplicate_mod_prompts: Vec::new(),
            modpack_update_confirm_candidate: None,
            pending_backup_before_modpack_update: None,
            instance_icon_textures: HashMap::new(),
            world_icon_textures: HashMap::new(),
            pack_icon_textures: HashMap::new(),
            checking_pack_updates: false,
            pack_update_candidates: Vec::new(),
            pack_updates_kind: None,
            screenshot_thumbs: HashMap::new(),
            screenshot_thumbs_loading: HashSet::new(),
            screenshot_thumb_lru: Vec::new(),
            screenshot_preview_path: None,
            screenshot_preview_texture: None,
            screenshot_preview_rename_input: String::new(),
            edit_instance_notes_text: String::new(),
            instance_notes_baseline: String::new(),
            mod_search_query: String::new(),
            mod_status_filter: "all".to_string(),
            selected_mod_files: HashSet::new(),
            collapsed_home_groups: HashSet::new(),
            home_grid_search: String::new(),
            mod_cart: Vec::new(),
            show_mod_cart_modal: false,
            mod_cart_resolving: false,
            mod_cart_request_id: 0,
            mod_cart_preview: Vec::new(),
            mod_cart_target_instance_id: None,
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
            version_picker_global_mode: false,
            version_picker_content_kind: None,
            install_target_open: false,
            install_target_title: String::new(),
            install_target_icon: None,
            install_target_choice: None,
            install_target_content_kind: None,
            install_target_create_new: false,
            install_target_instance_id: String::new(),
            install_target_new_name: String::new(),
            install_target_new_mc_version: String::new(),
            install_target_new_loader: "fabric".to_string(),
            install_target_datapack_world: String::new(),
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
            first_run_page: 0,
            first_run_scanning: false,
            first_run_rec: None,
            first_run_min_ram,
            first_run_max_ram,
            first_run_language,
            first_run_auto_updates,
            first_run_discord_rpc,
            first_run_download_concurrency,
            first_run_java_path,
            first_run_prefer_dedicated_gpu,
            first_run_offline_username: String::new(),
            first_run_offline_msg: None,
            storage_scanning: false,
            storage_cleaning: false,
            storage_summary: None,
            storage_unused_versions: Vec::new(),
            global_logs: VecDeque::with_capacity(2000),
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
            show_java_manager_window: false,
            latest_java_major: None,
            latest_java_checking: false,
            latest_java_error: None,
            java_install_selected_major: 21,
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
            bg_picker_open: false,
            settings_search: String::new(),
            settings_open_since: None,
            fade_page: AppPage::Home,
            fade_tab: DetailTab::Mods,
            fade_since: Instant::now(),
            show_quick_search: false,
            quick_search_query: String::new(),
            quick_search_open_since: None,
            quick_search_focus_pending: false,
            connectivity_status: ConnectivityStatus::Unknown,
            connectivity_checking: false,
            show_content_library_window: false,
            content_library_loading: false,
            content_library_items: Vec::new(),
            content_library_search: String::new(),
            content_library_kind_filter: "all".to_string(),
            content_library_inst_filter: "all".to_string(),
            content_library_pinned_only: false,
            show_modpack_update_window: false,
            modpack_update_instance_id: String::new(),
            modpack_update_loading: false,
            modpack_update_check: None,
            modpack_update_selected_version: String::new(),
            modpack_update_include_beta: false,
            modpack_update_include_alpha: false,
            modpack_update_error: None,
            preflight_confirm_instance_id: None,
            preflight_confirm_world: None,
            preflight_confirm_server: None,
            preflight_confirm_issues: Vec::new(),
            show_issue_report_window: false,
            issue_report_summary: String::new(),
            issue_report_markdown: String::new(),
            show_theme_issues_window: false,
            instance_settings_dirty: false,
            pending_tab_switch: None,
            pending_instance_switch: None,
            show_unsaved_changes_window: false,
            window_ready_at: Instant::now(),
            settled_window_size: None,
            system_fonts,
            show_python_import_window: false,
            python_import_path_input: String::new(),
            python_import_folder_picking: false,
            python_import_inspecting: false,
            python_import_inspection: None,
            python_import_error: None,
            python_import_selected: HashSet::new(),
            python_import_conflict: ConflictStrategy::Skip,
            python_import_global_settings: true,
            python_import_runtimes: true,
            python_import_themes: true,
            python_import_running: false,
            python_import_result: None,
            show_universal_import_modal: false,
            universal_import_kind: ExternalLauncherKind::PrismLauncher,
            universal_import_path: String::new(),
            universal_import_folder_picking: false,
            universal_import_scanning: false,
            universal_import_importing: false,
            universal_import_items: Vec::new(),
            universal_import_selected: HashSet::new(),
            universal_import_error: None,
            app_logo_texture,
            nav_history: Vec::new(),
            nav_history_idx: 0,
            nav_suppress_record: false,
            pending_ui_scale_pct,
            pending_font_family,
            pending_font_size,
            show_color_picker_modal: false,
            color_picker_rgb: [139, 92, 246],
            color_picker_hex_input: "#8B5CF6".to_string(),
            create_enable_lan_agent: false,
            archive_enable_lan_agent: false,
            install_target_enable_lan_agent: false,
            startup_splash_started_at: Instant::now(),
            startup_splash_done: motion_mode == MotionMode::Off,
            revealed_tab_counts: HashMap::new(),
            revealed_tab_counts_instance: None,
            last_task_log_msg: HashMap::new(),
            last_launch_log_msg: HashMap::new(),
            download_client,
            show_junk_cleanup_modal: false,
            junk_scan_report: None,
            junk_clean_temp: true,
            junk_clean_logs: true,
            junk_clean_caches: true,
            junk_scanning: false,
            junk_cleaning: false,
            folder_watcher: None,
            watched_folder_path: None,
        };

        app.content_store_stats = Some(ContentStore::stats(&app.paths));

        app.push_global_log(format!(
            "MCW Launcher: Rust Edition v{} started.",
            mcw_models::APP_VERSION
        ));
        if let Some(prompt) = &app.relocated_storage_prompt {
            app.push_global_log(format!(
                "[WARN] Launch folder {} is not writable; using safe storage at {}",
                prompt.old_root.display(),
                prompt.new_root.display()
            ));
        }
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
        app.nav_history.push(NavLocation {
            page: app.active_page,
            instance_id: app.selected_instance_id.clone(),
            tab: app.active_tab,
            browser_category: app.browser_category,
            inline_browser: app.instance_inline_browser,
        });
        app.reload_accounts();
        app.reload_java();
        app.start_java_diagnostics_scan(&cc.egui_ctx);
        app.fetch_versions(&cc.egui_ctx);
        app.start_connectivity_check(&cc.egui_ctx);
        app.ensure_account_avatars_loaded(&cc.egui_ctx);

        if !app.prefs.first_run_completed && app.instances.is_empty() {
            app.open_first_run_wizard(&cc.egui_ctx);
        }
        if mcw_updater::UPDATES_ENABLED && app.prefs.auto_check_updates {
            app.start_update_check(false, &cc.egui_ctx);
        }
        if app.prefs.auto_cleanup_reminders
            && crate::app::platform::unix_now_secs() >= app.prefs.junk_reminder_snooze_until
        {
            app.start_junk_scan(false, &cc.egui_ctx);
        }
        if let Some(req) = startup_launch {
            if app.relocated_storage_prompt.is_none() {
                if let Some(found) = app.instances.iter().find(|i| {
                    i.instance_id == req.instance_id
                        || i.name.eq_ignore_ascii_case(&req.instance_id)
                }) {
                    let target_id = found.instance_id.clone();
                    app.startup_splash_done = true;
                    app.selected_instance_id = Some(target_id);
                    app.sync_selected_from_cache();
                    app.start_launch_for_instance(
                        req.quick_play_world,
                        req.quick_play_server,
                        false,
                        &cc.egui_ctx,
                    );
                } else {
                    app.notice_banner = Some((
                        format!("Instance '{}' not found for --launch.", req.instance_id),
                        true,
                    ));
                }
            }
        }
        app
    }
}

impl Drop for McwApp {
    fn drop(&mut self) {
        if self.prefs.remember_window_size
            && self.prefs.window_size_customized
            && self.last_window_size[0] >= 840.0
            && self.last_window_size[1] >= 560.0
        {
            self.prefs.window_width = self.last_window_size[0];
            self.prefs.window_height = self.last_window_size[1];
            self.save_prefs();
        }
        mcw_net::flush_persistent_verification_cache();
        self.discord_rpc.shutdown();
    }
}

impl McwApp {
    pub(crate) fn has_open_in_app_modal(&self) -> bool {
        self.relocated_storage_prompt.is_some()
            || self.show_launcher_settings
            || self.show_quick_search
            || self.show_create_modal
            || self.show_export_modal
            || self.show_account_modal
            || self.show_rename_modal
            || self.show_group_tags_modal
            || self.show_clone_modal
            || self.show_delete_modal
            || self.pending_restore_backup_id.is_some()
            || self.pending_delete_world.is_some()
            || self.screenshot_preview_path.is_some()
            || self.version_picker_open
            || self.install_target_open
            || self.show_mod_cart_modal
            || self.modpack_dialog.open
            || !self.open_manual_modals.is_empty()
            || (self.show_all_tasks_modal && self.active_tasks.len() > 2)
            || !self.pending_duplicate_mod_prompts.is_empty()
            || self.show_update_dialog
            || self.show_first_run_wizard
            || self.show_repair_center_window
            || self.show_about_window
            || self.show_java_manager_window
            || self.show_content_library_window
            || self.show_modpack_update_window
            || self.preflight_confirm_instance_id.is_some()
            || self.show_issue_report_window
            || self.show_theme_issues_window
            || self.show_unsaved_changes_window
            || self.show_python_import_window
            || self.show_universal_import_modal
            || self.show_color_picker_modal
            || (self.show_junk_cleanup_modal && self.junk_scan_report.is_some())
    }

    pub(crate) fn topmost_notice_modal(&self) -> Option<&'static str> {
        let has_inst = self.selected_instance().is_some();
        let has_update_release = self
            .last_update_check
            .as_ref()
            .and_then(|c| c.latest.as_ref())
            .is_some();

        if self.relocated_storage_prompt.is_some() {
            Some("mcw_modal_storage_relocated")
        } else if !self.pending_duplicate_mod_prompts.is_empty() {
            Some("mcw_modal_duplicate_mod")
        } else if self.install_target_open {
            Some("mcw_modal_install_target")
        } else if self.show_mod_cart_modal {
            Some("mcw_modal_mod_cart")
        } else if self.version_picker_open {
            Some("mcw_modal_version_picker")
        } else if self.modpack_dialog.open {
            Some("mcw_modal_modpack_install")
        } else if self.show_unsaved_changes_window {
            Some("mcw_modal_unsaved_changes")
        } else if self.preflight_confirm_instance_id.is_some() {
            Some("mcw_modal_preflight_confirm")
        } else if self.show_delete_modal && has_inst {
            Some("mcw_modal_delete_instance")
        } else if self.pending_restore_backup_id.is_some() {
            Some("mcw_modal_restore_backup")
        } else if self.pending_delete_world.is_some() {
            Some("mcw_modal_delete_world")
        } else if self.screenshot_preview_path.is_some() {
            Some("mcw_modal_screenshot_preview")
        } else if self.show_rename_modal && has_inst {
            Some("mcw_modal_rename_instance")
        } else if self.show_group_tags_modal && has_inst {
            Some("mcw_modal_edit_group_tags")
        } else if self.show_clone_modal && has_inst {
            Some("mcw_modal_clone_instance")
        } else if !self.open_manual_modals.is_empty() {
            Some("mcw_modal_manual_downloads")
        } else if self.show_all_tasks_modal && self.active_tasks.len() > 2 {
            Some("mcw_all_tasks_modal")
        } else if self.show_update_dialog && has_update_release {
            Some("mcw_modal_update_dialog")
        } else if self.show_python_import_window {
            Some("mcw_modal_python_import")
        } else if self.show_universal_import_modal {
            Some("mcw_modal_universal_import")
        } else if self.show_color_picker_modal {
            Some("mcw_modal_color_picker")
        } else if self.show_junk_cleanup_modal && self.junk_scan_report.is_some() {
            Some("mcw_modal_junk_cleanup")
        } else if self.show_first_run_wizard {
            Some("mcw_modal_first_run_setup")
        } else if self.show_repair_center_window && has_inst {
            Some("mcw_modal_repair_center")
        } else if self.show_java_manager_window {
            Some("mcw_modal_java_manager")
        } else if self.show_content_library_window {
            Some("mcw_modal_content_library")
        } else if self.show_modpack_update_window {
            Some("mcw_modal_modpack_update")
        } else if self.show_issue_report_window {
            Some("mcw_modal_issue_report")
        } else if self.show_theme_issues_window {
            Some("mcw_modal_theme_issues")
        } else if self.show_export_modal && has_inst {
            Some("mcw_modal_export_modpack")
        } else if self.show_account_modal {
            Some("mcw_modal_accounts")
        } else if self.show_create_modal {
            Some("mcw_modal_create_instance")
        } else if self.show_about_window {
            Some("mcw_modal_about")
        } else if self.show_launcher_settings {
            Some("mcw_modal_launcher_settings")
        } else {
            None
        }
    }

    pub(crate) fn modal_notice_for(&self, modal_id: &str) -> Option<(String, bool)> {
        if self.topmost_notice_modal() == Some(modal_id) {
            self.notice_banner.clone()
        } else {
            None
        }
    }
}

impl eframe::App for McwApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            let size = rect.size();
            if size.x >= 840.0 && size.y >= 560.0 {
                self.last_window_size = [size.x, size.y];
                // Wait 2s after startup so initial DPI/non-client frame settlement
                // never falsely marks `window_size_customized = true`.
                if self.window_ready_at.elapsed() >= Duration::from_secs(2) {
                    let settled = *self.settled_window_size.get_or_insert([size.x, size.y]);
                    if (size.x - settled[0]).abs() > 12.0 || (size.y - settled[1]).abs() > 12.0 {
                        self.prefs.window_size_customized = true;
                    }
                }
            }
        }
        self.poll_events(ctx);
        self.sync_active_folder_watcher(ctx);
        self.ensure_account_avatars_loaded(ctx);

        if !self.connectivity_checking {
            let probe_interval = if self.connectivity_status == ConnectivityStatus::Offline {
                Duration::from_secs(15)
            } else {
                Duration::from_secs(45)
            };
            let elapsed = self.last_connectivity_check_at.elapsed();
            if elapsed >= probe_interval {
                self.start_connectivity_check(ctx);
            } else {
                ctx.request_repaint_after(probe_interval - elapsed);
            }
        }
        self.handle_dropped_files(ctx);

        // Auto-dismiss notice_banner after `notification_timeout_secs` (default 8s; 0 = disabled)
        let current_notice_msg = self.notice_banner.as_ref().map(|(m, _)| m.clone());
        if current_notice_msg != self.notice_banner_last_text {
            self.notice_banner_last_text = current_notice_msg.clone();
            self.notice_banner_since = current_notice_msg.as_ref().map(|_| Instant::now());
            if let Some((ref msg, is_err)) = self.notice_banner {
                let tag = if is_err { "ERROR" } else { "NOTICE" };
                self.push_global_log(format!("[{tag}] {msg}"));
            }
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
            }
        }
        if !self.has_open_in_app_modal() {
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) {
                self.navigate_back(ctx);
            } else if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) {
                self.navigate_forward(ctx);
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
        self.render_modals(ctx);
        self.render_quick_search_modal(ctx);
        if !self.startup_splash_done {
            self.render_startup_splash(ctx);
        }
    }
}

fn load_app_logo_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let bytes = include_bytes!("../../../assets/icons/mcw_launcher.png");
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color_img = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("mcw_app_logo_main", color_img, egui::TextureOptions::LINEAR))
}
