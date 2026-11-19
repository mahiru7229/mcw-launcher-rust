use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrereleaseKind {
    Alpha = 0,
    Beta = 1,
    Rc = 2,
    Final = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub prerelease: PrereleaseKind,
    pub prerelease_num: u32,
}

impl LauncherVersion {
    pub fn parse(raw: &str) -> Option<Self> {
        let cleaned = raw.trim().trim_start_matches(['v', 'V']);
        let (core, pre) = match cleaned.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (cleaned, None),
        };
        let mut nums = core.split('.');
        let major: u32 = nums.next()?.parse().ok()?;
        let minor: u32 = nums.next().unwrap_or("0").parse().ok()?;
        let patch: u32 = nums.next().unwrap_or("0").parse().ok()?;

        let (prerelease, prerelease_num) = match pre {
            None => (PrereleaseKind::Final, 0),
            Some(p) => {
                let lower = p.to_ascii_lowercase();
                let kind = if lower.starts_with("alpha") {
                    PrereleaseKind::Alpha
                } else if lower.starts_with("beta") {
                    PrereleaseKind::Beta
                } else if lower.starts_with("rc") {
                    PrereleaseKind::Rc
                } else {
                    PrereleaseKind::Alpha
                };
                let num = lower
                    .split(['.', '-', '_'])
                    .filter_map(|part| part.parse::<u32>().ok())
                    .next()
                    .unwrap_or(0);
                (kind, num)
            }
        };

        Some(Self {
            major,
            minor,
            patch,
            prerelease,
            prerelease_num,
        })
    }

    pub fn is_prerelease(&self) -> bool {
        self.prerelease != PrereleaseKind::Final
    }
}

impl Ord for LauncherVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (
            self.major,
            self.minor,
            self.patch,
            self.prerelease,
            self.prerelease_num,
        )
            .cmp(&(
                other.major,
                other.minor,
                other.patch,
                other.prerelease,
                other.prerelease_num,
            ))
    }
}

impl PartialOrd for LauncherVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_and_prerelease_versions_correctly() {
        let a1 = LauncherVersion::parse("v0.3.0-alpha.1").unwrap();
        let b1 = LauncherVersion::parse("0.3.0-beta.1").unwrap();
        let rc1 = LauncherVersion::parse("0.3.0-rc.1").unwrap();
        let rel = LauncherVersion::parse("v0.3.0").unwrap();
        let v187 = LauncherVersion::parse("v1.8.7").unwrap();

        assert!(a1 < b1);
        assert!(b1 < rc1);
        assert!(rc1 < rel);
        assert!(rel < v187);
        assert!(a1.is_prerelease());
        assert!(!rel.is_prerelease());
    }
}
