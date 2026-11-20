/// Direct 1:1 port of `src/core/java/jvm_presets.py` from MCW Launcher Python.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JvmPresetId {
    #[default]
    Default,
    Aikar,
    Zgc,
    Shenandoah,
    Custom,
}

impl JvmPresetId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Aikar => "aikar",
            Self::Zgc => "zgc",
            Self::Shenandoah => "shenandoah",
            Self::Custom => "custom",
        }
    }

    pub fn i18n_key(&self) -> &'static str {
        match self {
            Self::Default => "jvm_preset.default",
            Self::Aikar => "jvm_preset.aikar",
            Self::Zgc => "jvm_preset.zgc",
            Self::Shenandoah => "jvm_preset.shenandoah",
            Self::Custom => "jvm_preset.custom",
        }
    }

    pub fn parse(id: &str) -> Self {
        match id.trim().to_ascii_lowercase().as_str() {
            "" | "default" => Self::Default,
            "aikar" => Self::Aikar,
            "zgc" => Self::Zgc,
            "shenandoah" => Self::Shenandoah,
            _ => Self::Custom,
        }
    }
}

pub const AIKAR_FLAGS: &[&str] = &[
    "-XX:+UseG1GC",
    "-XX:+ParallelRefProcEnabled",
    "-XX:MaxGCPauseMillis=200",
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:+DisableExplicitGC",
    "-XX:+AlwaysPreTouch",
    "-XX:G1NewSizePercent=30",
    "-XX:G1MaxNewSizePercent=40",
    "-XX:G1ReservePercent=20",
    "-XX:G1HeapWastePercent=5",
    "-XX:G1MixedGCCountTarget=4",
    "-XX:InitiatingHeapOccupancyPercent=15",
    "-XX:G1MixedGCLiveThresholdPercent=90",
    "-XX:G1RSetUpdatingPauseTimePercent=5",
    "-XX:SurvivorRatio=32",
    "-XX:+PerfDisableSharedMem",
    "-XX:MaxTenuringThreshold=1",
];

pub const ZGC_FLAGS: &[&str] = &[
    "-XX:+UseZGC",
    "-XX:+ZGenerational",
    "-XX:+AlwaysPreTouch",
    "-XX:+DisableExplicitGC",
];

pub const SHENANDOAH_FLAGS: &[&str] = &[
    "-XX:+UseShenandoahGC",
    "-XX:+AlwaysPreTouch",
    "-XX:+DisableExplicitGC",
    "-XX:ShenandoahGCHeuristics=adaptive",
];

pub fn get_preset_flags(preset: JvmPresetId) -> Vec<String> {
    let slice: &[&str] = match preset {
        JvmPresetId::Default | JvmPresetId::Custom => &[],
        JvmPresetId::Aikar => AIKAR_FLAGS,
        JvmPresetId::Zgc => ZGC_FLAGS,
        JvmPresetId::Shenandoah => SHENANDOAH_FLAGS,
    };
    slice.iter().map(|s| (*s).to_string()).collect()
}

pub fn detect_preset(flags: &[String]) -> JvmPresetId {
    let normalized: Vec<&str> = flags
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if normalized.is_empty() {
        return JvmPresetId::Default;
    }
    if normalized.as_slice() == AIKAR_FLAGS {
        return JvmPresetId::Aikar;
    }
    if normalized.as_slice() == ZGC_FLAGS {
        return JvmPresetId::Zgc;
    }
    if normalized.as_slice() == SHENANDOAH_FLAGS {
        return JvmPresetId::Shenandoah;
    }
    JvmPresetId::Custom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_and_detects_all_jvm_presets() {
        assert_eq!(detect_preset(&[]), JvmPresetId::Default);
        for preset in [
            JvmPresetId::Aikar,
            JvmPresetId::Zgc,
            JvmPresetId::Shenandoah,
        ] {
            let flags = get_preset_flags(preset);
            assert_eq!(detect_preset(&flags), preset);
        }
        assert_eq!(
            detect_preset(&["-XX:+UseSerialGC".to_string()]),
            JvmPresetId::Custom
        );
    }
}
