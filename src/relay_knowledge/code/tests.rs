use super::*;
use crate::domain::CodeIndexResourceBudget;
use std::fs;

use super::changes::{GitChange, parse_name_status_z, tracked_entries};
use super::git::git_batch_blobs;
use super::test_fixtures::{TempGitRepo, reference, symbol};

#[test]
fn detects_supported_languages_and_filters_paths() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec!["src".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let trailing_slash_selector = CodeRepositorySelector::new(
        "alias",
        "HEAD",
        vec!["src/".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("selector should validate");

    assert_eq!(language_id("src/lib.rs"), Some("rust"));
    assert_eq!(language_id("src/app.py"), Some("python"));
    assert_eq!(language_id("src/app.js"), Some("javascript"));
    assert_eq!(language_id("src/app.jsx"), Some("jsx"));
    assert_eq!(language_id("src/app.ts"), Some("typescript"));
    assert_eq!(language_id("src/app.tsx"), Some("tsx"));
    assert_eq!(language_id("src/app.go"), Some("go"));
    assert_eq!(language_id("src/App.java"), Some("java"));
    assert_eq!(language_id("src/App.kt"), Some("kotlin"));
    assert_eq!(language_id("src/App.scala"), Some("scala"));
    assert_eq!(language_id("src/app.c"), Some("c"));
    assert_eq!(language_id("include/app.h"), Some("c"));
    assert_eq!(language_id("src/app.cpp"), Some("cpp"));
    assert_eq!(language_id("include/app.hpp"), Some("cpp"));
    assert_eq!(language_id("src/App.cs"), Some("csharp"));
    assert_eq!(language_id("src/app.rb"), Some("ruby"));
    assert_eq!(language_id("Gemfile"), Some("ruby"));
    assert_eq!(language_id("src/app.php"), Some("php"));
    assert_eq!(language_id("src/App.swift"), Some("swift"));
    assert_eq!(language_id("scripts/app.sh"), Some("bash"));
    assert_eq!(language_id(".bashrc"), Some("bash"));
    assert!(path_is_selected("src/lib.rs", &registration, &selector));
    assert!(path_is_selected(
        "src/lib.rs",
        &registration,
        &trailing_slash_selector
    ));
    assert!(!path_is_selected("tests/lib.rs", &registration, &selector));
    assert!(!path_is_selected("src/app.py", &registration, &selector));

    let file_filter_selector = CodeRepositorySelector::new(
        "alias",
        "HEAD",
        vec!["src/generated/temp.rs".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("selector should validate");
    assert!(!path_scope_allows(
        "src/generated",
        &registration,
        &file_filter_selector
    ));
    assert!(path_scope_overlaps(
        "src/generated",
        &registration,
        &file_filter_selector
    ));
}

#[test]
fn selector_filters_cannot_widen_registered_scope() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec!["src".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("registration should validate");
    let wider_path_selector =
        CodeRepositorySelector::new("alias", "HEAD", vec!["tests".to_owned()], Vec::new())
            .expect("selector should validate");
    let wider_language_selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["python".to_owned()])
            .expect("selector should validate");

    assert!(!path_is_selected(
        "tests/lib.rs",
        &registration,
        &wider_path_selector
    ));
    assert!(!path_is_selected(
        "src/app.py",
        &registration,
        &wider_language_selector
    ));
}

#[test]
fn language_scoped_selection_keeps_dependency_manifests() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let rust_selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let javascript_selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["javascript".to_owned()])
            .expect("selector should validate");
    let python_selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["python".to_owned()])
            .expect("selector should validate");
    let java_selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["java".to_owned()])
            .expect("selector should validate");
    let cpp_selector =
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), vec!["cpp".to_owned()])
            .expect("selector should validate");

    assert!(path_is_selected(
        "Cargo.toml",
        &registration,
        &rust_selector
    ));
    assert!(path_is_selected(
        "Cargo.lock",
        &registration,
        &rust_selector
    ));
    assert!(!path_is_selected(
        "package.json",
        &registration,
        &rust_selector
    ));
    assert!(path_is_selected(
        "package-lock.json",
        &registration,
        &javascript_selector
    ));
    assert!(path_is_selected(
        "requirements/base.txt",
        &registration,
        &python_selector
    ));
    assert!(path_is_selected(
        "constraints.txt",
        &registration,
        &python_selector
    ));
    assert!(path_is_selected("pom.xml", &registration, &java_selector));
    assert!(path_is_selected(
        "build.gradle",
        &registration,
        &java_selector
    ));
    assert!(path_is_selected(
        "conanfile.py",
        &registration,
        &cpp_selector
    ));
}

