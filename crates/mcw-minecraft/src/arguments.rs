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
}
