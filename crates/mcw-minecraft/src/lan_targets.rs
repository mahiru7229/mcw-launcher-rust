use crate::{MinecraftError, Result};
use mcw_models::instance::Instance;
use mcw_models::minecraft::{ArgumentEntry, VersionMetadata};
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::LauncherPaths;
use regex::Regex;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

const MIN_SUPPORTED_VERSION: (u32, u32, u32) = (1, 14, 0);
const NAMED_CLASS: &str = "net/minecraft/server/MinecraftServer";
const NAMED_METHODS: [&str; 2] = ["setUsesAuthentication", "setOnlineMode"];
const TARGET_DESCRIPTOR: &str = "(Z)V";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanAgentTarget {
    pub namespace: String,
    pub class_name: String,
    pub method_name: String,
}

impl LanAgentTarget {
    pub fn encoded(&self) -> String {
        format!("{}#{}", self.class_name, self.method_name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct LanAgentTargetResolution {
    pub game_version: String,
    pub loader: String,
    pub targets: Vec<LanAgentTarget>,
    pub warnings: Vec<String>,
}

impl LanAgentTargetResolution {
    pub fn encoded_targets(&self) -> Vec<String> {
        self.targets.iter().map(LanAgentTarget::encoded).collect()
    }
}

/// Port of MCW Launcher Python's `LanAgentTargetResolver`
/// (`src/core/lan/lan_agent_target_resolver.py`).
///
/// Infers the obfuscated/remapped `MinecraftServer#setUsesAuthentication(boolean)`
/// (or `setOnlineMode(boolean)`) bytecode target across Vanilla official mappings,
/// Fabric intermediary, Quilt hashed/intermediary, and Forge/NeoForge SRG runtime jars.
pub struct LanAgentTargetResolver;

impl LanAgentTargetResolver {
    pub async fn resolve(
        paths: &LauncherPaths,
        client: &DownloadClient,
        version: &VersionMetadata,
        instance: &Instance,
    ) -> LanAgentTargetResolution {
        let game_version = if instance.version_id.trim().is_empty() {
            version.id.trim().to_string()
        } else {
            instance.version_id.trim().to_string()
        };
        let loader = instance.mod_loader.name.trim().to_ascii_lowercase();
        let parsed_version = parse_release_version(&game_version);

        if let Some(pv) = parsed_version {
            if pv < MIN_SUPPORTED_VERSION {
                return LanAgentTargetResolution {
                    game_version,
                    loader,
                    targets: Vec::new(),
                    warnings: vec!["MCW LAN Agent supports Minecraft 1.14 or newer.".to_string()],
                };
            }
        }

        let preferred_named: Vec<&str> = if parsed_version.is_none_or(|v| v >= (26, 1, 0)) {
            NAMED_METHODS.to_vec()
        } else {
            NAMED_METHODS.iter().rev().copied().collect()
        };
        let mut targets: Vec<LanAgentTarget> = preferred_named
            .into_iter()
            .map(|m| LanAgentTarget {
                namespace: "named".to_string(),
                class_name: NAMED_CLASS.to_string(),
                method_name: m.to_string(),
            })
            .collect();
        let mut warnings = Vec::new();

        let mut official_target: Option<LanAgentTarget> = None;
        match Self::ensure_client_mappings(paths, client, version, &game_version).await {
            Ok(Some(mapping_path)) => {
                match fs::read_to_string(&mapping_path)
                    .ok()
                    .and_then(|txt| Self::parse_mojang_client_mappings(&txt))
                {
                    Some(t) => {
                        targets.push(t.clone());
                        official_target = Some(t);
                    }
                    None => warnings.push(
                        "Mojang client mappings did not contain the LAN authentication setter."
                            .to_string(),
                    ),
                }
            }
            Ok(None) => {
                if parsed_version.is_some_and(|v| v < (26, 1, 0)) {
                    warnings.push(
                        "Version metadata does not expose Mojang client mappings; falling back to named runtime targets."
                            .to_string(),
                    );
                }
            }
            Err(err) => warnings.push(format!("Mojang mapping resolution warning: {err}")),
        }

        if let Some(ref official) = official_target {
            if loader == "fabric" || loader == "quilt" {
                match Self::resolve_fabric_family_target(paths, version, &loader, official) {
                    Ok(Some(t)) => targets.insert(0, t),
                    Ok(None) => warnings.push(format!(
                        "{loader} mappings did not expose an alternate LAN authentication target."
                    )),
                    Err(err) => {
                        warnings.push(format!("{loader} mapping resolution warning: {err}"))
                    }
                }
            } else if loader == "forge" || loader == "neoforge" {
                match Self::resolve_forge_srg_target(paths, version, &game_version, &loader, official)
                {
                    Ok(Some(t)) => targets.insert(0, t),
                    Ok(None) => warnings.push(format!(
                        "{loader} SRG runtime artifacts not present; named/official targets remain available."
                    )),
                    Err(err) => {
                        warnings.push(format!("{loader} SRG mapping resolution warning: {err}"))
                    }
                }
            }
        }

        LanAgentTargetResolution {
            game_version,
            loader,
            targets: deduplicate_targets(targets),
            warnings,
        }
    }

    async fn ensure_client_mappings(
        paths: &LauncherPaths,
        client: &DownloadClient,
        version: &VersionMetadata,
        game_version: &str,
    ) -> Result<Option<PathBuf>> {
        let Some(dl) = version
            .downloads
            .as_ref()
            .and_then(|d| d.client_mappings.as_ref())
        else {
            return Ok(None);
        };
        if dl.url.trim().is_empty() {
            return Ok(None);
        }
        let safe_ver = sanitize_version_component(game_version);
        let dest = paths
            .cache_root
            .join("runtime")
            .join("agents")
            .join("mcw-lan-agent")
            .join("mappings")
            .join(safe_ver)
            .join("client.txt");
        let req = DownloadRequest::new(&dl.url, &dest)
            .with_sha1(dl.sha1.clone())
            .with_size(dl.size);
        client.download_file(&req).await?;
        Ok(Some(dest))
    }

    /// Parses ProGuard-format Mojang `client.txt` mappings for `MinecraftServer#setUsesAuthentication(boolean)`.
    pub fn parse_mojang_client_mappings(text: &str) -> Option<LanAgentTarget> {
        let named_dot = NAMED_CLASS.replace('/', ".");
        let method_re =
            Regex::new(r"^(?:\d+:\d+:)*(\S+)\s+([^\s(]+)\(([^)]*)\)(?::\d+:\d+)?$").ok()?;

        let mut current_class_matches = false;
        let mut official_class = String::new();

        for raw_line in text.lines() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if !raw_line.starts_with(char::is_whitespace) {
                current_class_matches = false;
                if let Some(without_colon) = trimmed.strip_suffix(':') {
                    if let Some((named_c, mapped_c)) = without_colon.split_once(" -> ") {
                        if named_c.trim() == named_dot {
                            official_class = mapped_c.trim().replace('.', "/");
                            current_class_matches = true;
                        }
                    }
                }
                continue;
            }

            if !current_class_matches {
                continue;
            }
            let Some((left, mapped_name)) = trimmed.rsplit_once(" -> ") else {
                continue;
            };
            let Some(caps) = method_re.captures(left.trim()) else {
                continue;
            };
            let ret_type = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let method_name = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let args_raw = caps.get(3).map(|m| m.as_str().trim()).unwrap_or("");
            if ret_type == "void" && NAMED_METHODS.contains(&method_name) && args_raw == "boolean" {
                return Some(LanAgentTarget {
                    namespace: "official".to_string(),
                    class_name: official_class,
                    method_name: mapped_name.trim().to_string(),
                });
            }
        }
        None
    }

    fn resolve_fabric_family_target(
        paths: &LauncherPaths,
        version: &VersionMetadata,
        loader: &str,
        official: &LanAgentTarget,
    ) -> Result<Option<LanAgentTarget>> {
        let libs_dir = paths.libraries_dir();
        if let Some(jar) = find_mapping_jar(&libs_dir, version, "net.fabricmc:intermediary:") {
            if let Some(t) = Self::parse_tiny_jar(&jar, official, "intermediary", "intermediary")? {
                return Ok(Some(t));
            }
        }
        if loader == "quilt" {
            if let Some(jar) = find_mapping_jar(&libs_dir, version, "org.quiltmc:hashed:") {
                if let Some(t) = Self::parse_tiny_jar(&jar, official, "hashed", "quilt-hashed")? {
                    return Ok(Some(t));
                }
            }
        }
        Ok(None)
    }

    fn parse_tiny_jar(
        jar_path: &Path,
        official: &LanAgentTarget,
        target_ns: &str,
        result_ns: &str,
    ) -> Result<Option<LanAgentTarget>> {
        let file = File::open(jar_path)?;
        let mut archive = ZipArchive::new(file)?;
        let mut entry = archive.by_name("mappings/mappings.tiny")?;
        let mut payload = String::new();
        entry.read_to_string(&mut payload)?;
        Ok(Self::parse_tiny_mappings(
            &payload, official, target_ns, result_ns,
        ))
    }

    /// Parses Fabric/Quilt Tiny v1 (`v1\t...`) or Tiny v2 (`tiny\t2\t...`) mappings.
    pub fn parse_tiny_mappings(
        payload: &str,
        official: &LanAgentTarget,
        target_ns: &str,
        result_ns: &str,
    ) -> Option<LanAgentTarget> {
        let mut lines = payload.lines();
        let header_line = lines.next()?;
        let header: Vec<&str> = header_line.split('\t').collect();
        if header.first() == Some(&"v1") {
            let namespaces = &header[1..];
            let off_idx = namespaces.iter().position(|n| *n == "official")?;
            let tgt_idx = namespaces.iter().position(|n| *n == target_ns)?;
            let official_cls = normalize_internal(&official.class_name);
            let mut target_cls = official_cls.clone();

            for line in lines {
                let cols: Vec<&str> = line.split('\t').collect();
                if cols.first() == Some(&"CLASS") && cols.len() > namespaces.len() {
                    let names = &cols[1..=namespaces.len()];
                    if normalize_internal(names[off_idx]) == official_cls {
                        target_cls = normalize_internal(names[tgt_idx]);
                    }
                } else if cols.first() == Some(&"METHOD") && cols.len() >= 3 + namespaces.len() {
                    let owner = normalize_internal(cols[1]);
                    let desc = cols[2].trim();
                    let names = &cols[3..3 + namespaces.len()];
                    if owner == official_cls
                        && desc == TARGET_DESCRIPTOR
                        && names[off_idx].trim() == official.method_name
                    {
                        return Some(LanAgentTarget {
                            namespace: result_ns.to_string(),
                            class_name: target_cls,
                            method_name: names[tgt_idx].trim().to_string(),
                        });
                    }
                }
            }
            return None;
        }

        if header.first() == Some(&"tiny") && header.get(1) == Some(&"2") && header.len() >= 5 {
            let namespaces = &header[3..];
            let off_idx = namespaces.iter().position(|n| *n == "official")?;
            let tgt_idx = namespaces.iter().position(|n| *n == target_ns)?;
            let official_cls = normalize_internal(&official.class_name);
            let mut current_off = String::new();
            let mut current_tgt = String::new();

            for line in lines {
                let is_indented = line.starts_with('\t');
                let cols: Vec<&str> = line.trim_start_matches('\t').split('\t').collect();
                if !is_indented && cols.first() == Some(&"c") && cols.len() > namespaces.len() {
                    let names = &cols[1..=namespaces.len()];
                    current_off = normalize_internal(names[off_idx]);
                    current_tgt = normalize_internal(names[tgt_idx]);
                    continue;
                }
                if current_off == official_cls
                    && cols.first() == Some(&"m")
                    && cols.len() >= 2 + namespaces.len()
                {
                    let desc = cols[1].trim();
                    let names = &cols[2..2 + namespaces.len()];
                    if desc == TARGET_DESCRIPTOR && names[off_idx].trim() == official.method_name {
                        return Some(LanAgentTarget {
                            namespace: result_ns.to_string(),
                            class_name: current_tgt,
                            method_name: names[tgt_idx].trim().to_string(),
                        });
                    }
                }
            }
            return None;
        }
        None
    }

    fn resolve_forge_srg_target(
        paths: &LauncherPaths,
        version: &VersionMetadata,
        game_version: &str,
        loader: &str,
        official: &LanAgentTarget,
    ) -> Result<Option<LanAgentTarget>> {
        let Some((slim_jar, srg_jar)) = Self::find_forge_runtime_jars(paths, version, game_version)
        else {
            return Ok(None);
        };
        let slim_methods = Self::read_class_methods(&slim_jar, &official.class_name)?;
        let srg_methods = Self::read_class_methods(&srg_jar, &official.class_name)?;
        if slim_methods.len() != srg_methods.len() {
            return Err(MinecraftError::InvalidMetadata(
                "Forge slim and SRG MinecraftServer method tables differ in length.".to_string(),
            ));
        }

        let matching: Vec<usize> = slim_methods
            .iter()
            .enumerate()
            .filter(|(_, (name, desc))| name == &official.method_name && desc == TARGET_DESCRIPTOR)
            .map(|(i, _)| i)
            .collect();
        if matching.len() != 1 {
            return Ok(None);
        }
        let (srg_name, srg_desc) = &srg_methods[matching[0]];
        if srg_desc != TARGET_DESCRIPTOR || srg_name.is_empty() {
            return Ok(None);
        }
        let ns = if loader == "neoforge" {
            "neoforge-srg"
        } else {
            "forge-srg"
        };
        Ok(Some(LanAgentTarget {
            namespace: ns.to_string(),
            class_name: official.class_name.clone(),
            method_name: srg_name.clone(),
        }))
    }

    fn find_forge_runtime_jars(
        paths: &LauncherPaths,
        version: &VersionMetadata,
        game_version: &str,
    ) -> Option<(PathBuf, PathBuf)> {
        let client_root = paths
            .libraries_dir()
            .join("net")
            .join("minecraft")
            .join("client");
        if !client_root.is_dir() {
            return None;
        }
        let mut dirs = Vec::new();
        if let Some(mcp) = forge_mcp_version(version) {
            dirs.push(client_root.join(format!("{game_version}-{mcp}")));
        }
        let prefix = format!("{game_version}-");
        if let Ok(entries) = fs::read_dir(&client_root) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir()
                    && e.file_name().to_string_lossy().starts_with(&prefix)
                    && !dirs.contains(&p)
                {
                    dirs.push(p);
                }
            }
        }
        for dir in dirs {
            let folder_name = dir.file_name()?.to_string_lossy();
            let base = format!("client-{folder_name}");
            let slim = dir.join(format!("{base}-slim.jar"));
            let srg = dir.join(format!("{base}-srg.jar"));
            if slim.is_file() && srg.is_file() {
                return Some((slim, srg));
            }
        }
        None
    }

