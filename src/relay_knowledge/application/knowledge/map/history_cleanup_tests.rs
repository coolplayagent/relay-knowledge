use super::*;

pub(super) async fn history_file_contents(directory: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut entries = fs::read_dir(directory)
        .await
        .expect("history directory should read");
    let mut names = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .expect("history entry should read")
    {
        names.push((
            entry.file_name().to_string_lossy().into_owned(),
            fs::read(entry.path())
                .await
                .expect("history artifact should read"),
        ));
    }
    names.sort();
    names
}

#[tokio::test]
async fn history_cleanup_rejects_unknown_entries_before_deleting_generated_files() {
    let root = temp_root("history-cleanup-unknown");
    let directory = root
        .join(AGENT_CONTRACT_DIR_NAME)
        .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME);
    fs::create_dir_all(&directory)
        .await
        .expect("history directory should create");
    let generated = directory.join(format!("{:020}-{:020}-{}.yaml", 1, 1, "a".repeat(64)));
    fs::write(&generated, "legacy archive")
        .await
        .expect("generated artifact should write");
    fs::write(directory.join("operator-notes.txt"), "preserve")
        .await
        .expect("unknown file should write");

    let error = cleanup_history_artifacts_in(&root, AGENT_CONTRACT_DIR_NAME)
        .await
        .expect_err("unknown files must stop cleanup");
    assert!(matches!(error, KnowledgeMapServiceError::Integrity(_)));
    assert!(fs::try_exists(generated).await.unwrap());
    assert!(
        fs::try_exists(directory.join("operator-notes.txt"))
            .await
            .unwrap()
    );
    let _ = fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn history_cleanup_rejects_a_symlinked_directory_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let root = temp_root("history-cleanup-symlink");
    let outside = temp_root("history-cleanup-outside");
    fs::create_dir_all(root.join(AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("contract directory should create");
    fs::create_dir_all(&outside)
        .await
        .expect("outside directory should create");
    fs::write(outside.join("preserve.txt"), "preserve")
        .await
        .expect("outside file should write");
    symlink(
        &outside,
        root.join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME),
    )
    .expect("history symlink should create");

    let error = cleanup_history_artifacts_in(&root, AGENT_CONTRACT_DIR_NAME)
        .await
        .expect_err("symlinked history directory must be rejected");
    assert!(matches!(error, KnowledgeMapServiceError::UnsafePath(_)));
    assert!(fs::try_exists(outside.join("preserve.txt")).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
    let _ = fs::remove_dir_all(outside).await;
}

#[tokio::test]
async fn history_cleanup_is_bounded_and_resumes_on_the_next_attempt() {
    let root = temp_root("history-cleanup-batches");
    let directory = root
        .join(AGENT_CONTRACT_DIR_NAME)
        .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME);
    fs::create_dir_all(&directory)
        .await
        .expect("history directory should create");
    for version in 1..=1_025_u64 {
        let path = directory.join(format!(
            "{version:020}-{version:020}-{}.yaml",
            "a".repeat(64)
        ));
        fs::write(path, "legacy archive")
            .await
            .expect("history artifact should write");
    }

    let progress = cleanup_history_artifacts_in(&root, AGENT_CONTRACT_DIR_NAME)
        .await
        .expect("the first cleanup attempt should stop without failing its caller");
    assert_eq!(
        progress,
        HistoryCleanupStatus::Pending {
            removed: HISTORY_CLEANUP_ENTRY_LIMIT
        }
    );
    assert_eq!(history_file_contents(&directory).await.len(), 1);
    let completed = cleanup_history_artifacts_in(&root, AGENT_CONTRACT_DIR_NAME)
        .await
        .expect("the next attempt should finish cleanup");
    assert_eq!(completed, HistoryCleanupStatus::Complete);
    assert!(!fs::try_exists(directory).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn committed_map_mutation_succeeds_when_history_cleanup_needs_another_batch() {
    let root = temp_root("history-cleanup-after-commit");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("map should initialize");
    let directory = root
        .join(AGENT_CONTRACT_DIR_NAME)
        .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME);
    fs::create_dir_all(&directory)
        .await
        .expect("history directory should create");
    for version in 1..=1_025_u64 {
        fs::write(
            directory.join(format!(
                "{version:020}-{version:020}-{}.yaml",
                "a".repeat(64)
            )),
            "legacy archive",
        )
        .await
        .expect("history artifact should write");
    }

    service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "post-cleanup-source".to_owned(),
                topic: "build".to_owned(),
                kind: KnowledgeMapSourceKind::Config,
                uri: "build/post-cleanup.toml".to_owned(),
                source_scope: Some("repo".to_owned()),
                description: None,
            },
        )
        .await
        .expect("a committed mutation must not fail because cleanup remains pending");
    assert_eq!(history_file_contents(&directory).await.len(), 1);
    assert!(
        service
            .show(&context, None)
            .await
            .expect("committed map should remain readable")
            .map
            .sources
            .iter()
            .any(|source| source.id == "post-cleanup-source")
    );
    service
        .init(&context)
        .await
        .expect("the next maintenance attempt should finish cleanup");
    assert!(!fs::try_exists(directory).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn validation_rejects_a_dangling_history_symlink() {
    use std::os::unix::fs::symlink;

    let root = temp_root("history-validation-dangling-symlink");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("map should initialize");
    symlink(
        root.join("missing-history-target"),
        root.join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME),
    )
    .expect("dangling history symlink should create");

    let validation = service
        .validate(&context)
        .await
        .expect("validation should run");
    assert!(!validation.valid);
    assert!(
        validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("unsafe knowledge map artifact path"))
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn current_init_removes_recognized_legacy_namespace_history_artifacts() {
    let root = temp_root("legacy-namespace-history-cleanup");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("map should initialize");
    let directory = root
        .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
        .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME);
    fs::create_dir_all(&directory)
        .await
        .expect("legacy history directory should create");
    fs::write(
        directory.join(format!("{:020}-{:020}-{}.yaml", 1, 1, "a".repeat(64))),
        "obsolete archive",
    )
    .await
    .expect("legacy history artifact should write");

    service
        .init(&context)
        .await
        .expect("idempotent init should clean the legacy namespace");
    assert!(!fs::try_exists(directory).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn show_rejects_explicit_empty_archive_fields_in_a_recent_only_manifest() {
    let root = temp_root("invalid-show-empty-archive-fields");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    let content = fs::read_to_string(service.map_path())
        .await
        .expect("manifest should read");
    let content = content.replacen(
        "history:\n",
        "history:\n  archived_through: 0\n  archive: null\n  index: null\n",
        1,
    );
    fs::write(service.map_path(), content)
        .await
        .expect("manifest should write");

    let error = service
        .show(&context, None)
        .await
        .expect_err("show must reject even empty archive fields in schema v4");
    assert!(
        error
            .to_string()
            .contains("must not reference archive artifacts")
    );
    let _ = fs::remove_dir_all(root).await;
}
