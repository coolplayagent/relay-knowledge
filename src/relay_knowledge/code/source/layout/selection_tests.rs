use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

use super::{
    RepositorySourceKind, SourceLayoutDiscovery, path_is_selected,
    selection_exclusion_reason_for_source, source_default_file_preset_excludes,
};

#[test]
fn source_preset_does_not_exclude_tracked_directory_names() {
    for path in [
        "build/workflow.yaml",
        ".cloudbuild/cloudbuild.yaml",
        ".cid/pipeline.yml",
        ".build_config/settings.toml",
        "dist/bundle.js",
        "frontend/dist/js/components/sidebar.js",
        "node_modules/pkg/dist/js/core/index.js",
        "target/generated.rs",
        "vendor/pkg/lib.rs",
        "third_party/pkg/lib.rs",
    ] {
        assert!(!source_default_file_preset_excludes(path), "{path}");
    }
}

#[test]
fn explicit_default_exclusion_opt_in_normalizes_extension_case() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec!["assets/logo.SVG".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    assert!(path_is_selected(
        "assets/logo.SVG",
        &registration,
        &selector
    ));
}

#[test]
fn default_file_preset_excludes_dataset_dumps_and_keeps_uv_lock_facts() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec![".".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    assert!(!path_is_selected(
        ".agent_teams/evals/datasets/swebench-verified-full.jsonl",
        &registration,
        &selector
    ));
    assert!(source_default_file_preset_excludes("uv.lock"));
    assert!(path_is_selected("uv.lock", &registration, &selector));
}

#[test]
fn git_tracked_directory_names_are_selected_without_opt_in() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec![".".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    for path in [
        "build/workflow.yaml",
        ".cloudbuild/cloudbuild.yaml",
        ".cid/pipeline.yml",
        ".build_config/settings.toml",
        "external_deps/python_sdk/session_client.py",
        "packages/ui/src/index.ts",
        "modules/java_sdk/src/main/java/example/SessionClient.java",
        "plugins/example.com/nonstandard/session/client.go",
        "Sources/SwiftSdk/SessionClient.swift",
        "lib/app/controller.rb",
        "vendor/pkg/session_client.py",
        "third_party/pkg/session_client.py",
    ] {
        assert!(path_is_selected(path, &registration, &selector), "{path}");
    }
}

#[test]
fn default_source_preset_keeps_file_extension_opt_in_scoped() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec![".".to_owned(), "manual.pdf".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    assert!(path_is_selected("manual.pdf", &registration, &selector));
    assert!(!path_is_selected("other.pdf", &registration, &selector));
}

#[test]
fn non_git_default_scope_rejects_broad_dependency_paths() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    assert_eq!(
        selection_exclusion_reason_for_source(
            "vendor/pkg/lib.rs",
            &registration,
            &selector,
            &SourceLayoutDiscovery::default(),
            RepositorySourceKind::FileSystem,
        ),
        Some("outside non-git default source whitelist".to_owned())
    );
    assert_eq!(
        selection_exclusion_reason_for_source(
            "vendor/pkg/lib.rs",
            &registration,
            &selector,
            &SourceLayoutDiscovery::default(),
            RepositorySourceKind::Git,
        ),
        None
    );
}
