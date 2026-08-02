use std::{cmp::Ordering, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StableVersion {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: bool,
}

impl StableVersion {
    pub(super) const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self::from_parts(major, minor, patch, false)
    }

    pub(super) const fn prerelease(major: u64, minor: u64, patch: u64) -> Self {
        Self::from_parts(major, minor, patch, true)
    }

    const fn from_parts(major: u64, minor: u64, patch: u64, prerelease: bool) -> Self {
        Self {
            major,
            minor,
            patch,
            prerelease,
        }
    }
}

impl Ord for StableVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (
            self.major,
            self.minor,
            self.patch,
            release_precedence(self.prerelease),
        )
            .cmp(&(
                other.major,
                other.minor,
                other.patch,
                release_precedence(other.prerelease),
            ))
    }
}

impl PartialOrd for StableVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for StableVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

const fn release_precedence(prerelease: bool) -> u8 {
    if prerelease { 0 } else { 1 }
}

pub(super) fn stable_version(value: &str) -> Result<StableVersion, String> {
    let trimmed = value.trim().trim_start_matches('v');
    if trimmed.split('+').next().unwrap_or(trimmed).contains('-') {
        return Err(format!("release version '{value}' is a prerelease"));
    }
    comparable_version(value)
}

fn comparable_version(value: &str) -> Result<StableVersion, String> {
    let trimmed = value.trim().trim_start_matches('v');
    let without_build = trimmed.split('+').next().unwrap_or(trimmed);
    let prerelease = without_build.contains('-');
    let core = trimmed
        .split('+')
        .next()
        .unwrap_or(trimmed)
        .split('-')
        .next()
        .unwrap_or(trimmed);
    let mut parts = core.split('.');
    let Some(major) = parts.next() else {
        return Err(format!("release version '{value}' is not semver"));
    };
    let Some(minor) = parts.next() else {
        return Err(format!("release version '{value}' is not semver"));
    };
    let Some(patch) = parts.next() else {
        return Err(format!("release version '{value}' is not semver"));
    };
    if parts.next().is_some() {
        return Err(format!("release version '{value}' is not semver"));
    }

    let major = parse_version_component(value, major)?;
    let minor = parse_version_component(value, minor)?;
    let patch = parse_version_component(value, patch)?;
    if prerelease {
        Ok(StableVersion::prerelease(major, minor, patch))
    } else {
        Ok(StableVersion::new(major, minor, patch))
    }
}

fn parse_version_component(value: &str, component: &str) -> Result<u64, String> {
    if component.is_empty()
        || !component
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err(format!("release version '{value}' is not semver"));
    }

    component
        .parse::<u64>()
        .map_err(|_| format!("release version '{value}' is not semver"))
}

pub(super) fn current_version() -> StableVersion {
    comparable_version(env!("CARGO_PKG_VERSION")).expect("Cargo package version must be semver")
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