    fn read_class_methods(jar_path: &Path, class_name: &str) -> Result<Vec<(String, String)>> {
        let entry_name = format!("{}.class", normalize_internal(class_name));
        let file = File::open(jar_path)?;
        let mut archive = ZipArchive::new(file)?;
        let mut entry = archive.by_name(&entry_name)?;
        let mut payload = Vec::new();
        entry.read_to_end(&mut payload)?;
        Self::parse_class_method_table(&payload)
    }

    /// Parses the JVM binary `.class` constant pool & `methods[]` table to extract `(name, descriptor)`.
    pub fn parse_class_method_table(payload: &[u8]) -> Result<Vec<(String, String)>> {
        let mut r = ClassReader::new(payload);
        if r.read_u4()? != 0xCAFE_BABE {
            return Err(MinecraftError::InvalidMetadata(
                "Invalid Java class file magic".to_string(),
            ));
        }
        r.skip(4)?; // minor + major version
        let cp_count = r.read_u2()? as usize;
        let mut utf8 = std::collections::HashMap::new();
        let mut idx = 1usize;
        while idx < cp_count {
            let tag = r.read_u1()?;
            match tag {
                1 => {
                    let len = r.read_u2()? as usize;
                    let bytes = r.read_bytes(len)?;
                    utf8.insert(idx as u16, String::from_utf8_lossy(bytes).into_owned());
                }
                3 | 4 => r.skip(4)?,
                5 | 6 => {
                    r.skip(8)?;
                    idx += 1;
                }
                7 | 8 | 16 | 19 | 20 => r.skip(2)?,
                9 | 10 | 11 | 12 | 17 | 18 => r.skip(4)?,
                15 => r.skip(3)?,
                other => {
                    return Err(MinecraftError::InvalidMetadata(format!(
                        "Unsupported JVM constant pool tag: {other}"
                    )))
                }
            }
            idx += 1;
        }

        r.skip(6)?; // access_flags + this_class + super_class
        let interfaces_count = r.read_u2()? as usize;
        r.skip(interfaces_count * 2)?;

        let fields_count = r.read_u2()? as usize;
        read_members(&mut r, fields_count, None)?;

        let methods_count = r.read_u2()? as usize;
        let mut methods = Vec::with_capacity(methods_count);
        read_members(&mut r, methods_count, Some((&utf8, &mut methods)))?;
        Ok(methods)
    }
}

