use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mcw_auth::OfflineAuth;
use mcw_minecraft::{VanillaLaunchPipeline, VersionManifestService};
use mcw_models::account::AccountSource;
use mcw_models::instance::ModLoader;
use mcw_models::progress::{ProgressEvent, ProgressSink, ProgressStage};
use mcw_net::DownloadClient;
use mcw_runtime::{JavaDiscovery, ProcessLauncher};
use mcw_storage::{InstanceRunLock, InstanceStore, LauncherPaths};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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
    /// Discover installed Java runtimes on this machine
    Java,
    /// Manage Minecraft instances (list, create, show, rename, delete)
    Instance {
        #[command(subcommand)]
        action: InstanceCommands,
    },
    /// Download/verify and launch a Minecraft Vanilla instance
    Launch {
        /// Instance name
        #[arg(long)]
        instance: String,
        /// Minecraft version (e.g. 1.21.4)
        #[arg(long)]
        version: String,
        /// Offline player username
        #[arg(long)]
        offline: String,
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
        Commands::Java => {
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
        Commands::Launch {
            instance,
            version,
            offline,
            java,
            min_mem,
            max_mem,
            width,
            height,
            dry_run,
        } => {
            let auth = OfflineAuth::authenticate(&offline)?;
            let store = InstanceStore::new(&paths);
            let (inst, mut settings) = store.ensure_instance(&instance, &version)?;
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
                .prepare(&inst, &settings, &auth, AccountSource::Offline, sink)
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

            let mut child = ProcessLauncher::spawn_inherited(&prepared.spec)?;
            _lock.mark_running(child.id())?;
            let status = child.wait().context("Failed waiting on Minecraft process")?;
            println!("Minecraft exited with status: {status}");
        }
    }

    Ok(())
}
