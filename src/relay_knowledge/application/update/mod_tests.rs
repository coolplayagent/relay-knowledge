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
    let raw_response = serde_json::to_vec(&sample_version_response(env!("CARGO_PKG_VERSION"), 100))
        .expect("sample response should serialize");

    assert!(serde_json::from_slice::<VersionCheckCache>(&raw_response).is_err());
}

fn sample_version_response(current_version: &str, checked_at_unix_ms: u64) -> VersionCheckResponse {
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