#[test]
fn dot_path_filter_selects_repository_root() {
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
    let selector_dot =
        CodeRepositorySelector::new("alias", "HEAD", vec!["./".to_owned()], Vec::new())
            .expect("selector should validate");
    let selector_relative =
        CodeRepositorySelector::new("alias", "HEAD", vec!["./src".to_owned()], Vec::new())
            .expect("selector should validate");

    assert!(path_is_selected("src/lib.rs", &registration, &selector));
    assert!(path_is_selected("README.md", &registration, &selector));
    assert!(path_is_selected("src/lib.rs", &registration, &selector_dot));
    assert!(path_is_selected(
        "src/lib.rs",
        &registration,
        &selector_relative
    ));
}

#[test]
fn explicit_default_exclusion_opt_in_stays_path_scoped() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec![".".to_owned(), "dist".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    assert!(path_is_selected("dist/bundle.js", &registration, &selector));
    assert!(!path_is_selected(
        "target/generated.rs",
        &registration,
        &selector
    ));
}

#[test]
fn git_batch_blobs_reads_multiple_commit_files() {
    let repo = TempGitRepo::create("batch-blobs");
    repo.write("src/alpha.rs", "pub fn alpha() {}\n");
    repo.write("src/beta.rs", "pub fn beta() {\n    alpha();\n}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);

    let blobs = git_batch_blobs(
        &repo.path,
        &commit,
        &["src/alpha.rs".to_owned(), "src/beta.rs".to_owned()],
    )
    .expect("batch blobs should load");

    assert_eq!(blobs[0], b"pub fn alpha() {}\n");
    assert_eq!(blobs[1], b"pub fn beta() {\n    alpha();\n}\n");
}

#[test]
fn tracked_entries_include_blob_sizes_for_batch_planning() {
    let repo = TempGitRepo::create("tracked-entry-sizes");
    repo.write("src/alpha.rs", "fn alpha() {}\n");
    repo.write("src/beta.rs", "fn beta() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);

    let entries = tracked_entries(&repo.path, &commit).expect("entries should load");

    assert!(entries.iter().any(|entry| {
        entry.path == "src/alpha.rs" && entry.byte_count == "fn alpha() {}\n".len()
    }));
    assert!(entries.iter().any(|entry| {
        entry.path == "src/beta.rs" && entry.byte_count == "fn beta() {}\n".len()
    }));
}

#[test]
fn tracked_entries_skip_gitlink_submodules() {
    let repo = TempGitRepo::create("tracked-entry-gitlinks");
    repo.write("src/lib.rs", "fn alpha() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);
    repo.git([
        "update-index",
        "--add",
        "--cacheinfo",
        "160000",
        commit.as_str(),
        "vendor/module",
    ]);
    repo.git(["commit", "-m", "add gitlink"]);
    let head = repo.git_text(["rev-parse", "HEAD"]);

    let entries = tracked_entries(&repo.path, &head).expect("entries should load");

    assert!(entries.iter().any(|entry| entry.path == "src/lib.rs"));
    assert!(!entries.iter().any(|entry| entry.path == "vendor/module"));
}

#[test]
fn full_index_plan_stops_batch_before_next_blob_exceeds_byte_budget() {
    let repo = TempGitRepo::create("byte-budget-fetch");
    repo.write("src/a.rs", "fn a() {}\n");
    repo.write("src/b.rs", "fn b() {}\n");
    repo.write("src/c.rs", "fn c() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let budget = CodeIndexResourceBudget::new(128, "fn a() {}\nfn b() {}\n".len(), 50_000)
        .expect("budget should validate");
    let plan = prepare_full_index_plan(repo.registration(), repo.selector(), budget)
        .expect("plan should prepare");

    let (plan, first_batch) = plan.parse_next_batch().expect("first batch should parse");
    let (plan, second_batch) = plan.parse_next_batch().expect("second batch should parse");
    let (_, third_batch) = plan.parse_next_batch().expect("third batch should parse");

    let first_batch = first_batch.expect("first batch should exist");
    let second_batch = second_batch.expect("second batch should exist");
    assert!(third_batch.is_none());
    assert_eq!(first_batch.files.len(), 2);
    assert_eq!(first_batch.files[0].path, "src/a.rs");
    assert_eq!(first_batch.files[1].path, "src/b.rs");
    assert_eq!(second_batch.files.len(), 1);
    assert_eq!(second_batch.files[0].path, "src/c.rs");
}

#[test]
fn full_index_plan_preserves_order_across_bounded_parallel_parse_chunks() {
    let repo = TempGitRepo::create("parallel-fetch-order");
    for index in 0..40 {
        repo.write(
            &format!("src/file_{index:02}.rs"),
            &format!("fn f_{index}() {{}}\n"),
        );
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let budget =
        CodeIndexResourceBudget::new(40, 1024 * 1024, 50_000).expect("budget should validate");
    let plan = prepare_full_index_plan(repo.registration(), repo.selector(), budget)
        .expect("plan should prepare");

    let (_, batch) = plan.parse_next_batch().expect("batch should parse");
    let batch = batch.expect("batch should exist");

    assert_eq!(batch.files.len(), 40);
    for (index, file) in batch.files.iter().enumerate() {
        assert_eq!(file.path, format!("src/file_{index:02}.rs"));
    }
}

#[test]
fn explicit_default_exclusion_opt_in_supports_dataset_paths() {
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec!["data/events.jsonl".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    assert!(path_is_selected(
        "data/events.jsonl",
        &registration,
        &selector
    ));
    assert!(!path_is_selected(
        "other/events.jsonl",
        &registration,
        &selector
    ));
}

#[test]
fn anchored_ignore_rules_only_match_repo_root_paths() {
    let repo = TempGitRepo::create("anchored-ignore-rules");
    repo.write(".relay-knowledgeignore", "/docs\n");
    repo.write("docs/root.rs", "fn root() {}\n");
    repo.write("src/docs/nested.rs", "fn nested() {}\n");

    let registration = repo.registration();
    let selector = repo.selector();

    assert!(!path_is_selected("docs/root.rs", &registration, &selector));
    assert!(path_is_selected(
        "src/docs/nested.rs",
        &registration,
        &selector
    ));
}

#[test]
fn incremental_deletions_survive_tighter_ignore_rules() {
    let repo = TempGitRepo::create("incremental-tightened-ignore");
    repo.write("src/lib.rs", "fn kept() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);

    repo.write(".relay-knowledgeignore", "src\n");
    fs::remove_file(repo.path.join("src/lib.rs")).expect("source file should delete");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "tighten ignore and delete"]);

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::incremental(base, "HEAD").expect("incremental mode should validate"),
        Vec::new(),
    )
    .expect("incremental delete should index");

    assert_eq!(snapshot.deleted_paths, ["src/lib.rs"]);
}

#[test]
fn repository_id_includes_local_root_with_remote_origin() {
    let first = TempGitRepo::create("repo-id-first");
    let second = TempGitRepo::create("repo-id-second");
    first.git([
        "remote",
        "add",
        "origin",
        "https://example.invalid/repo.git",
    ]);
    second.git([
        "remote",
        "add",
        "origin",
        "https://example.invalid/repo.git",
    ]);

    let first_registration =
        register_repository(&first.path, "first", Vec::new(), Vec::new()).expect("first repo");
    let second_registration =
        register_repository(&second.path, "second", Vec::new(), Vec::new()).expect("second repo");

    assert_ne!(
        first_registration.repository_id,
        second_registration.repository_id
    );
}

#[test]
fn blank_repository_alias_defaults_to_git_root_directory_name() {
    let repo = TempGitRepo::create("project-default-alias");
    let nested = repo.path.join("src");
    let expected_alias = repo
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("fixture root should have a directory name")
        .to_owned();

    let registration =
        register_repository(nested, "   ", Vec::new(), Vec::new()).expect("repo should register");

    assert_eq!(registration.alias, expected_alias);
    assert_eq!(registration.root_path, repo.path.display().to_string());
}

#[test]
fn register_repository_rejects_language_filters() {
    let repo = TempGitRepo::create("register-language-filter");
    repo.write("src/lib.rs", "fn value() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);

    let error = register_repository(&repo.path, "fixture", Vec::new(), vec!["rust".to_owned()])
        .expect_err("registration language filters should be rejected");

    assert!(
        error
            .to_string()
            .contains(REGISTRATION_LANGUAGE_FILTER_ERROR)
    );
}

#[test]
fn diff_refs_reject_dash_prefixed_values() {
    let repo = TempGitRepo::create("dash-ref");
    repo.write("src/lib.rs", "fn value() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);

    let error = changed_paths_for_diff(&repo.path, "--cached", "HEAD")
        .expect_err("dash-prefixed refs should be rejected");

    assert!(error.to_string().contains("must not start"));
}

#[test]
fn parses_git_name_status_for_rename_copy_and_delete() {
    let changes =
        parse_name_status_z(b"M\0src/lib.rs\0R100\0old.rs\0new.rs\0C100\0a.py\0b.py\0D\0gone.ts\0")
            .expect("name-status should parse");

    assert_eq!(
        changes,
        vec![
            GitChange::AddedOrModified {
                path: "src/lib.rs".to_owned()
            },
            GitChange::Renamed {
                old_path: "old.rs".to_owned(),
                new_path: "new.rs".to_owned()
            },
            GitChange::Copied {
                old_path: "a.py".to_owned(),
                new_path: "b.py".to_owned()
            },
            GitChange::Deleted {
                path: "gone.ts".to_owned()
            }
        ]
    );
}

#[test]
fn worktree_status_uses_destination_path_for_renames_and_copies() {
    let paths = worktree_changed_paths(
        b"R  src/new.rs\0src/old.rs\0C  src/copied.rs\0src/source.rs\0 M src/lib.rs\0",
    );

    assert_eq!(paths[0].path, "src/new.rs");
    assert_eq!(paths[0].deleted_source.as_deref(), Some("src/old.rs"));
    assert_eq!(paths[1].path, "src/copied.rs");
    assert_eq!(paths[1].deleted_source, None);
    assert_eq!(paths[2].path, "src/lib.rs");
    assert_eq!(paths[2].deleted_source, None);
}

#[test]
fn repository_ids_include_local_checkout_identity() {
    let first = TempGitRepo::create("repo-id-first");
    let second = TempGitRepo::create("repo-id-second");
    first.git([
        "config",
        "remote.origin.url",
        "https://example.invalid/repo.git",
    ]);
    second.git([
        "config",
        "remote.origin.url",
        "https://example.invalid/repo.git",
    ]);

    let first = register_repository(&first.path, "first", Vec::new(), Vec::new())
        .expect("first repository should register");
    let second = register_repository(&second.path, "second", Vec::new(), Vec::new())
        .expect("second repository should register");

    assert_ne!(first.repository_id, second.repository_id);
}

#[test]
fn rejects_dash_prefixed_git_refs_before_diff_execution() {
    let repo = TempGitRepo::create("dash-ref");
    repo.write("src/lib.rs", "fn value() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);

    let error = changed_paths_for_diff(&repo.path, "--cached", "HEAD")
        .expect_err("dash-prefixed refs should be rejected");

    assert!(error.to_string().contains("base_ref"));
}

#[test]
fn impact_paths_for_copies_only_include_destination() {
    let paths = impact_paths_from_changes(vec![GitChange::Copied {
        old_path: "src/source.rs".to_owned(),
        new_path: "src/copied.rs".to_owned(),
    }]);

    assert_eq!(paths, ["src/copied.rs"]);
}

#[test]
fn incremental_deletions_are_limited_to_selected_scope() {
    let repo = TempGitRepo::create("incremental-delete-scope");
    repo.write("src/lib.rs", "fn kept() {}\n");
    repo.write("docs/out.rs", "fn out_of_scope() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    fs::remove_file(repo.path.join("docs/out.rs")).expect("out-of-scope file should delete");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "delete docs"]);

    let snapshot = build_index_snapshot(
        &repo.registration(),
        &repo.selector(),
        CodeIndexMode::incremental(base, "HEAD").expect("incremental mode should validate"),
        Vec::new(),
    )
    .expect("incremental delete should index");

    assert!(snapshot.deleted_paths.is_empty());
}

#[test]
fn deleted_symbol_names_are_extracted_from_base_diff() {
    let repo = TempGitRepo::create("deleted-symbol-seeds");
    repo.write("src/lib.rs", "fn removed_api() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    fs::remove_file(repo.path.join("src/lib.rs")).expect("source file should delete");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "delete api"]);

    let names =
        deleted_symbol_names_for_diff(&repo.registration(), &repo.selector(), &base, "HEAD")
            .expect("deleted symbols should parse");

    assert_eq!(names, ["removed_api"]);
}

#[test]
fn scope_preview_reports_default_and_ignore_exclusions() {
    let repo = TempGitRepo::create("scope-preview");
    repo.write("src/lib.rs", "fn kept() {}\n");
    repo.write("dist/bundle.js", "function generated() {}\n");
    repo.write("data/events.jsonl", "{\"kind\":\"fixture\"}\n");
    repo.write("docs/notes.rs", "fn ignored() {}\n");
    repo.write("manual.pdf", "%PDF-1.7\n");
    repo.write("uv.lock", "version = 1\n");
    repo.write(".relay-knowledgeignore", "docs\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        vec![".".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    let preview = preview_repository_scope(&registration, &selector).expect("preview should build");

    assert_eq!(preview.selected_file_count, 2);
    assert!(
        preview
            .language_distribution
            .iter()
            .any(|language| language.language_id == "rust")
    );
    assert!(
        preview
            .language_distribution
            .iter()
            .any(|language| language.language_id == "python")
    );
    assert!(preview.excluded_paths.iter().any(|path| {
        path.path == "dist/bundle.js" && path.reason == "excluded by source preset"
    }));
    assert!(preview.excluded_paths.iter().any(|path| {
        path.path == "data/events.jsonl" && path.reason == "excluded by source preset"
    }));
    assert!(preview.excluded_paths.iter().any(|path| {
        path.path == "docs/notes.rs" && path.reason == "excluded by .relay-knowledgeignore"
    }));
}

#[test]
fn scope_preview_uses_ignore_rules_from_requested_commit() {
    let repo = TempGitRepo::create("scope-preview-commit-ignore");
    repo.write("src/lib.rs", "fn kept() {}\n");
    repo.write("docs/notes.rs", "fn ignored() {}\n");
    repo.write(".relay-knowledgeignore", "docs\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    repo.write(".relay-knowledgeignore", "");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        vec![".".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    let preview = preview_repository_scope(&registration, &selector).expect("preview should build");

    assert_eq!(preview.selected_file_count, 1);
    assert!(preview.excluded_paths.iter().any(|path| {
        path.path == "docs/notes.rs" && path.reason == "excluded by .relay-knowledgeignore"
    }));
}

#[test]
fn impact_path_partition_uses_ignore_rules_from_selected_commit() {
    let repo = TempGitRepo::create("impact-partition-commit-ignore");
    repo.write("src/lib.rs", "fn kept() {}\n");
    repo.write("docs/notes.rs", "fn ignored() {}\n");
    repo.write(".relay-knowledgeignore", "docs\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    repo.write(".relay-knowledgeignore", "");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        vec![".".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    let groups = partition_changed_paths_for_selector(
        &registration,
        &selector,
        vec!["src/lib.rs".to_owned(), "docs/notes.rs".to_owned()],
    )
    .expect("paths should partition");

    assert_eq!(groups.in_scope_changed_paths, ["src/lib.rs"]);
    assert_eq!(groups.out_of_scope_changed_paths, ["docs/notes.rs"]);
}

#[test]
fn scope_preview_counts_each_degraded_file_once() {
    let repo = TempGitRepo::create("scope-preview-degraded-count");
    repo.write("docs/large.custom", &"x".repeat(512 * 1024 + 1));
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        vec![".".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let selector = CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");

    let preview = preview_repository_scope(&registration, &selector).expect("preview should build");

    assert_eq!(preview.selected_file_count, 1);
    assert_eq!(preview.unsupported_file_count, 1);
    assert_eq!(preview.generated_or_heavy_file_count, 1);
    assert_eq!(preview.expected_degraded_file_count, 1);
}

#[test]
fn impact_path_partition_uses_effective_scope() {
    let repo = TempGitRepo::create("impact-path-groups");
    repo.write("src/lib.rs", "fn kept() {}\n");
    repo.write("dist/bundle.js", "function generated() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);

    let groups = partition_changed_paths_for_selector(
        &repo.registration(),
        &repo.selector(),
        vec!["src/lib.rs".to_owned(), "dist/bundle.js".to_owned()],
    )
    .expect("paths should partition");

    assert_eq!(groups.in_scope_changed_paths, ["src/lib.rs"]);
    assert_eq!(groups.out_of_scope_changed_paths, ["dist/bundle.js"]);
}

#[test]
fn reference_resolution_prefers_same_path_and_leaves_ambiguous_names_unresolved() {
    let symbols = vec![
        symbol("sym-a", "src/a.rs", "run"),
        symbol("sym-b", "src/b.rs", "run"),
    ];
    let mut references = vec![
        reference("ref-a", "src/a.rs", "run"),
        reference("ref-c", "src/c.rs", "run"),
    ];

    resolve_reference_targets(&symbols, &mut references);

    assert_eq!(
        references[0].target_symbol_snapshot_id.as_deref(),
        Some("sym-a")
    );
    assert_eq!(references[1].target_symbol_snapshot_id, None);
}