struct ClassReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> ClassReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        if self
            .offset
            .checked_add(len)
            .is_none_or(|end| end > self.data.len())
        {
            return Err(MinecraftError::InvalidMetadata(
                "Unexpected end of Java class file".to_string(),
            ));
        }
        let slice = &self.data[self.offset..self.offset + len];
        self.offset += len;
        Ok(slice)
    }

    fn skip(&mut self, len: usize) -> Result<()> {
        let _ = self.read_bytes(len)?;
        Ok(())
    }

    fn read_u1(&mut self) -> Result<u8> {
        Ok(self.read_bytes(1)?[0])
    }

    fn read_u2(&mut self) -> Result<u16> {
        let b = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn read_u4(&mut self) -> Result<u32> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

type MethodCollector<'a> = (
    &'a std::collections::HashMap<u16, String>,
    &'a mut Vec<(String, String)>,
);

fn read_members(
    r: &mut ClassReader<'_>,
    count: usize,
    mut collect: Option<MethodCollector<'_>>,
) -> Result<()> {
    for _ in 0..count {
        r.skip(2)?; // access_flags
        let name_idx = r.read_u2()?;
        let desc_idx = r.read_u2()?;
        let attrs = r.read_u2()? as usize;
        if let Some((utf8, ref mut out)) = collect {
            let name = utf8.get(&name_idx).cloned().unwrap_or_default();
            let desc = utf8.get(&desc_idx).cloned().unwrap_or_default();
            out.push((name, desc));
        }
        for _ in 0..attrs {
            r.skip(2)?;
            let attr_len = r.read_u4()? as usize;
            r.skip(attr_len)?;
        }
    }
    Ok(())
}

