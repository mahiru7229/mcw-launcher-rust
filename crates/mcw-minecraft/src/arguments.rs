use crate::rules::LibraryRules;
use mcw_models::account::{AccountSource, Authentication};
use mcw_models::instance::InstanceSettings;
use mcw_models::minecraft::{ArgumentEntry, VersionMetadata};
use std::collections::{HashMap, HashSet};

const UNSAFE_OFFLINE_AUTH_HOST_PREFIXES: &[&str] = &[
    "-Dminecraft.api.auth.host=",
    "-Dminecraft.api.account.host=",
    "-Dminecraft.api.session.host=",
    "-Dminecraft.api.services.host=",
];

const OFFLINE_MULTIPLAYER_HOST_ARGS: &[&str] = &[
    "-Dminecraft.api.auth.host=https://authserver.mojang.com",
    "-Dminecraft.api.account.host=https://api.mojang.com",
    "-Dminecraft.api.session.host=https://sessionserver.mojang.com",
    "-Dminecraft.api.services.host=https://nope.invalid",
];

const OFFLINE_IDENTITY_FLAGS: &[&str] = &[
    "--username",
    "--uuid",
    "--accessToken",
    "--userType",
    "--clientId",
    "--xuid",
];

const OFFLINE_OMITTED_FLAGS: &[&str] = &["--clientId", "--xuid"];

pub struct ArgumentContext {
    pub values: HashMap<String, String>,
    pub features: HashMap<String, bool>,
}

impl ArgumentContext {
    pub fn new(values: HashMap<String, String>) -> Self {
        Self {
            values,
            features: HashMap::new(),
        }
    }
}

pub struct ArgumentBuilder;

impl ArgumentBuilder {
    pub fn build(
        version: &VersionMetadata,
        context: &ArgumentContext,
        settings: &InstanceSettings,
        auth: &Authentication,
        account_source: AccountSource,
    ) -> (Vec<String>, Vec<String>) {
        let mut jvm_args = vec![
            format!("-Xms{}M", settings.min_memory),
            format!("-Xmx{}M", settings.max_memory),
        ];
        jvm_args.extend(settings.jvm_arguments.iter().cloned());

        let mut game_args = vec![
            "--width".to_string(),
            settings.width.to_string(),
            "--height".to_string(),
            settings.height.to_string(),
        ];
        if settings.fullscreen {
            game_args.push("--fullscreen".to_string());
        }
        game_args.extend(settings.game_arguments.iter().cloned());

        if let Some(ref arguments) = version.arguments {
            for entry in &arguments.jvm {
                jvm_args.extend(Self::resolve_entry(entry, context));
            }
            for entry in &arguments.game {
                game_args.extend(Self::resolve_entry(entry, context));
            }
        } else if let Some(ref legacy) = version.minecraft_arguments {
            for token in legacy.split_whitespace() {
                game_args.push(Self::substitute(token, &context.values));
            }
            if let Some(natives) = context.values.get("natives_directory") {
                jvm_args.push(format!("-Djava.library.path={natives}"));
            }
            if let Some(brand) = context.values.get("launcher_name") {
                jvm_args.push(format!("-Dminecraft.launcher.brand={brand}"));
            }
            if let Some(ver) = context.values.get("launcher_version") {
                jvm_args.push(format!("-Dminecraft.launcher.version={ver}"));
            }
            if let Some(cp) = context.values.get("classpath") {
                jvm_args.push("-cp".to_string());
                jvm_args.push(cp.clone());
            }
        }

        if let Some(base_id) = version
            .inherits_from
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let base_jar = format!("{base_id}.jar");
            for arg in &mut jvm_args {
                if let Some(list) = arg.strip_prefix("-DignoreList=") {
                    let has_base = list
                        .split(',')
                        .any(|entry| entry.trim().eq_ignore_ascii_case(&base_jar));
                    if !has_base {
                        if list.trim().is_empty() {
                            *arg = format!("-DignoreList={base_jar}");
                        } else {
                            *arg = format!("{arg},{base_jar}");
                        }
                    }
                }
            }
        }

        if Self::needs_log4shell_mitigation(version)
            && !jvm_args
                .iter()
                .any(|arg| arg.starts_with("-Dlog4j2.formatMsgNoLookups"))
        {
            jvm_args.push("-Dlog4j2.formatMsgNoLookups=true".to_string());
        }

        if account_source == AccountSource::Offline {
            jvm_args.retain(|arg| {
                !UNSAFE_OFFLINE_AUTH_HOST_PREFIXES
                    .iter()
                    .any(|prefix| arg.starts_with(prefix))
            });
            if Self::is_offline_multiplayer_fix_required(version) {
                jvm_args.extend(
                    OFFLINE_MULTIPLAYER_HOST_ARGS
                        .iter()
                        .map(|s| (*s).to_string()),
                );
            }
            game_args = Self::normalize_offline_identity_args(game_args, auth);
        }

