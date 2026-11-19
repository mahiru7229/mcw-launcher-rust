use crate::arguments::{ArgumentBuilder, ArgumentContext};
use crate::assets::AssetManager;
use crate::classpath::ClasspathBuilder;
use crate::forge::{ForgeFamily, ForgeLoaderService};
use crate::libraries::LibraryManager;
use crate::manifest::VersionManifestService;
use crate::modloader::{LoaderKind, ModLoaderService};
use crate::{MinecraftError, Result};
use mcw_models::account::{AccountSource, Authentication};
use mcw_models::instance::{Instance, InstanceSettings};
use mcw_models::progress::{ProgressEvent, ProgressSink, ProgressStage};
use mcw_models::{APP_VERSION, LAUNCHER_NAME};
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_runtime::{
    detect_mods_required_java, JavaInstallation, JavaMajorPolicy, JavaProvisioner, JavaResolver,
    LaunchSpec,
};
use mcw_storage::LauncherPaths;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct PreparedLaunch {
    pub version_id: String,
    pub java: JavaInstallation,
    pub spec: LaunchSpec,
}

pub struct VanillaLaunchPipeline<'a> {
    paths: &'a LauncherPaths,
    client: &'a DownloadClient,
}

impl<'a> VanillaLaunchPipeline<'a> {
    pub fn new(paths: &'a LauncherPaths, client: &'a DownloadClient) -> Self {
        Self { paths, client }
    }

