use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use mcw_auth::{AccountStore, MicrosoftAuthClient, OfflineAuth, StoredAccount};
use mcw_minecraft::{LoaderKind, ModLoaderService, VanillaLaunchPipeline, VersionManifestService};
use mcw_models::account::AccountSource;
use mcw_models::instance::ModLoader;
use mcw_models::progress::{ProgressEvent, ProgressSink, ProgressStage};
use mcw_mods::{is_loader_compatible, ModManager};
use mcw_net::DownloadClient;
use mcw_providers::{ApiCache, ModrinthClient, ModrinthSearchParams};
use mcw_runtime::{
    ExitClassification, JavaDiscovery, JavaProvisioner, LogLine, LogSink, LogStream,
    SupervisedProcess,
};
use mcw_storage::{InstanceRunLock, InstanceStore, LauncherPaths};
use mcw_updater::{UpdateChannel, UpdateChecker};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "mcw-cli",
    version,
    about = "MCW Launcher 2.0 (Rust Edition) — Core CLI"
)]
struct Cli {
    /// Launcher data root directory (defaults to current working directory)
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List available Minecraft versions from Mojang
    Versions {
        /// Include snapshots
        #[arg(long)]
        snapshots: bool,
        /// Force refresh manifest from Mojang
        #[arg(long)]
        refresh: bool,
        /// Maximum number of versions to display
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Discover or install managed Java runtimes
    Java {
        #[command(subcommand)]
        action: Option<JavaCommands>,
    },
    /// Manage Minecraft instances (list, create, show, rename, delete)
    Instance {
        #[command(subcommand)]
        action: InstanceCommands,
    },
    /// List available Fabric or Quilt loader versions for a Minecraft version
    Loaders {
        /// Loader name: fabric or quilt
        loader: String,
        /// Minecraft version (e.g. 1.21.4)
        version: String,
        /// Maximum number of entries to print
        #[arg(long, default_value_t = 15)]
        limit: usize,
    },
    /// Manage mods inside an instance (parallel .jar scanner)
    Mods {
        #[command(subcommand)]
        action: ModCommands,
    },
    /// Search and install mods from Modrinth v2 (with SQLite WAL cache)
    Modrinth {
        #[command(subcommand)]
        action: ModrinthCommands,
    },
    /// Check for launcher updates (Cloudflare CDN + GitHub Releases failover)
    Update {
        /// Update channel: stable or beta
        #[arg(long, default_value = "stable")]
        channel: String,
    },
    /// Manage Minecraft accounts (Microsoft OAuth & Offline)
    Auth {
        #[command(subcommand)]
        action: AuthCommands,
    },
    /// Download/verify and launch a Minecraft Vanilla instance
    Launch {
        /// Instance name
        #[arg(long)]
        instance: String,
        /// Minecraft version (e.g. 1.21.4)
        #[arg(long)]
        version: String,
        /// Mod loader override (vanilla, fabric, quilt)
        #[arg(long)]
        loader: Option<String>,
        /// Mod loader version override (e.g. latest or 0.16.10)
        #[arg(long)]
        loader_version: Option<String>,
        /// Offline player username (mutually exclusive with --account)
        #[arg(long)]
        offline: Option<String>,
        /// Saved account username or UUID (supports Microsoft & Offline)
        #[arg(long)]
        account: Option<String>,
        /// Explicit path to java executable
        #[arg(long)]
        java: Option<PathBuf>,
        /// Minimum JVM memory in MB
        #[arg(long, default_value_t = 1024)]
        min_mem: u32,
        /// Maximum JVM memory in MB
        #[arg(long, default_value_t = 2048)]
        max_mem: u32,
        /// Window width
        #[arg(long, default_value_t = 854)]
        width: u32,
        /// Window height
        #[arg(long, default_value_t = 480)]
        height: u32,
        /// Prepare & verify everything, print launch command, but do not spawn Minecraft
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum InstanceCommands {
    /// List all instances
    List,
    /// Create a new instance
    Create {
        /// Instance name
        name: String,
        /// Minecraft version (e.g. 1.21.4)
        #[arg(long)]
        version: String,
        /// Mod loader (vanilla, fabric, quilt)
        #[arg(long, default_value = "vanilla")]
        loader: String,
        /// Mod loader version (optional for vanilla)
        #[arg(long, default_value = "-1")]
        loader_version: String,
    },
    /// Show detailed info for an instance
    Show {
        /// Instance name
        name: String,
    },
    /// Rename an existing instance
    Rename {
        /// Current instance name
        old_name: String,
        /// New instance name
        new_name: String,
    },
    /// Delete an instance permanently
    Delete {
        /// Instance name
        name: String,
    },
}

#[derive(Subcommand)]
enum JavaCommands {
    /// List discovered Java runtimes
    List,
    /// Download and install a managed Eclipse Temurin JDK (8, 17, 21, 25)
    Install {
        /// Java major version (8, 17, 21, 25)
        major: u32,
    },
}

#[derive(Subcommand)]
enum ModCommands {
    /// List installed mods in an instance
    List { instance: String },
    /// Enable a disabled mod (.jar.disabled -> .jar)
    Enable { instance: String, file_name: String },
    /// Disable a mod (.jar -> .jar.disabled)
    Disable { instance: String, file_name: String },
    /// Delete a mod file permanently
    Delete { instance: String, file_name: String },
}

#[derive(Subcommand)]
enum ModrinthCommands {
    /// Search Modrinth projects
    Search {
        /// Search query (e.g. sodium)
        query: String,
        /// Filter by Minecraft version (e.g. 1.21.4)
        #[arg(long)]
        version: Option<String>,
        /// Filter by loader (fabric, quilt, forge, neoforge)
        #[arg(long)]
        loader: Option<String>,
        /// Project type (mod, modpack, resourcepack, shader)
        #[arg(long, default_value = "mod")]
        project_type: String,
        /// Maximum results
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// Install a Modrinth mod (and its required dependencies) into an instance
    Install {
        /// Target instance name
        instance: String,
        /// Modrinth project slug or ID (e.g. sodium, fabric-api)
        slug: String,
    },
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Sign in with a Microsoft account (Device Code by default, or --browser for PKCE)
    Login {
        /// Use browser PKCE flow on 127.0.0.1:8400 instead of Device Code
        #[arg(long)]
        browser: bool,
    },
    /// Add an offline account to the local store
    AddOffline {
        /// Offline player username
        username: String,
    },
    /// List stored accounts
    List,
    /// Remove a stored account
    Remove {
        /// Username or UUID
        account: String,
    },
}

struct CliLogSink;

impl LogSink for CliLogSink {
    fn on_log(&self, line: LogLine) {
        match line.stream {
            LogStream::Stdout => println!("{}", line.text),
            LogStream::Stderr => eprintln!("{}", line.text),
        }
    }
}

struct CliProgressSink {
    last_printed_pct: AtomicU64,
}

impl CliProgressSink {
    fn new() -> Self {
        Self {
            last_printed_pct: AtomicU64::new(u64::MAX),
        }
    }
}

impl ProgressSink for CliProgressSink {
    fn on_progress(&self, event: ProgressEvent) {
        match event.stage {
            ProgressStage::DownloadingLibraries | ProgressStage::DownloadingAssets => {
                if event.total == 0 {
                    return;
                }
                let pct = (event.current * 100) / event.total;
                let bucket = pct / 10;
                let prev = self.last_printed_pct.swap(bucket, Ordering::Relaxed);
                if prev != bucket || event.current == event.total {
                    println!("[{:?}] {} ({}%)", event.stage, event.message, pct);
                }
            }
            _ => {
                self.last_printed_pct.store(u64::MAX, Ordering::Relaxed);
                println!("[{:?}] {}", event.stage, event.message);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let root = match cli.root {
        Some(dir) => dir,
        None => std::env::current_dir().context("Failed to determine current directory")?,
    };
    let paths = LauncherPaths::portable(root);
    paths.initialize()?;

    match cli.command {
        Commands::Versions {
            snapshots,
            refresh,
            limit,
        } => {
            let client = DownloadClient::new()?;
            let svc = VersionManifestService::new(&paths, &client);
            let manifest = svc.get_manifest(refresh).await?;
            println!(
                "Latest release: {} | Latest snapshot: {}",
                manifest.latest.release, manifest.latest.snapshot
            );
            let mut count = 0;
            for v in manifest.versions {
                if !snapshots && v.version_type != "release" {
                    continue;
                }
                println!("{:<18} {:<10} {}", v.id, v.version_type, v.release_time);
                count += 1;
                if count >= limit {
                    break;
                }
            }
        }
        Commands::Java { action } => match action.unwrap_or(JavaCommands::List) {
            JavaCommands::List => {
                let found = JavaDiscovery::discover(Some(&paths.runtimes_root));
                if found.is_empty() {
                    println!("No Java runtimes discovered.");
                } else {
                    for j in found {
                        println!(
                            "Java {:<2} ({:<12}) -> {}",
                            j.major,
                            j.version_string,
                            j.path.display()
                        );
                    }
                }
            }
            JavaCommands::Install { major } => {
                let client = DownloadClient::new()?;
                println!("Installing managed Eclipse Temurin Java {major}...");
                let installed =
                    JavaProvisioner::ensure_managed(&paths.runtimes_root, major, &client).await?;
                println!(
                    "Installed Java {} ({}) -> {}",
                    installed.major,
                    installed.version_string,
                    installed.path.display()
                );
            }
        },
        Commands::Instance { action } => {
            let store = InstanceStore::new(&paths);
            match action {
                InstanceCommands::List => {
                    let list = store.list_instances()?;
                    if list.is_empty() {
                        println!("No instances found.");
                    } else {
                        for inst in list {
                            let running = if InstanceRunLock::is_running(&paths, &inst.instance_id)
                            {
                                "running"
                            } else {
                                &inst.last_launch_state
                            };
                            println!(
                                "{:<24} {:<10} {:<10} [{}]",
                                inst.name, inst.version_id, inst.mod_loader.name, running
                            );
                        }
                    }
                }
                InstanceCommands::Create {
                    name,
                    version,
                    loader,
                    loader_version,
                } => {
                    let mod_loader = ModLoader {
                        name: loader.to_lowercase(),
                        version: loader_version,
                    };
                    let (inst, _) = store.create_instance(&name, &version, mod_loader)?;
                    println!(
                        "Created instance '{}' (MC {}, loader: {})",
                        inst.name, inst.version_id, inst.mod_loader.name
                    );
                }
                InstanceCommands::Show { name } => {
                    let (inst, settings) = store.get_instance(&name)?;
                    let running = InstanceRunLock::is_running(&paths, &inst.instance_id);
                    println!("Name     : {}", inst.name);
                    println!("Version  : {}", inst.version_id);
                    println!(
                        "Loader   : {} ({})",
                        inst.mod_loader.name, inst.mod_loader.version
                    );
                    println!(
                        "Status   : {}",
                        if running {
                            "running"
                        } else {
                            &inst.last_launch_state
                        }
                    );
                    println!("Memory   : {}M - {}M", settings.min_memory, settings.max_memory);
                    println!("Window   : {}x{}", settings.width, settings.height);
                    println!("Dir      : {}", paths.instance_dir(&inst.instance_id).display());
                }
                InstanceCommands::Rename { old_name, new_name } => {
                    let updated = store.rename_instance(&old_name, &new_name)?;
                    println!("Renamed instance '{}' -> '{}'", old_name, updated.name);
                }
                InstanceCommands::Delete { name } => {
                    store.delete_instance(&name)?;
                    println!("Deleted instance '{}'", name);
                }
            }
        }
        Commands::Loaders {
            loader,
            version,
            limit,
        } => {
            let kind = LoaderKind::parse(&loader)
                .with_context(|| format!("Unsupported loader '{loader}'. Use 'fabric' or 'quilt'."))?;
            let client = DownloadClient::new()?;
            let svc = ModLoaderService::new(&paths, &client);
            let entries = svc.list_versions(kind, &version).await?;
            for entry in entries.into_iter().take(limit) {
                let tag = if entry.stable { "stable" } else { "beta" };
                println!("{:<20} [{}]", entry.version, tag);
            }
        }
        Commands::Mods { action } => {
            let store = InstanceStore::new(&paths);
            match action {
                ModCommands::List { instance } => {
                    let (inst, _) = store.get_instance(&instance)?;
                    let mods_dir = paths.minecraft_dir(&inst.instance_id).join("mods");
                    let mgr = ModManager::new(&mods_dir);
                    let mods = mgr.list();
                    if mods.is_empty() {
                        println!("No mods installed in '{}'.", inst.name);
                    } else {
                        for m in mods {
                            let state = if m.enabled { "ON " } else { "OFF" };
                            let compat = if is_loader_compatible(m.loader, &inst.mod_loader.name) {
                                ""
                            } else {
                                " [INCOMPATIBLE]"
                            };
                            println!(
                                "[{}] {:<28} {:<14} {:?} Java {}+ ({}){}",
                                state,
                                m.name,
                                m.version,
                                m.loader,
                                m.required_java_major,
                                m.file_name,
                                compat
                            );
                        }
                    }
                }
                ModCommands::Enable {
                    instance,
                    file_name,
                } => {
                    let (inst, _) = store.get_instance(&instance)?;
                    let mods_dir = paths.minecraft_dir(&inst.instance_id).join("mods");
                    let updated = ModManager::new(&mods_dir).set_enabled(&file_name, true)?;
                    println!("Enabled: {}", updated.display());
                }
                ModCommands::Disable {
                    instance,
                    file_name,
                } => {
                    let (inst, _) = store.get_instance(&instance)?;
                    let mods_dir = paths.minecraft_dir(&inst.instance_id).join("mods");
                    let updated = ModManager::new(&mods_dir).set_enabled(&file_name, false)?;
                    println!("Disabled: {}", updated.display());
                }
                ModCommands::Delete {
                    instance,
                    file_name,
                } => {
                    let (inst, _) = store.get_instance(&instance)?;
                    let mods_dir = paths.minecraft_dir(&inst.instance_id).join("mods");
                    ModManager::new(&mods_dir).delete(&file_name)?;
                    println!("Deleted mod '{}'", file_name);
                }
            }
        }
        Commands::Modrinth { action } => {
            let cache_path = paths.cache_root.join("providers").join("api_cache.sqlite");
            let cache = Arc::new(ApiCache::open(&cache_path)?);
            let mr = ModrinthClient::new(Some(cache))?;
            match action {
                ModrinthCommands::Search {
                    query,
                    version,
                    loader,
                    project_type,
                    limit,
                } => {
                    let res = mr
                        .search(&ModrinthSearchParams {
                            query,
                            project_type,
                            loader,
                            game_version: version,
                            limit,
                            ..Default::default()
                        })
                        .await?;
                    println!("Found {} total hits:", res.total_hits);
                    for hit in res.hits {
                        println!(
                            "{:<24} {:<10} {} ({} downloads)",
                            hit.slug, hit.project_type, hit.title, hit.downloads
                        );
                    }
                }
                ModrinthCommands::Install { instance, slug } => {
                    let store = InstanceStore::new(&paths);
                    let (inst, _) = store.get_instance(&instance)?;
                    let mods_dir = paths.minecraft_dir(&inst.instance_id).join("mods");
                    std::fs::create_dir_all(&mods_dir)?;
                    let downloader = DownloadClient::new()?;
                    let installed = mr
                        .install_with_dependencies(
                            &slug,
                            &inst.version_id,
                            &inst.mod_loader.name,
                            &mods_dir,
                            &downloader,
                        )
                        .await?;
                    for item in installed {
                        let tag = if item.is_dependency { "dep" } else { "mod" };
                        println!(
                            "Installed [{}] {} ({}) -> {}",
                            tag,
                            item.project_id,
                            item.version_number,
                            item.file_path.display()
                        );
                    }
                }
            }
        }
        Commands::Update { channel } => {
            let checker = UpdateChecker::new()?;
            let ch = UpdateChannel::parse(&channel);
            let res = checker.check(mcw_models::APP_VERSION, ch).await?;
            println!(
                "Current: {} | Channel: {:?} | Update available: {}",
                res.current_version, res.channel, res.update_available
            );
            if let Some(latest) = res.latest {
                println!(
                    "Latest : {} (source: {}, prerelease: {})",
                    latest.tag_name, latest.source, latest.prerelease
                );
                if !latest.html_url.is_empty() {
                    println!("URL    : {}", latest.html_url);
                }
            }
        }
        Commands::Auth { action } => {
            let store = AccountStore::new(&paths);
            match action {
                AuthCommands::Login { browser } => {
                    let client = MicrosoftAuthClient::new()?;
                    let oauth = if browser {
                        client
                            .authenticate_browser_pkce(Duration::from_secs(180), |url| {
                                println!("Open this URL in your browser to sign in:\n{url}");
                                open_url_in_browser(url);
                            })
                            .await?
                    } else {
                        let dc = client.request_device_code().await?;
                        println!("=== Microsoft Device Login ===");
                        println!("URL  : {}", dc.verification_uri);
                        println!("Code : {}", dc.user_code);
                        if !dc.message.is_empty() {
                            println!("{}", dc.message);
                        }
                        open_url_in_browser(&dc.verification_uri);
                        client
                            .poll_device_code(&dc.device_code, dc.interval, dc.expires_in)
                            .await?
                    };

                    println!("Completing Xbox Live -> XSTS -> Minecraft profile...");
                    let login = client.complete_minecraft_chain(oauth).await?;
                    let uuid = login.authentication.uuid.clone();
                    if let Some(ref rt) = login.refresh_token {
                        store.store_refresh_token(&uuid, rt)?;
                    }
                    let stored = StoredAccount {
                        uuid,
                        username: login.profile.name.clone(),
                        account_type: AccountSource::Microsoft,
                        skin_url: login.profile.active_skin_url(),
                        active: true,
                    };
                    store.upsert(stored, true)?;
                    println!(
                        "Signed in as Microsoft account '{}' ({})",
                        login.profile.name, login.authentication.uuid
                    );
                }
                AuthCommands::AddOffline { username } => {
                    let auth = OfflineAuth::authenticate(&username)?;
                    let stored = StoredAccount {
                        uuid: auth.uuid.clone(),
                        username: auth.player_name.clone(),
                        account_type: AccountSource::Offline,
                        skin_url: None,
                        active: true,
                    };
                    store.upsert(stored, true)?;
                    println!(
                        "Added offline account '{}' ({})",
                        auth.player_name, auth.uuid
                    );
                }
                AuthCommands::List => {
                    let accounts = store.list()?;
                    if accounts.is_empty() {
                        println!("No accounts stored.");
                    } else {
                        for acc in accounts {
                            let marker = if acc.active { "*" } else { " " };
                            println!(
                                "{} {:<20} {:<10} {}",
                                marker,
                                acc.username,
                                format!("{:?}", acc.account_type).to_lowercase(),
                                acc.uuid
                            );
                        }
                    }
                }
                AuthCommands::Remove { account } => {
                    let removed = store.remove(&account)?;
                    println!("Removed account '{}'", removed.username);
                }
            }
        }
        Commands::Launch {
            instance,
            version,
            loader,
            loader_version,
            offline,
            account,
            java,
            min_mem,
            max_mem,
            width,
            height,
            dry_run,
        } => {
            let (auth, account_source) = match (offline, account) {
                (Some(player), None) => (OfflineAuth::authenticate(&player)?, AccountSource::Offline),
                (None, Some(query)) => {
                    let acc_store = AccountStore::new(&paths);
                    let stored = acc_store.find(&query)?;
                    match stored.account_type {
                        AccountSource::Offline => (
                            OfflineAuth::authenticate(&stored.username)?,
                            AccountSource::Offline,
                        ),
                        AccountSource::Microsoft => {
                            let rt = acc_store.load_refresh_token(&stored.uuid)?;
                            let ms_client = MicrosoftAuthClient::new()?;
                            let refreshed = ms_client.refresh_microsoft_token(&rt).await?;
                            let login = ms_client.complete_minecraft_chain(refreshed).await?;
                            if let Some(ref new_rt) = login.refresh_token {
                                let _ = acc_store.store_refresh_token(&stored.uuid, new_rt);
                            }
                            (login.authentication, AccountSource::Microsoft)
                        }
                    }
                }
                (None, None) => bail!("Provide either --offline <PLAYER> or --account <USERNAME>"),
                (Some(_), Some(_)) => bail!("Use either --offline or --account, not both"),
            };

            let store = InstanceStore::new(&paths);
            let (mut inst, mut settings) = store.ensure_instance(&instance, &version)?;
            if let Some(loader_name) = loader {
                inst.mod_loader.name = loader_name.to_ascii_lowercase();
                inst.mod_loader.version = loader_version.unwrap_or_else(|| "latest".to_string());
                store.save_instance(&inst)?;
            } else if let Some(lv) = loader_version {
                inst.mod_loader.version = lv;
                store.save_instance(&inst)?;
            }
            let _lock = InstanceRunLock::acquire(&paths, &inst.instance_id)?;

            settings.min_memory = min_mem;
            settings.max_memory = max_mem;
            settings.width = width;
            settings.height = height;
            if java.is_some() {
                settings.java_path = java;
            }
            store.save_settings(&inst.instance_id, &settings)?;

            let client = DownloadClient::new()?;
            let pipeline = VanillaLaunchPipeline::new(&paths, &client);
            let sink = Arc::new(CliProgressSink::new());
            let prepared = pipeline
                .prepare(&inst, &settings, &auth, account_source, sink)
                .await?;

            println!(
                "Prepared Minecraft {} with Java {} ({})",
                prepared.version_id,
                prepared.java.major,
                prepared.java.path.display()
            );

            if dry_run {
                println!("--dry-run enabled; skipping process spawn.");
                println!("Working dir : {}", prepared.spec.working_dir.display());
                println!("Main class  : {}", prepared.spec.main_class);
                println!("JVM args    : {} items", prepared.spec.jvm_args.len());
                println!("Game args   : {:?}", prepared.spec.game_args);
                return Ok(());
            }

            let log_path = paths
                .logs_root
                .join(format!("{}-latest.log", inst.instance_id));
            let supervised =
                SupervisedProcess::spawn(&prepared.spec, Some(&log_path), Arc::new(CliLogSink))?;
            _lock.mark_running(supervised.pid())?;

            let control = supervised.control();
            let ctrl_c_handle = tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    eprintln!("\nStop requested (Ctrl+C) — terminating Minecraft process tree...");
                    control.request_stop();
                }
            });

            let report = supervised.wait().await?;
            ctrl_c_handle.abort();

            inst.total_playtime_seconds = inst
                .total_playtime_seconds
                .saturating_add(report.duration_secs);
            inst.last_exit_code = report.exit_code;
            inst.last_launch_crashed = report.classification == ExitClassification::Crashed;
            inst.last_launch_state = match report.classification {
                ExitClassification::Crashed => "crashed".to_string(),
                ExitClassification::Normal | ExitClassification::StoppedByUser => {
                    "finished".to_string()
                }
            };
            let _ = store.save_instance(&inst);

            println!(
                "Minecraft exited: {:?} (code: {:?}, duration: {}s, log: {})",
                report.classification,
                report.exit_code,
                report.duration_secs,
                log_path.display()
            );
            if report.unsupported_class_version {
                eprintln!(
                    "Hint: Detected UnsupportedClassVersionError — one or more mods require a newer Java version."
                );
            }
        }
    }

    Ok(())
}

fn open_url_in_browser(url: &str) {
    let _ = webbrowser::open(url);
}
