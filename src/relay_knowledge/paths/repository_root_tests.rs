use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

#[test]
fn root_search_walks_up_to_git_marker() {
    let root = temp_root("git-marker");
    let nested = root.join("src").join("module");
    fs::create_dir_all(root.join(".git")).expect("git marker should create");
    fs::create_dir_all(&nested).expect("nested dir should create");

    let discovered = discover_repository_root(&nested)
        .expect("search should succeed")
        .expect("root should be found");

    assert_eq!(discovered, root);
    let _ = fs::remove_dir_all(discovered);
}

#[test]
fn root_search_walks_up_to_knowledge_contract_directory() {
    let root = temp_root("knowledge-marker");
    let nested = root.join("docs").join("architecture");
    fs::create_dir_all(root.join(crate::project::AGENT_CONTRACT_DIR_NAME))
        .expect("knowledge directory should create");
    fs::write(
        root.join(crate::project::KNOWLEDGE_MAP_RELATIVE_PATH),
        "schema_version: 3",
    )
    .expect("knowledge marker should write");
    fs::create_dir_all(&nested).expect("nested dir should create");

    let discovered = discover_repository_root(&nested)
        .expect("search should succeed")
        .expect("root should be found");

    assert_eq!(discovered, root);
    let _ = fs::remove_dir_all(discovered);
}

#[test]
fn root_search_falls_back_to_nearest_agents_file() {
    let root = temp_root("agents-marker");
    let nested = root.join("src").join("module");
    fs::create_dir_all(&nested).expect("nested dir should create");
    fs::write(
        root.join("AGENTS.md"),
        "Knowledge map: knowledge/knowledge-map.yaml",
    )
    .expect("agents should write");

    let discovered = discover_repository_root(&nested)
        .expect("search should succeed")
        .expect("root should be found");

    assert_eq!(discovered, root);
    let _ = fs::remove_dir_all(discovered);
}

#[test]
fn nested_agents_file_fallback_keeps_nearest_scope() {
    let root = temp_root("nested-agents-marker");
    let scoped = root.join("src");
    let nested = scoped.join("module");
    fs::create_dir_all(&nested).expect("nested dir should create");
    fs::write(root.join("AGENTS.md"), "Workspace instructions.").expect("root agents write");
    fs::write(scoped.join("AGENTS.md"), "Scoped instructions.").expect("scoped agents write");

    let discovered = discover_repository_root(&nested)
        .expect("search should succeed")
        .expect("root should be found");

    assert_eq!(discovered, scoped);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scoped_agents_file_does_not_override_git_root() {
    let root = temp_root("scoped-agents");
    let nested = root.join("src").join("module");
    fs::create_dir_all(root.join(".git")).expect("git marker should create");
    fs::create_dir_all(&nested).expect("nested dir should create");
    fs::write(nested.join("AGENTS.md"), "Scoped instructions.")
        .expect("scoped agents should write");

    let discovered = discover_repository_root(&nested)
        .expect("search should succeed")
        .expect("root should be found");

    assert_eq!(discovered, root);
    let _ = fs::remove_dir_all(discovered);
}

#[test]
fn missing_markers_return_none() {
    let root = temp_root("missing-marker");
    let nested = root.join("src");
    fs::create_dir_all(&nested).expect("nested dir should create");

    let discovered = discover_repository_root(&nested).expect("search should succeed");

    assert_eq!(discovered, None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_start_directory_returns_error() {
    let root = temp_root("missing-start");
    let missing = root.join("missing");
    fs::create_dir_all(&root).expect("root should create");

    let error = discover_repository_root(&missing).expect_err("missing start should fail");

    assert!(matches!(
        error,
        RepositoryRootDiscoveryError::StartUnavailable { .. }
    ));
    let _ = fs::remove_dir_all(root);
}

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "relay-knowledge-root-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos()
    ))
}
