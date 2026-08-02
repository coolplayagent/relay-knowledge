use crate::{env::UpdateEnvOverrides, project::GITHUB_REPOSITORY_FULL_NAME};

// Direct tests for update runtime configuration.

use super::*;

#[test]
fn configured_sources_accept_aliases_and_remove_duplicates() {
    let sources =
        parse_update_sources(Some("github,crates,crates.io")).expect("sources should parse");

    assert_eq!(sources, vec![UpdateSource::Github, UpdateSource::CratesIo]);
}

#[test]
fn invalid_source_lists_and_github_repository_names_are_rejected() {
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
fn disabled_checks_ignore_unused_source_and_repository_overrides() {
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
