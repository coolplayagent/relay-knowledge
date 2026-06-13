use std::{
    cmp::Ordering,
    error::Error,
    fmt,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

mod http_fetch;

use reqwest::{StatusCode, header};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    env::{RELAY_KNOWLEDGE_UPDATE_GITHUB_REPO, RELAY_KNOWLEDGE_UPDATE_SOURCES, UpdateEnvOverrides},
    net::{
        NetworkRuntime, http,
        qos::{QosPolicy, QosRuntime},
    },
    paths::RuntimePaths,
    project::{GITHUB_REPOSITORY_FULL_NAME, PROJECT_NAME},
};
use http_fetch::qos_transport_diagnostic;

pub const DEFAULT_UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const VERSION_CHECK_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

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

/// Machine-readable result for `relay-knowledge version check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionCheckResponse {
    pub project_name: String,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub source: Option<String>,
    pub release_url: Option<String>,
    pub checked_at_unix_ms: u64,
    pub diagnostics: Vec<VersionCheckDiagnostic>,
}

/// Source-specific version-check diagnostic safe for CLI output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionCheckDiagnostic {
    pub source: Option<String>,
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VersionCheckCache {
    cache_key: String,
    response: VersionCheckResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseCandidate {
    source: UpdateSource,
    version: StableVersion,
    release_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StableVersion {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: bool,
}

impl StableVersion {
    const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self::from_parts(major, minor, patch, false)
    }

    const fn prerelease(major: u64, minor: u64, patch: u64) -> Self {
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

pub async fn check_for_updates(
    paths: &RuntimePaths,
    network: &NetworkRuntime,
    config: &UpdateRuntimeConfig,
    force_refresh: bool,
) -> VersionCheckResponse {
    let now_ms = current_time_millis();
    let cache_path = paths.version_check_cache_file();
    if !force_refresh
        && let Some(cached) =
            read_fresh_cache(&cache_path, now_ms, config.check_interval, config).await
    {
        return cached;
    }

    let response = fetch_latest_version(network, config, now_ms).await;
    let _ = write_cache(&cache_path, &response, config).await;
    response
}

pub async fn update_notice(
    paths: &RuntimePaths,
    network: &NetworkRuntime,
    config: &UpdateRuntimeConfig,
) -> Option<String> {
    if !config.enabled {
        return None;
    }
    let response = check_for_updates(paths, network, config, false).await;
    if !response.update_available {
        return None;
    }

    Some(format!(
        "{} {} is available; current {}. Run `relay-knowledge version check` for details.\n",
        PROJECT_NAME,
        response
            .latest_version
            .unwrap_or_else(|| "unknown".to_owned()),
        response.current_version
    ))
}

async fn fetch_latest_version(
    network: &NetworkRuntime,
    config: &UpdateRuntimeConfig,
    checked_at_unix_ms: u64,
) -> VersionCheckResponse {
    let current_version = current_version();
    let network_config = network.current();
    let client = match http::outbound_json_client(&network_config.http) {
        Ok(client) => client,
        Err(error) => {
            return response_from_candidates(
                current_version,
                Vec::new(),
                vec![diagnostic(
                    None,
                    "client_build_failed",
                    error.to_string(),
                    false,
                )],
                checked_at_unix_ms,
            );
        }
    };

    let mut candidates = Vec::new();
    let mut diagnostics = Vec::new();
    let max_response_bytes = network_config.http.max_request_body_bytes;
    let qos = network.qos_runtime();
    for source in &config.sources {
        match fetch_source(
            &client,
            &qos,
            &network_config.qos,
            config,
            *source,
            max_response_bytes,
        )
        .await
        {
            Ok(candidate) => candidates.push(candidate),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }

    response_from_candidates(current_version, candidates, diagnostics, checked_at_unix_ms)
}

async fn fetch_source(
    client: &reqwest::Client,
    qos: &QosRuntime,
    policy: &QosPolicy,
    config: &UpdateRuntimeConfig,
    source: UpdateSource,
    max_response_bytes: u64,
) -> Result<ReleaseCandidate, VersionCheckDiagnostic> {
    match source {
        UpdateSource::Github => {
            fetch_github_release(client, qos, policy, &config.github_repo, max_response_bytes).await
        }
        UpdateSource::CratesIo => {
            fetch_crates_release(client, qos, policy, max_response_bytes).await
        }
    }
}

async fn fetch_github_release(
    client: &reqwest::Client,
    qos: &QosRuntime,
    policy: &QosPolicy,
    repo: &str,
    max_response_bytes: u64,
) -> Result<ReleaseCandidate, VersionCheckDiagnostic> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let response = send_json_request(client, qos, policy, &url)
        .await
        .map_err(|error| qos_transport_diagnostic(UpdateSource::Github, error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(status_diagnostic(UpdateSource::Github, status));
    }

    let payload = read_json_response::<GithubLatestRelease>(
        response,
        UpdateSource::Github,
        max_response_bytes,
    )
    .await?;
    github_candidate(payload)
}

async fn fetch_crates_release(
    client: &reqwest::Client,
    qos: &QosRuntime,
    policy: &QosPolicy,
    max_response_bytes: u64,
) -> Result<ReleaseCandidate, VersionCheckDiagnostic> {
    let url = format!("https://crates.io/api/v1/crates/{PROJECT_NAME}");
    let response = send_json_request(client, qos, policy, &url)
        .await
        .map_err(|error| qos_transport_diagnostic(UpdateSource::CratesIo, error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(status_diagnostic(UpdateSource::CratesIo, status));
    }

    let payload = read_json_response::<CratesPackageResponse>(
        response,
        UpdateSource::CratesIo,
        max_response_bytes,
    )
    .await?;
    crates_candidate(payload)
}

async fn read_json_response<T>(
    response: http::QosHttpResponse,
    source: UpdateSource,
    max_response_bytes: u64,
) -> Result<T, VersionCheckDiagnostic>
where
    T: DeserializeOwned,
{
    if response
        .content_length()
        .is_some_and(|length| length > max_response_bytes)
    {
        return Err(response_body_too_large_diagnostic(
            source,
            max_response_bytes,
        ));
    }

    let max_response_bytes = max_response_bytes.try_into().unwrap_or(usize::MAX);
    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| transport_diagnostic(source, error))?
    {
        append_limited_response_body(source, &mut body, &chunk, max_response_bytes)?;
    }

    serde_json::from_slice(&body).map_err(|error| {
        diagnostic(
            Some(source),
            "invalid_response_json",
            error.to_string(),
            false,
        )
    })
}

fn append_limited_response_body(
    source: UpdateSource,
    body: &mut Vec<u8>,
    chunk: &[u8],
    max_response_bytes: usize,
) -> Result<(), VersionCheckDiagnostic> {
    let Some(next_len) = body
        .len()
        .checked_add(chunk.len())
        .filter(|next_len| *next_len <= max_response_bytes)
    else {
        let max_response_bytes = max_response_bytes.try_into().unwrap_or(u64::MAX);
        return Err(response_body_too_large_diagnostic(
            source,
            max_response_bytes,
        ));
    };
    body.reserve(next_len.saturating_sub(body.len()));
    body.extend_from_slice(chunk);
    Ok(())
}

async fn send_json_request(
    client: &reqwest::Client,
    qos: &QosRuntime,
    policy: &QosPolicy,
    url: &str,
) -> Result<http::QosHttpResponse, http::QosHttpClientError> {
    http::send_request_with_qos(
        qos,
        policy,
        client
            .get(url)
            .header(
                header::USER_AGENT,
                format!("{PROJECT_NAME}/{}", env!("CARGO_PKG_VERSION")),
            )
            .timeout(VERSION_CHECK_REQUEST_TIMEOUT),
    )
    .await
}

#[derive(Debug, Deserialize)]
struct GithubLatestRelease {
    tag_name: String,
    html_url: String,
    prerelease: bool,
}

fn github_candidate(
    release: GithubLatestRelease,
) -> Result<ReleaseCandidate, VersionCheckDiagnostic> {
    if release.prerelease {
        return Err(diagnostic(
            Some(UpdateSource::Github),
            "prerelease_ignored",
            format!("GitHub release '{}' is a prerelease", release.tag_name),
            false,
        ));
    }
    let version = stable_version(&release.tag_name).map_err(|message| {
        diagnostic(
            Some(UpdateSource::Github),
            "invalid_version",
            message,
            false,
        )
    })?;

    Ok(ReleaseCandidate {
        source: UpdateSource::Github,
        version,
        release_url: release.html_url,
    })
}

#[derive(Debug, Deserialize)]
struct CratesPackageResponse {
    #[serde(rename = "crate")]
    package: CratesPackage,
}

#[derive(Debug, Deserialize)]
struct CratesPackage {
    max_stable_version: Option<String>,
}

fn crates_candidate(
    response: CratesPackageResponse,
) -> Result<ReleaseCandidate, VersionCheckDiagnostic> {
    let Some(max_stable_version) = response.package.max_stable_version else {
        return Err(diagnostic(
            Some(UpdateSource::CratesIo),
            "stable_version_unavailable",
            "crates.io response did not include a stable release version",
            false,
        ));
    };
    let version = stable_version(&max_stable_version).map_err(|message| {
        diagnostic(
            Some(UpdateSource::CratesIo),
            "invalid_version",
            message,
            false,
        )
    })?;

    Ok(ReleaseCandidate {
        source: UpdateSource::CratesIo,
        version,
        release_url: format!("https://crates.io/crates/{PROJECT_NAME}"),
    })
}

fn response_from_candidates(
    current_version: StableVersion,
    candidates: Vec<ReleaseCandidate>,
    diagnostics: Vec<VersionCheckDiagnostic>,
    checked_at_unix_ms: u64,
) -> VersionCheckResponse {
    let latest = candidates
        .into_iter()
        .max_by(|left, right| left.version.cmp(&right.version));
    let update_available = latest
        .as_ref()
        .is_some_and(|candidate| candidate.version > current_version);

    VersionCheckResponse {
        project_name: PROJECT_NAME.to_owned(),
        current_version: env!("CARGO_PKG_VERSION").to_owned(),
        latest_version: latest
            .as_ref()
            .map(|candidate| candidate.version.to_string()),
        update_available,
        source: latest
            .as_ref()
            .map(|candidate| candidate.source.as_str().to_owned()),
        release_url: latest
            .as_ref()
            .map(|candidate| candidate.release_url.clone()),
        checked_at_unix_ms,
        diagnostics,
    }
}

fn stable_version(value: &str) -> Result<StableVersion, String> {
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

fn current_version() -> StableVersion {
    comparable_version(env!("CARGO_PKG_VERSION")).expect("Cargo package version must be semver")
}

fn diagnostic(
    source: Option<UpdateSource>,
    code: impl Into<String>,
    message: impl Into<String>,
    retryable: bool,
) -> VersionCheckDiagnostic {
    VersionCheckDiagnostic {
        source: source.map(|value| value.as_str().to_owned()),
        code: code.into(),
        message: message.into(),
        retryable,
    }
}

fn transport_diagnostic(source: UpdateSource, error: reqwest::Error) -> VersionCheckDiagnostic {
    diagnostic(Some(source), "transport_failed", error.to_string(), true)
}

fn status_diagnostic(source: UpdateSource, status: StatusCode) -> VersionCheckDiagnostic {
    diagnostic(
        Some(source),
        "http_status",
        format!("release metadata request returned HTTP {}", status.as_u16()),
        status.is_server_error()
            || status == StatusCode::REQUEST_TIMEOUT
            || status == StatusCode::TOO_MANY_REQUESTS,
    )
}

fn response_body_too_large_diagnostic(
    source: UpdateSource,
    max_response_bytes: u64,
) -> VersionCheckDiagnostic {
    diagnostic(
        Some(source),
        "response_body_too_large",
        format!("release metadata response exceeded {max_response_bytes} bytes"),
        false,
    )
}

async fn read_fresh_cache(
    path: &Path,
    now_ms: u64,
    interval: Duration,
    config: &UpdateRuntimeConfig,
) -> Option<VersionCheckResponse> {
    let bytes = tokio::fs::read(path).await.ok()?;
    let cache = serde_json::from_slice::<VersionCheckCache>(&bytes).ok()?;
    if cache_is_usable(&cache, now_ms, interval, config) {
        Some(cache.response)
    } else {
        None
    }
}

fn cache_is_usable(
    cache: &VersionCheckCache,
    now_ms: u64,
    interval: Duration,
    config: &UpdateRuntimeConfig,
) -> bool {
    cache.cache_key == version_cache_key(config)
        && cache.response.current_version == env!("CARGO_PKG_VERSION")
        && cache_is_fresh(&cache.response, now_ms, interval)
}

fn cache_is_fresh(response: &VersionCheckResponse, now_ms: u64, interval: Duration) -> bool {
    now_ms
        .checked_sub(response.checked_at_unix_ms)
        .is_some_and(|age| age <= duration_millis(interval))
}

async fn write_cache(
    path: &Path,
    response: &VersionCheckResponse,
    config: &UpdateRuntimeConfig,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let cache = VersionCheckCache {
        cache_key: version_cache_key(config),
        response: response.clone(),
    };
    let bytes = serde_json::to_vec(&cache)?;
    tokio::fs::write(path, bytes).await
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

fn version_cache_key(config: &UpdateRuntimeConfig) -> String {
    let sources = config
        .sources
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<_>>()
        .join(",");
    format!("sources={sources};github_repo={}", config.github_repo)
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

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_configured_update_sources_with_aliases_and_deduplication() {
        let sources =
            parse_update_sources(Some("github,crates,crates.io")).expect("sources should parse");

        assert_eq!(sources, vec![UpdateSource::Github, UpdateSource::CratesIo]);
    }

    #[test]
    fn rejects_empty_update_sources_and_invalid_github_repo() {
        assert_eq!(
            parse_update_sources(Some("github,,crates")).expect_err("empty source should fail"),
            UpdateRuntimeConfigError::EmptySourceList
        );
        assert_eq!(
            validate_github_repo("relay-knowledge").expect_err("repo should require owner"),
            UpdateRuntimeConfigError::InvalidGithubRepo("relay-knowledge".to_owned())
        );
    }

    #[test]
    fn disabled_update_config_ignores_unused_source_and_repo_overrides() {
        let config = UpdateRuntimeConfig::from_environment(&UpdateEnvOverrides {
            enabled: Some(false),
            sources: Some("not-a-source".to_owned()),
            check_interval_ms: None,
            github_repo: Some("not-owner-repo".to_owned()),
        })
        .expect("disabled update checks should ignore unused source settings");

        assert!(!config.enabled);
        assert_eq!(
            config.sources,
            vec![UpdateSource::Github, UpdateSource::CratesIo]
        );
        assert_eq!(config.github_repo, GITHUB_REPOSITORY_FULL_NAME);
    }

    #[test]
    fn parses_stable_versions_and_rejects_prereleases() {
        assert_eq!(
            stable_version("v1.2.3").expect("version should parse"),
            StableVersion::new(1, 2, 3)
        );
        assert_eq!(
            comparable_version("1.2.3-rc.1").expect("current prerelease should compare"),
            StableVersion::prerelease(1, 2, 3)
        );
        assert!(StableVersion::new(1, 2, 3) > StableVersion::prerelease(1, 2, 3));
        assert!(stable_version("1.2.3-rc.1").is_err());
    }

    #[test]
    fn selects_highest_stable_candidate() {
        let response = response_from_candidates(
            StableVersion::new(1, 0, 4),
            vec![
                ReleaseCandidate {
                    source: UpdateSource::Github,
                    version: StableVersion::new(1, 0, 5),
                    release_url: "https://github.example/release".to_owned(),
                },
                ReleaseCandidate {
                    source: UpdateSource::CratesIo,
                    version: StableVersion::new(1, 0, 6),
                    release_url: "https://crates.example/release".to_owned(),
                },
            ],
            Vec::new(),
            42,
        );

        assert!(response.update_available);
        assert_eq!(response.latest_version, Some("1.0.6".to_owned()));
        assert_eq!(response.source, Some("crates.io".to_owned()));
    }

    #[test]
    fn prerelease_current_version_is_older_than_matching_stable_candidate() {
        let response = response_from_candidates(
            StableVersion::prerelease(1, 0, 5),
            vec![ReleaseCandidate {
                source: UpdateSource::Github,
                version: StableVersion::new(1, 0, 5),
                release_url: "https://github.example/release".to_owned(),
            }],
            Vec::new(),
            42,
        );

        assert!(response.update_available);
        assert_eq!(response.latest_version, Some("1.0.5".to_owned()));
    }

    #[test]
    fn parses_release_payloads_into_candidates() {
        let github = github_candidate(GithubLatestRelease {
            tag_name: "v1.2.3".to_owned(),
            html_url: "https://github.example/release".to_owned(),
            prerelease: false,
        })
        .expect("GitHub release should parse");
        let crates = crates_candidate(CratesPackageResponse {
            package: CratesPackage {
                max_stable_version: Some("1.2.4".to_owned()),
            },
        })
        .expect("crates release should parse");

        assert_eq!(github.version, StableVersion::new(1, 2, 3));
        assert_eq!(crates.version, StableVersion::new(1, 2, 4));
    }

    #[test]
    fn crates_candidate_uses_stable_version_field() {
        let crates = crates_candidate(CratesPackageResponse {
            package: CratesPackage {
                max_stable_version: Some("2.0.0".to_owned()),
            },
        })
        .expect("stable crates release should parse");
        let missing_stable = crates_candidate(CratesPackageResponse {
            package: CratesPackage {
                max_stable_version: None,
            },
        })
        .expect_err("missing stable version should be diagnostic");

        assert_eq!(crates.version, StableVersion::new(2, 0, 0));
        assert_eq!(missing_stable.code, "stable_version_unavailable");
    }

    #[test]
    fn response_body_limit_rejects_oversized_chunks() {
        let mut body = b"{}".to_vec();

        append_limited_response_body(UpdateSource::Github, &mut body, b"\n", 3)
            .expect("boundary-sized body should pass");
        let diagnostic = append_limited_response_body(UpdateSource::Github, &mut body, b"x", 3)
            .expect_err("body over the configured limit should fail");

        assert_eq!(diagnostic.code, "response_body_too_large");
    }

    #[test]
    fn cache_freshness_uses_interval_boundary() {
        let response = sample_version_response(env!("CARGO_PKG_VERSION"), 100);

        assert!(cache_is_fresh(&response, 200, Duration::from_millis(100)));
        assert!(!cache_is_fresh(&response, 201, Duration::from_millis(100)));
    }

    #[test]
    fn cache_usability_requires_current_binary_and_source_configuration() {
        let config = UpdateRuntimeConfig::from_environment(&UpdateEnvOverrides::default())
            .expect("default config should parse");
        let cache = VersionCheckCache {
            cache_key: version_cache_key(&config),
            response: sample_version_response(env!("CARGO_PKG_VERSION"), 100),
        };

        assert!(cache_is_usable(
            &cache,
            200,
            Duration::from_millis(100),
            &config
        ));

        let mut previous_binary_cache = cache.clone();
        previous_binary_cache.response.current_version = "0.0.1".to_owned();
        assert!(!cache_is_usable(
            &previous_binary_cache,
            200,
            Duration::from_millis(100),
            &config
        ));

        let mut changed_source_cache = cache;
        changed_source_cache.cache_key = "sources=crates.io;github_repo=example/repo".to_owned();
        assert!(!cache_is_usable(
            &changed_source_cache,
            200,
            Duration::from_millis(100),
            &config
        ));
    }

    #[test]
    fn cache_format_requires_configuration_key_wrapper() {
        let raw_response =
            serde_json::to_vec(&sample_version_response(env!("CARGO_PKG_VERSION"), 100))
                .expect("sample response should serialize");

        assert!(serde_json::from_slice::<VersionCheckCache>(&raw_response).is_err());
    }

    fn sample_version_response(
        current_version: &str,
        checked_at_unix_ms: u64,
    ) -> VersionCheckResponse {
        VersionCheckResponse {
            project_name: PROJECT_NAME.to_owned(),
            current_version: current_version.to_owned(),
            latest_version: Some("1.0.5".to_owned()),
            update_available: true,
            source: Some("github".to_owned()),
            release_url: None,
            checked_at_unix_ms,
            diagnostics: Vec::new(),
        }
    }
}
