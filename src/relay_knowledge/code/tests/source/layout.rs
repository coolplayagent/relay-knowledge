use super::test_fixtures::TempGitRepo;
use super::*;
use crate::domain::{CodeIndexResourceBudget, CodeParseStatus};

#[test]
fn full_index_plan_discovers_nonstandard_source_roots_without_path_filters() {
    let repo = TempGitRepo::create("full-index-source-discovery");
    repo.write("src/lib.rs", "pub fn local_entry() {}\n");
    repo.write("build/workflow.yaml", "steps:\n  - cargo test\n");
    repo.write(".cloudbuild/cloudbuild.yaml", "steps:\n  - name: test\n");
    repo.write(".cid/pipeline.yml", "jobs:\n  test: cargo test\n");
    repo.write(".build_config/settings.toml", "profile = \"ci\"\n");
    repo.write(
        "external_deps/rust_sdk/lib.rs",
        "pub fn external_session_client() {}\n",
    );
    repo.write(
        "modules/java_sdk/src/main/java/example/ExternalJavaSessionClient.java",
        "package example;\npublic class ExternalJavaSessionClient {}\n",
    );
    repo.write("vendor/pkg/lib.rs", "pub fn vendored_dependency() {}\n");
    repo.write(
        "third_party/pkg/lib.rs",
        "pub fn third_party_dependency() {}\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        Vec::new(),
        Vec::new(),
    )
    .expect("registration should validate");
    let mut plan = prepare_full_index_plan(
        registration,
        repo.selector(),
        CodeIndexResourceBudget::default(),
    )
    .expect("plan should prepare");
    let mut paths = Vec::new();
    let mut symbol_names = Vec::new();
    loop {
        let (next_plan, batch) = plan.parse_next_batch().expect("batch should parse");
        plan = next_plan;
        let Some(batch) = batch else {
            break;
        };
        paths.extend(batch.files.into_iter().map(|file| file.path));
        symbol_names.extend(batch.symbols.into_iter().map(|symbol| symbol.name));
    }

    assert!(paths.iter().any(|path| path == "src/lib.rs"));
    assert!(
        paths
            .iter()
            .any(|path| path == "external_deps/rust_sdk/lib.rs")
    );
    assert!(paths.iter().any(|path| {
        path == "modules/java_sdk/src/main/java/example/ExternalJavaSessionClient.java"
    }));
    for path in [
        "build/workflow.yaml",
        ".cloudbuild/cloudbuild.yaml",
        ".cid/pipeline.yml",
        ".build_config/settings.toml",
        "vendor/pkg/lib.rs",
        "third_party/pkg/lib.rs",
    ] {
        assert!(paths.iter().any(|indexed| indexed == path), "{path}");
    }
    assert!(
        symbol_names
            .iter()
            .any(|name| name == "external_session_client")
    );
    assert!(
        symbol_names
            .iter()
            .any(|name| name == "ExternalJavaSessionClient")
    );
    assert!(
        symbol_names
            .iter()
            .any(|name| name == "vendored_dependency")
    );
    assert!(
        symbol_names
            .iter()
            .any(|name| name == "third_party_dependency")
    );
}

#[test]
fn full_index_plan_extends_default_src_scope_with_discovered_source_roots() {
    let repo = TempGitRepo::create("full-index-source-discovery-src-scope");
    repo.write("src/lib.rs", "pub fn local_entry() {}\n");
    repo.write(
        "external_deps/rust_sdk/lib.rs",
        "pub fn external_session_client() {}\n",
    );
    repo.write("tests/helper.rs", "pub fn test_helper() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        vec!["src".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");

    let plan = prepare_full_index_plan(
        registration,
        repo.selector(),
        CodeIndexResourceBudget::default(),
    )
    .expect("plan should prepare");
    let session = plan.session();
    let (_, batch) = plan.parse_next_batch().expect("batch should parse");
    let batch = batch.expect("batch should exist");
    let paths = batch
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();

    assert!(session.path_filters.contains(&"src".to_owned()));
    assert!(
        session
            .path_filters
            .contains(&"external_deps/rust_sdk".to_owned())
    );
    assert!(paths.contains(&"src/lib.rs"));
    assert!(paths.contains(&"external_deps/rust_sdk/lib.rs"));
    assert!(!paths.contains(&"tests/helper.rs"));
}

#[test]
fn incremental_index_discovers_new_nonstandard_source_roots() {
    let repo = TempGitRepo::create("incremental-source-discovery");
    repo.write("src/lib.rs", "pub fn local_entry() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        Vec::new(),
        Vec::new(),
    )
    .expect("registration should validate");
    let base_snapshot = build_index_snapshot(
        &registration,
        &repo.selector(),
        CodeIndexMode::Full,
        Vec::new(),
    )
    .expect("base index should build");
    let previous_hashes = base_snapshot
        .files
        .iter()
        .map(|file| CodeFileFingerprint {
            path: file.path.clone(),
            blob_hash: file.blob_hash.clone(),
        })
        .collect::<Vec<_>>();
    repo.write(
        "external_deps/rust_sdk/lib.rs",
        "pub fn incremental_external_session_client() {}\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "add external source"]);

    let snapshot = build_index_snapshot(
        &registration,
        &repo.selector(),
        CodeIndexMode::incremental(base, "HEAD").expect("incremental mode should validate"),
        previous_hashes,
    )
    .expect("incremental index should build");

    assert!(snapshot.files.iter().any(|file| {
        file.path == "external_deps/rust_sdk/lib.rs" && file.parse_status == CodeParseStatus::Parsed
    }));
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "incremental_external_session_client")
    );
}

#[test]
fn incremental_index_extends_default_src_scope_for_new_source_roots() {
    let repo = TempGitRepo::create("incremental-source-discovery-src-scope");
    repo.write("src/lib.rs", "pub fn local_entry() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        repo.path.display().to_string(),
        vec!["src".to_owned()],
        Vec::new(),
    )
    .expect("registration should validate");
    let base_snapshot = build_index_snapshot(
        &registration,
        &repo.selector(),
        CodeIndexMode::Full,
        Vec::new(),
    )
    .expect("base index should build");
    let previous_hashes = base_snapshot
        .files
        .iter()
        .map(|file| CodeFileFingerprint {
            path: file.path.clone(),
            blob_hash: file.blob_hash.clone(),
        })
        .collect::<Vec<_>>();
    repo.write(
        "external_deps/rust_sdk/lib.rs",
        "pub fn incremental_external_session_client() {}\n",
    );
    repo.write("tests/helper.rs", "pub fn test_helper() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "add external source"]);

    let snapshot = build_index_snapshot(
        &registration,
        &repo.selector(),
        CodeIndexMode::incremental(base, "HEAD").expect("incremental mode should validate"),
        previous_hashes,
    )
    .expect("incremental index should build");

    assert!(snapshot.path_filters.contains(&"src".to_owned()));
    assert!(
        snapshot
            .path_filters
            .contains(&"external_deps/rust_sdk".to_owned())
    );
    assert!(snapshot.files.iter().any(|file| {
        file.path == "external_deps/rust_sdk/lib.rs" && file.parse_status == CodeParseStatus::Parsed
    }));
    assert!(
        !snapshot
            .files
            .iter()
            .any(|file| file.path == "tests/helper.rs")
    );
}