fn find_mapping_jar(
    libs_dir: &Path,
    version: &VersionMetadata,
    coord_prefix: &str,
) -> Option<PathBuf> {
    for lib in &version.libraries {
        if !lib.name.trim().starts_with(coord_prefix) {
            continue;
        }
        if let Some(rel) = lib
            .downloads
            .as_ref()
            .and_then(|d| d.artifact.as_ref())
            .and_then(|a| a.path.as_ref())
        {
            if let Ok(p) = LauncherPaths::safe_join_relative(libs_dir, rel) {
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        let parts: Vec<&str> = lib.name.trim().split(':').collect();
        if parts.len() >= 3 {
            let mut p = libs_dir.to_path_buf();
            for seg in parts[0].split('.') {
                p.push(seg);
            }
            p.push(parts[1]);
            p.push(parts[2]);
            let classifier = parts.get(3).copied().unwrap_or("");
            let suffix = if classifier.is_empty() {
                String::new()
            } else {
                format!("-{classifier}")
            };
            p.push(format!("{}-{}{}.jar", parts[1], parts[2], suffix));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn forge_mcp_version(version: &VersionMetadata) -> Option<String> {
    let args = version.arguments.as_ref()?;
    let mut flat = Vec::new();
    for entry in &args.game {
        match entry {
            ArgumentEntry::Simple(s) => flat.push(s.as_str()),
            ArgumentEntry::Conditional { value, .. } => flat.extend(value.as_vec()),
        }
    }
    for flag in ["--fml.neoFormVersion", "--fml.mcpVersion"] {
        for pair in flat.windows(2) {
            if pair[0] == flag && !pair[1].trim().is_empty() {
                return Some(pair[1].trim().to_string());
            }
        }
    }
    None
}

fn normalize_internal(v: &str) -> String {
    v.trim().replace('.', "/")
}

fn parse_release_version(v: &str) -> Option<(u32, u32, u32)> {
    let re = Regex::new(r"^(\d+)\.(\d+)(?:\.(\d+))?$").ok()?;
    let caps = re.captures(v.trim())?;
    Some((
        caps.get(1)?.as_str().parse().ok()?,
        caps.get(2)?.as_str().parse().ok()?,
        caps.get(3)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0),
    ))
}

fn sanitize_version_component(v: &str) -> String {
    let clean: String = v
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = clean.trim_matches('-');
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

fn deduplicate_targets(targets: Vec<LanAgentTarget>) -> Vec<LanAgentTarget> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in targets {
        if seen.insert((t.class_name.clone(), t.method_name.clone())) {
            out.push(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mojang_client_mappings_for_minecraft_server() {
        let mappings = r#"
# comment
net.minecraft.server.MinecraftServer -> alx:
    boolean isDedicatedServer() -> l
    12:15:void setUsesAuthentication(boolean) -> e
"#;
        let target = LanAgentTargetResolver::parse_mojang_client_mappings(mappings).unwrap();
        assert_eq!(target.namespace, "official");
        assert_eq!(target.class_name, "alx");
        assert_eq!(target.method_name, "e");
        assert_eq!(target.encoded(), "alx#e");
    }

    #[test]
    fn parses_tiny_v1_and_v2_mappings() {
        let off = LanAgentTarget {
            namespace: "official".to_string(),
            class_name: "alx".to_string(),
            method_name: "e".to_string(),
        };

        let tiny_v1 = "v1\tofficial\tintermediary\nCLASS\talx\tnet/minecraft/class_1132\nMETHOD\talx\t(Z)V\te\tmethod_3772\n";
        let res1 = LanAgentTargetResolver::parse_tiny_mappings(
            tiny_v1,
            &off,
            "intermediary",
            "intermediary",
        )
        .unwrap();
        assert_eq!(res1.class_name, "net/minecraft/class_1132");
        assert_eq!(res1.method_name, "method_3772");

        let tiny_v2 = "tiny\t2\t0\tofficial\thashed\nc\talx\tnet/minecraft/unmapped/C_9999\n\tm\t(Z)V\te\tm_9876\n";
        let res2 =
            LanAgentTargetResolver::parse_tiny_mappings(tiny_v2, &off, "hashed", "quilt-hashed")
                .unwrap();
        assert_eq!(res2.class_name, "net/minecraft/unmapped/C_9999");
        assert_eq!(res2.method_name, "m_9876");
    }

    #[test]
    fn parses_jvm_class_method_table_bytes() {
        // Minimal valid classfile (JDK 8, version 52.0) with 1 method: `m_1234_(Z)V`
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xCAFE_BABEu32.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes()); // minor
        bytes.extend_from_slice(&52u16.to_be_bytes()); // major
        bytes.extend_from_slice(&3u16.to_be_bytes()); // cp_count = 3 (indices 1, 2)
                                                      // #1 UTF8 "m_1234_"
        bytes.push(1);
        bytes.extend_from_slice(&7u16.to_be_bytes());
        bytes.extend_from_slice(b"m_1234_");
        // #2 UTF8 "(Z)V"
        bytes.push(1);
        bytes.extend_from_slice(&4u16.to_be_bytes());
        bytes.extend_from_slice(b"(Z)V");

        bytes.extend_from_slice(&[0u8; 6]); // access + this + super
        bytes.extend_from_slice(&0u16.to_be_bytes()); // interfaces
        bytes.extend_from_slice(&0u16.to_be_bytes()); // fields
        bytes.extend_from_slice(&1u16.to_be_bytes()); // methods_count = 1
        bytes.extend_from_slice(&0u16.to_be_bytes()); // access
        bytes.extend_from_slice(&1u16.to_be_bytes()); // name_idx = 1
        bytes.extend_from_slice(&2u16.to_be_bytes()); // desc_idx = 2
        bytes.extend_from_slice(&0u16.to_be_bytes()); // attrs = 0

        let methods = LanAgentTargetResolver::parse_class_method_table(&bytes).unwrap();
        assert_eq!(methods, vec![("m_1234_".to_string(), "(Z)V".to_string())]);
    }
}
