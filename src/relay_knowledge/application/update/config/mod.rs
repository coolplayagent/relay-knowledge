use std::{error::Error, fmt, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    env::{RELAY_KNOWLEDGE_UPDATE_GITHUB_REPO, RELAY_KNOWLEDGE_UPDATE_SOURCES, UpdateEnvOverrides},
    project::GITHUB_REPOSITORY_FULL_NAME,
};

const DEFAULT_UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Supported upstream sources for release metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateSource {
    Github,
    CratesIo,
}

impl UpdateSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::CratesIo => "crates.io",
        }
    }

    fn parse(value: &str) -> Result<Self, UpdateRuntimeConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "github" | "github-releases" => Ok(Self::Github),
            "crates" | "crates.io" | "crates-io" => Ok(Self::CratesIo),
            other => Err(UpdateRuntimeConfigError::InvalidSource(other.to_owned())),
        }
    }
}

/// Runtime update-check policy resolved from environment and project defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRuntimeConfig {
    pub enabled: bool,
    pub sources: Vec<UpdateSource>,
    pub check_interval: Duration,
    pub github_repo: String,
}

impl UpdateRuntimeConfig {
    pub fn from_environment(
        overrides: &UpdateEnvOverrides,
    ) -> Result<Self, UpdateRuntimeConfigError> {
        let enabled = overrides.enabled.unwrap_or(true);
        let check_interval = Duration::from_millis(
            overrides
                .check_interval_ms
                .unwrap_or(duration_millis(DEFAULT_UPDATE_CHECK_INTERVAL)),
        );
        if !enabled {
            return Ok(Self {
                enabled,
                sources: default_update_sources(),
                check_interval,
                github_repo: GITHUB_REPOSITORY_FULL_NAME.to_owned(),
            });
        }

        Ok(Self {
            enabled,
            sources: parse_update_sources(overrides.sources.as_deref())?,
            check_interval,
            github_repo: validate_github_repo(
                overrides
                    .github_repo
                    .as_deref()
                    .unwrap_or(GITHUB_REPOSITORY_FULL_NAME),
            )?,
        })
    }
}

/// Update-check runtime configuration error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateRuntimeConfigError {
    EmptySourceList,
    InvalidSource(String),
    InvalidGithubRepo(String),
}

impl fmt::Display for UpdateRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySourceList => write!(
                formatter,
                "{RELAY_KNOWLEDGE_UPDATE_SOURCES} must include github or crates.io"
            ),
            Self::InvalidSource(value) => write!(
                formatter,
                "invalid {RELAY_KNOWLEDGE_UPDATE_SOURCES} value '{value}', expected github or crates.io"
            ),
            Self::InvalidGithubRepo(value) => write!(
                formatter,
                "{RELAY_KNOWLEDGE_UPDATE_GITHUB_REPO} must be owner/name, got '{value}'"
            ),
        }
    }
}

impl Error for UpdateRuntimeConfigError {}

pub(super) fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn parse_update_sources(
    value: Option<&str>,
) -> Result<Vec<UpdateSource>, UpdateRuntimeConfigError> {
    let Some(raw_sources) = value else {
        return Ok(default_update_sources());
    };
    let mut sources = Vec::new();
    for raw_source in raw_sources.split(',') {
        let trimmed = raw_source.trim();
        if trimmed.is_empty() {
            return Err(UpdateRuntimeConfigError::EmptySourceList);
        }
        let source = UpdateSource::parse(trimmed)?;
        if !sources.contains(&source) {
            sources.push(source);
        }
    }
    if sources.is_empty() {
        return Err(UpdateRuntimeConfigError::EmptySourceList);
    }

    Ok(sources)
}

fn default_update_sources() -> Vec<UpdateSource> {
    vec![UpdateSource::Github, UpdateSource::CratesIo]
}

fn validate_github_repo(value: &str) -> Result<String, UpdateRuntimeConfigError> {
    let trimmed = value.trim();
    let parts = trimmed.split('/').collect::<Vec<_>>();
    if parts.len() != 2
        || parts.iter().any(|part| part.is_empty())
        || trimmed.contains(char::is_whitespace)
    {
        return Err(UpdateRuntimeConfigError::InvalidGithubRepo(
            value.to_owned(),
        ));
    }

    Ok(trimmed.to_owned())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