        (jvm_args, game_args)
    }

    fn resolve_entry(entry: &ArgumentEntry, context: &ArgumentContext) -> Vec<String> {
        match entry {
            ArgumentEntry::Simple(s) => vec![Self::substitute(s, &context.values)],
            ArgumentEntry::Conditional { rules, value } => {
                if !LibraryRules::is_allowed_with_features(rules, &context.features) {
                    return Vec::new();
                }
                value
                    .as_vec()
                    .into_iter()
                    .map(|item| Self::substitute(item, &context.values))
                    .collect()
            }
        }
    }

    pub fn substitute(template: &str, values: &HashMap<String, String>) -> String {
        let mut out = template.to_string();
        for (k, v) in values {
            let token = format!("${{{k}}}");
            if out.contains(&token) {
                out = out.replace(&token, v);
            }
        }
        out
    }

    fn is_offline_multiplayer_fix_required(version: &VersionMetadata) -> bool {
        let ver = version
            .inherits_from
            .as_deref()
            .unwrap_or(&version.id)
            .trim();
        ver == "1.16.4" || ver == "1.16.5" || ver.contains("1.16.4") || ver.contains("1.16.5")
    }

    /// Automatic Log4Shell mitigation (`CVE-2021-44228`) for Minecraft `1.7` through `1.18.1`.
    pub fn needs_log4shell_mitigation(version: &VersionMetadata) -> bool {
        let raw = version
            .inherits_from
            .as_deref()
            .unwrap_or(&version.id)
            .trim();
        let base = raw.split(['-', '+']).next().unwrap_or(raw).trim();
        let mut parts = base.split('.');
        let Ok(major) = parts.next().unwrap_or_default().parse::<u32>() else {
            return false;
        };
        let Ok(minor) = parts.next().unwrap_or_default().parse::<u32>() else {
            return false;
        };
        let patch = parts
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        major == 1 && ((7..=17).contains(&minor) || (minor == 18 && patch <= 1))
    }

    fn normalize_offline_identity_args(args: Vec<String>, auth: &Authentication) -> Vec<String> {
        let mut canonical = HashMap::new();
        canonical.insert("--username", auth.player_name.clone());
        canonical.insert("--uuid", auth.uuid.replace('-', ""));
        canonical.insert("--accessToken", "0".to_string());
        canonical.insert("--userType", "legacy".to_string());

        let mut normalized = Vec::with_capacity(args.len());
        let mut seen: HashSet<&str> = HashSet::new();
        let mut i = 0;

        while i < args.len() {
            let arg = &args[i];
            let matched = OFFLINE_IDENTITY_FLAGS
                .iter()
                .copied()
                .find(|flag| arg == flag || arg.starts_with(&format!("{flag}=")));

            let Some(flag) = matched else {
                normalized.push(arg.clone());
                i += 1;
                continue;
            };

            let inline = arg.starts_with(&format!("{flag}="));
            i += if inline { 1 } else { 2 };

            if seen.contains(flag) || OFFLINE_OMITTED_FLAGS.contains(&flag) {
                continue;
            }
            if let Some(val) = canonical.get(flag) {
                if !val.is_empty() {
                    normalized.push(flag.to_string());
                    normalized.push(val.clone());
                    seen.insert(flag);
                }
            }
        }

        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcw_models::minecraft::{ArgumentValue, Arguments};

    #[test]
    fn substitutes_placeholders_and_normalizes_offline_identity() {
        let meta = VersionMetadata {
            id: "1.21.4".to_string(),
            version_type: "release".to_string(),
            main_class: "net.minecraft.client.main.Main".to_string(),
            inherits_from: None,
            asset_index: None,
            assets: Some("19".to_string()),
            downloads: None,
            libraries: vec![],
            java_version: None,
            minecraft_arguments: None,
            arguments: Some(Arguments {
                jvm: vec![
                    ArgumentEntry::Simple("-Djava.library.path=${natives_directory}".to_string()),
                    ArgumentEntry::Simple("-cp".to_string()),
                    ArgumentEntry::Simple("${classpath}".to_string()),
                ],
                game: vec![
                    ArgumentEntry::Simple("--username".to_string()),
                    ArgumentEntry::Simple("${auth_player_name}".to_string()),
                    ArgumentEntry::Simple("--uuid".to_string()),
                    ArgumentEntry::Simple("${auth_uuid}".to_string()),
                    ArgumentEntry::Simple("--accessToken".to_string()),
                    ArgumentEntry::Simple("${auth_access_token}".to_string()),
                    ArgumentEntry::Simple("--xuid".to_string()),
                    ArgumentEntry::Simple("${auth_xuid}".to_string()),
                    ArgumentEntry::Conditional {
                        rules: vec![],
                        value: ArgumentValue::Multiple(vec![
                            "--version".to_string(),
                            "${version_name}".to_string(),
                        ]),
                    },
                ],
            }),
        };

        let mut values = HashMap::new();
        values.insert("natives_directory".to_string(), "/natives".to_string());
        values.insert("classpath".to_string(), "/cp/client.jar".to_string());
        values.insert("auth_player_name".to_string(), "Player".to_string());
        values.insert(
            "auth_uuid".to_string(),
            "0123456789abcdef0123456789abcdef".to_string(),
        );
        values.insert("auth_access_token".to_string(), "0".to_string());
        values.insert("auth_xuid".to_string(), "".to_string());
        values.insert("version_name".to_string(), "1.21.4".to_string());

        let auth = Authentication {
            player_name: "Player".to_string(),
            uuid: "0123456789abcdef0123456789abcdef".to_string(),
            access_token: "0".to_string(),
            xuid: String::new(),
            client_id: String::new(),
            user_type: "legacy".to_string(),
        };

        let (jvm, game) = ArgumentBuilder::build(
            &meta,
            &ArgumentContext::new(values),
            &InstanceSettings::default(),
            &auth,
            AccountSource::Offline,
        );

        assert!(jvm.contains(&"-Djava.library.path=/natives".to_string()));
        assert!(jvm.contains(&"/cp/client.jar".to_string()));
        assert!(!game.contains(&"--xuid".to_string()));
        assert!(game.contains(&"Player".to_string()));
        assert!(game.contains(&"1.21.4".to_string()));
    }

    #[test]
    fn appends_inherited_base_jar_to_ignore_list() {
        let meta = VersionMetadata {
            id: "1.20.1-forge-47.4.0".to_string(),
            version_type: "release".to_string(),
            main_class: "cpw.mods.bootstraplauncher.BootstrapLauncher".to_string(),
            inherits_from: Some("1.20.1".to_string()),
            asset_index: None,
            assets: Some("5".to_string()),
            downloads: None,
            libraries: vec![],
            java_version: None,
            minecraft_arguments: None,
            arguments: Some(Arguments {
                jvm: vec![ArgumentEntry::Simple(
                    "-DignoreList=bootstraplauncher,${version_name}.jar".to_string(),
                )],
                game: vec![],
            }),
        };

        let mut values = HashMap::new();
        values.insert(
            "version_name".to_string(),
            "1.20.1-forge-47.4.0".to_string(),
        );
        let auth = Authentication {
            player_name: "Player".to_string(),
            uuid: "0123456789abcdef0123456789abcdef".to_string(),
            access_token: "0".to_string(),
            xuid: String::new(),
            client_id: String::new(),
            user_type: "legacy".to_string(),
        };

        let (jvm, _) = ArgumentBuilder::build(
            &meta,
            &ArgumentContext::new(values),
            &InstanceSettings::default(),
            &auth,
            AccountSource::Offline,
        );

        assert!(jvm.contains(
            &"-DignoreList=bootstraplauncher,1.20.1-forge-47.4.0.jar,1.20.1.jar".to_string()
        ));
    }

    #[test]
    fn injects_log4shell_mitigation_for_vulnerable_versions_only() {
        let make_meta = |id: &str, inherits_from: Option<&str>| VersionMetadata {
            id: id.to_string(),
            version_type: "release".to_string(),
            main_class: "net.minecraft.client.main.Main".to_string(),
            inherits_from: inherits_from.map(str::to_string),
            asset_index: None,
            assets: None,
            downloads: None,
            libraries: vec![],
            java_version: None,
            minecraft_arguments: None,
            arguments: Some(Arguments {
                jvm: vec![],
                game: vec![],
            }),
        };

        for ver in ["1.7.10", "1.12.2", "1.16.5", "1.18", "1.18.1"] {
            let meta = make_meta(ver, None);
            let (jvm, _) = ArgumentBuilder::build(
                &meta,
                &ArgumentContext::new(HashMap::new()),
                &InstanceSettings::default(),
                &Authentication {
                    player_name: "Player".to_string(),
                    uuid: "0123456789abcdef0123456789abcdef".to_string(),
                    access_token: "0".to_string(),
                    xuid: String::new(),
                    client_id: String::new(),
                    user_type: "legacy".to_string(),
                },
                AccountSource::Offline,
            );
            assert!(
                jvm.contains(&"-Dlog4j2.formatMsgNoLookups=true".to_string()),
                "expected mitigation for {ver}"
            );
        }

        let inherited = make_meta("1.16.5-forge-36.2.39", Some("1.16.5"));
        assert!(ArgumentBuilder::needs_log4shell_mitigation(&inherited));

        for safe in ["1.6.4", "1.18.2", "1.20.1", "1.21.4"] {
            assert!(
                !ArgumentBuilder::needs_log4shell_mitigation(&make_meta(safe, None)),
                "unexpected mitigation for {safe}"
            );
        }
    }
}
