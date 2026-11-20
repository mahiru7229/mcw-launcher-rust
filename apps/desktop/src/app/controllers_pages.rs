//! Page & Content Browser controllers separated from legacy `controllers.rs` (SOLID SRP).

use super::platform::focus_viewport;
use super::*;

pub(crate) fn content_browser_window_title(global_mode: bool, category: BrowserCategory) -> String {
    if global_mode {
        "Mod & Modpacks - MCW Content Browser".to_string()
    } else {
        format!("{} - MCW Content Browser", category.label())
    }
}

impl McwApp {
    pub(crate) fn load_servers_async(
        &mut self,
        instance_id: &str,
        ctx: &egui::Context,
        force: bool,
    ) {
        if !force {
            if let Some(cached) = self.servers_cache.get(instance_id) {
                if self.selected_instance_id.as_deref() == Some(instance_id) {
                    self.selected_servers = cached.clone();
                }
                return;
            }
        }
        if !self.loading_servers.insert(instance_id.to_string()) {
            return;
        }
        let mc_dir = self.paths.minecraft_dir(instance_id);
        let id = instance_id.to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let servers = tokio::task::spawn_blocking(move || ServerManager::list_servers(&mc_dir))
                .await
                .unwrap_or_default();
            let _ = tx.send(AppEvent::ServersLoaded {
                instance_id: id,
                servers,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn load_cached_versions_async(&mut self, ctx: &egui::Context) {
        if self.cached_versions_loading {
            return;
        }
        self.cached_versions_loading = true;
        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        self.runtime.spawn(async move {
            let list = tokio::task::spawn_blocking(move || {
                StorageCleanupService::list_cached_versions(&paths)
            })
            .await
            .unwrap_or_default();
            let _ = tx.send(AppEvent::CachedVersionsLoaded(list));
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_launch_with_server(&mut self, server_ip: String, ctx: &egui::Context) {
        self.start_launch_impl(None, Some(server_ip), ctx);
    }

    pub(crate) fn open_content_browser(
        &mut self,
        category: BrowserCategory,
        provider: Option<BrowserProvider>,
        ctx: &egui::Context,
    ) {
        self.open_content_browser_mode(category, provider, false, ctx);
    }

    pub(crate) fn open_content_browser_mode(
        &mut self,
        category: BrowserCategory,
        provider: Option<BrowserProvider>,
        global_mode: bool,
        ctx: &egui::Context,
    ) {
        let was_open = self.show_content_browser;
        let prev_global = self.browser_global_mode;
        let prev_provider = self.browser_provider;
        self.show_content_browser = true;
        self.browser_global_mode = global_mode;
        let category_changed = self.browser_category != category;
        self.browser_category = category;

        if let Some(prov) = provider {
            self.browser_provider = prov;
        }
        if category != BrowserCategory::Modpacks
            && !matches!(
                self.browser_provider,
                BrowserProvider::Modrinth | BrowserProvider::CurseForge
            )
        {
            self.browser_provider = BrowserProvider::Modrinth;
        }
        let provider_changed = self.browser_provider != prev_provider;

        // Modpacks default loader filter: Fabric for Modrinth, Forge for CurseForge / ATLauncher / FTB.
        if category == BrowserCategory::Modpacks
            && (!was_open || category_changed || provider_changed)
        {
            self.browser_loader_filter = match self.browser_provider {
                BrowserProvider::Modrinth => "fabric".to_string(),
                BrowserProvider::CurseForge
                | BrowserProvider::Ftb
                | BrowserProvider::AtLauncher => "forge".to_string(),
            };
        } else if category == BrowserCategory::Mods {
            if global_mode {
                if category_changed || !prev_global {
                    self.browser_loader_filter = "all".to_string();
                }
            } else if let Some(inst) = self.selected_instance() {
                if !inst.mod_loader.name.is_empty() && !inst.mod_loader.is_vanilla() {
                    self.browser_loader_filter = inst.mod_loader.name.to_ascii_lowercase();
                }
            }
        }

        if !global_mode && category == BrowserCategory::DataPacks {
            if let Some(id) = self.selected_instance_id.clone() {
                self.load_worlds_async(&id, ctx, false);
            }
        }

        if was_open {
            let vp_id = egui::ViewportId::from_hash_of("mcw_vp_content_browser");
            let title =
                content_browser_window_title(self.browser_global_mode, self.browser_category);
            focus_viewport(ctx, vp_id, Some(&title));
        }

        let scope_key = if self.browser_global_mode {
            format!("global:{}", self.browser_mc_version_filter)
        } else {
            self.selected_instance_id
                .as_deref()
                .unwrap_or("")
                .to_string()
        };
        let target_key = format!(
            "{:?}:{:?}:{}",
            self.browser_category, self.browser_provider, scope_key
        );
        let provider_empty = match self.browser_provider {
            BrowserProvider::Modrinth => self.browser_mr_results.is_empty(),
            BrowserProvider::CurseForge => self.browser_cf_results.is_empty(),
            BrowserProvider::Ftb => self.browser_ftb_results.is_empty(),
            BrowserProvider::AtLauncher => self.browser_atl_results.is_empty(),
        };
        if category_changed
            || provider_changed
            || global_mode != prev_global
            || self.browser_initialized_key.as_deref() != Some(target_key.as_str())
            || provider_empty
        {
            self.browser_selected_key = None;
            self.browser_initialized_key = Some(target_key);
            self.start_content_browser_search(ctx);
        }
    }

    pub(crate) fn start_content_browser_search(&mut self, ctx: &egui::Context) {
        self.browser_request_id = self.browser_request_id.wrapping_add(1);
        let request_id = self.browser_request_id;
        let category = self.browser_category;
        let provider = if category != BrowserCategory::Modpacks
            && !matches!(
                self.browser_provider,
                BrowserProvider::Modrinth | BrowserProvider::CurseForge
            ) {
            BrowserProvider::Modrinth
        } else {
            self.browser_provider
        };
        self.browser_provider = provider;
        self.browser_searching = true;

        let scope_key = if self.browser_global_mode {
            format!("global:{}", self.browser_mc_version_filter)
        } else {
            self.selected_instance_id
                .as_deref()
                .unwrap_or("")
                .to_string()
        };
        let target_key = format!("{:?}:{:?}:{}", category, provider, scope_key);
        self.browser_initialized_key = Some(target_key);

        let query = self.browser_query.clone();
        let sort = self.browser_sort.clone();
        let inst = self.selected_instance().cloned();
        let game_version = if category == BrowserCategory::Modpacks {
            None
        } else if self.browser_global_mode {
            let v = self.browser_mc_version_filter.trim();
            if v.is_empty() || v.eq_ignore_ascii_case("all") {
                None
            } else {
                Some(v.to_string())
            }
        } else {
            inst.as_ref().map(|i| i.version_id.clone())
        };
        let loader = if matches!(category, BrowserCategory::Mods | BrowserCategory::Modpacks)
            && !self.browser_loader_filter.trim().is_empty()
            && self.browser_loader_filter != "all"
        {
            Some(self.browser_loader_filter.clone())
        } else {
            None
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
            let cache = ApiCache::open(&cache_path).ok().map(Arc::new);
            let mut mr_res = Ok(Vec::new());
            let mut cf_res = Ok(Vec::new());
            let mut ftb_res = Ok(Vec::new());
            let mut atl_res = Ok(Vec::new());

            match provider {
                BrowserProvider::Modrinth => {
                    mr_res = async {
                        let mr = ModrinthClient::new(cache).map_err(|e| e.to_string())?;
                        let search = mr
                            .search(&ModrinthSearchParams {
                                query,
                                project_type: category.modrinth_project_type().to_string(),
                                loader,
                                game_version,
                                index: sort,
                                limit: 20,
                                ..Default::default()
                            })
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(search.hits)
                    }
                    .await;
                }
                BrowserProvider::CurseForge => {
                    cf_res = async {
                        let cf =
                            CurseForgeClient::new(api_key, cache).map_err(|e| e.to_string())?;
                        let search = cf
                            .search(&CurseForgeSearchParams {
                                query,
                                kind: category.curseforge_kind().to_string(),
                                loader,
                                game_version,
                                sort,
                                page_size: 20,
                                ..Default::default()
                            })
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(search.data)
                    }
                    .await;
                }
                BrowserProvider::Ftb => {
                    ftb_res = async {
                        let ftb = FtbClient::new(cache).map_err(|e| e.to_string())?;
                        ftb.search_packs(&query, 20)
                            .await
                            .map_err(|e| e.to_string())
                    }
                    .await;
                }
                BrowserProvider::AtLauncher => {
                    atl_res = async {
                        let atl = AtLauncherClient::new(cache).map_err(|e| e.to_string())?;
                        atl.search(&query, 20).await.map_err(|e| e.to_string())
                    }
                    .await;
                }
            }

            let _ = tx.send(AppEvent::ContentBrowserSearchFinished {
                request_id,
                category,
                provider,
                modrinth: mr_res,
                curseforge: cf_res,
                ftb: ftb_res,
                atl: atl_res,
            });
            ctx.request_repaint();
        });
    }
}