    pub async fn prepare(
        &self,
        instance: &Instance,
        settings: &InstanceSettings,
        auth: &Authentication,
        account_source: AccountSource,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<PreparedLaunch> {
        // 1. Fetch manifest & version metadata
        progress.on_progress(ProgressEvent {
            stage: ProgressStage::FetchingManifest,
            message: format!("Fetching Mojang manifest for {}...", instance.version_id),
            current: 0,
            total: 1,
        });
        let manifest_svc = VersionManifestService::new(self.paths, self.client);
        let entry = manifest_svc
            .find_version(&instance.version_id, false)
            .await?;

        progress.on_progress(ProgressEvent {
            stage: ProgressStage::DownloadingMetadata,
            message: format!("Downloading metadata for {}...", entry.id),
            current: 0,
            total: 1,
        });
        let base_metadata = manifest_svc.load_version_metadata(&entry).await?;
        let base_version_id = base_metadata.id.clone();

        // 2. Download base client JAR early (needed by modern Forge/NeoForge installers too)
        let client_artifact = base_metadata
            .downloads
            .as_ref()
            .and_then(|d| d.client.as_ref())
            .ok_or_else(|| {
                MinecraftError::InvalidMetadata(format!(
                    "Version '{}' is missing client download metadata",
                    base_version_id
                ))
            })?;
        let client_jar = self.paths.client_jar(&base_version_id);
        progress.on_progress(ProgressEvent {
            stage: ProgressStage::DownloadingClient,
            message: format!("Downloading client JAR ({})...", base_version_id),
            current: 0,
            total: 1,
        });
        let client_req = DownloadRequest::new(&client_artifact.url, &client_jar)
            .with_sha1(client_artifact.sha1.clone())
            .with_size(client_artifact.size);
        self.client.download_file(&client_req).await?;

        // 3. Resolve Java runtime BEFORE Forge/NeoForge installer (so modern installers can run
        //    with the right Java, and legacy 1.12.2 Forge gets Java 8 automatically!)
        let metadata_java_major = base_metadata.java_version.as_ref().map(|j| j.major_version);
        let mut required_java =
            JavaMajorPolicy::required_for_minecraft(&base_version_id, metadata_java_major)?;
        let loader_name = instance.mod_loader.name.to_ascii_lowercase();
        if loader_name == "fabric" || loader_name == "quilt" {
            let mods_dir = self.paths.minecraft_dir(&instance.instance_id).join("mods");
            if mods_dir.is_dir() {
                required_java = required_java.max(detect_mods_required_java(&mods_dir));
            }
        }
        let allow_higher = matches!(loader_name.as_str(), "vanilla" | "fabric" | "quilt");

        progress.on_progress(ProgressEvent {
            stage: ProgressStage::SelectingJava,
            message: format!("Selecting Java runtime (requires Java {required_java}+)..."),
            current: 0,
            total: 1,
        });
        let java = match JavaResolver::resolve(
            required_java,
            settings.java_path.as_deref(),
            Some(&self.paths.runtimes_root),
            allow_higher,
        ) {
            Ok(found) => found,
            Err(_) if settings.java_path.is_none() => {
                let managed_major = JavaMajorPolicy::resolve(required_java)?;
                progress.on_progress(ProgressEvent {
                    stage: ProgressStage::InstallingJava,
                    message: format!("Downloading managed Eclipse Temurin Java {managed_major}..."),
                    current: 0,
                    total: 1,
                });
                JavaProvisioner::ensure_managed(
                    &self.paths.runtimes_root,
                    required_java,
                    self.client,
                )
                .await?
            }
            Err(err) => return Err(err.into()),
        };

        // 4. Merge Mod Loader profile (Fabric, Quilt, Forge, or NeoForge)
        let metadata = if let Some(kind) = LoaderKind::parse(&instance.mod_loader.name) {
            progress.on_progress(ProgressEvent {
                stage: ProgressStage::InstallingModLoader,
                message: format!(
                    "Resolving {} loader for {}...",
                    kind.as_str(),
                    base_version_id
                ),
                current: 0,
                total: 1,
            });
            let loader_svc = ModLoaderService::new(self.paths, self.client);
            loader_svc
                .install_and_merge(kind, &base_metadata, &instance.mod_loader.version)
                .await?
        } else if let Some(family) = ForgeFamily::parse(&instance.mod_loader.name) {
            progress.on_progress(ProgressEvent {
                stage: ProgressStage::InstallingModLoader,
                message: format!(
                    "Installing {} {} for {}...",
                    family.as_str(),
                    instance.mod_loader.version,
                    base_version_id
                ),
                current: 0,
                total: 1,
            });
            let forge_svc = ForgeLoaderService::new(self.paths, self.client);
            forge_svc
                .install_and_merge(
                    family,
                    &base_metadata,
                    &instance.mod_loader.version,
                    Some(&java.path),
                )
                .await?
        } else {
            base_metadata
        };

        // 5. Download libraries & extract natives
        let lib_mgr = LibraryManager::new(self.paths, self.client);
        lib_mgr
            .ensure_libraries(&metadata, Arc::clone(&progress))
            .await?;

        // 6. Download assets
        let asset_mgr = AssetManager::new(self.paths, self.client);
        let asset_index_id = asset_mgr
            .ensure_assets(&metadata, Arc::clone(&progress))
            .await?;

        // 6. Build classpath & launch arguments
        let libraries_dir = self.paths.libraries_dir();
        let natives_dir = self.paths.natives_dir(&metadata.id);
        std::fs::create_dir_all(&natives_dir)?;
        let game_dir = self.paths.minecraft_dir(&instance.instance_id);
        std::fs::create_dir_all(&game_dir)?;

        let classpath = ClasspathBuilder::build(&metadata, &client_jar, &libraries_dir);
        let mut ctx_values = HashMap::new();
        ctx_values.insert("auth_player_name".to_string(), auth.player_name.clone());
        ctx_values.insert("version_name".to_string(), metadata.id.clone());
        ctx_values.insert(
            "game_directory".to_string(),
            game_dir.to_string_lossy().to_string(),
        );
        ctx_values.insert(
            "assets_root".to_string(),
            self.paths.assets_dir().to_string_lossy().to_string(),
        );
        ctx_values.insert("assets_index_name".to_string(), asset_index_id);
        ctx_values.insert("auth_uuid".to_string(), auth.uuid.clone());
        ctx_values.insert("auth_access_token".to_string(), auth.access_token.clone());
        ctx_values.insert("auth_xuid".to_string(), auth.xuid.clone());
        ctx_values.insert("clientid".to_string(), auth.client_id.clone());
        ctx_values.insert("user_type".to_string(), auth.user_type.clone());
        ctx_values.insert(
            "version_type".to_string(),
            if metadata.version_type.is_empty() {
                "release".to_string()
            } else {
                metadata.version_type.clone()
            },
        );
        ctx_values.insert(
            "natives_directory".to_string(),
            natives_dir.to_string_lossy().to_string(),
        );
        ctx_values.insert("launcher_name".to_string(), LAUNCHER_NAME.to_string());
        ctx_values.insert("launcher_version".to_string(), APP_VERSION.to_string());
        ctx_values.insert("classpath".to_string(), classpath);
        ctx_values.insert(
            "library_directory".to_string(),
            libraries_dir.to_string_lossy().to_string(),
        );
        ctx_values.insert(
            "classpath_separator".to_string(),
            if cfg!(windows) { ";" } else { ":" }.to_string(),
        );

        let (jvm_args, game_args) = ArgumentBuilder::build(
            &metadata,
            &ArgumentContext::new(ctx_values),
            settings,
            auth,
            account_source,
        );

        progress.on_progress(ProgressEvent {
            stage: ProgressStage::Completed,
            message: "Ready to launch".to_string(),
            current: 1,
            total: 1,
        });

        Ok(PreparedLaunch {
            version_id: metadata.id,
            java: java.clone(),
            spec: LaunchSpec {
                java_executable: java.path,
                working_dir: game_dir,
                jvm_args,
                main_class: metadata.main_class,
                game_args,
            },
        })
    }
}
