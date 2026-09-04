use super::*;
use crate::application::knowledge::map::history::MISSING_HISTORY_INDEX_MESSAGE;

#[tokio::test]
async fn oldest_history_lookup_has_a_constant_read_bound_and_crosses_leaves() {
    let root = temp_root("bounded-history-index");
    fs::create_dir_all(
        root.join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME),
    )
    .await
    .expect("history directory should create");
    let service = KnowledgeMapService::new(root.clone());
    let mut previous = None;
    let mut index = None;
    for version in 1..=70 {
        let archive = KnowledgeMapHistoryArchive {
            schema_version: DIRECTORY_ARTIFACT_SCHEMA_VERSION,
            from_version: version,
            through_version: version,
            previous: previous.clone(),
            entries: vec![crate::domain::KnowledgeMapHistoryEntry {
                version,
                action: "fixture".to_owned(),
                actor: "test".to_owned(),
                summary: format!("History entry {version}"),
            }],
        };
        let yaml = serialize_yaml(&archive).expect("archive should serialize");
        let digest = content_digest(yaml.as_bytes());
        let relative =
            format!("{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/{version:020}-{version:020}-{digest}.yaml");
        publish_immutable(&root, &relative, yaml.as_bytes())
            .await
            .expect("archive should publish");
        let archive_ref = KnowledgeMapArchiveRef {
            r#ref: relative,
            digest,
        };
        index = Some(
            service
                .append_history_index(index, archive_ref.clone(), &archive)
                .await
                .expect("index append should work"),
        );
        previous = Some(archive_ref);
    }
    let index = index.expect("index should exist");
    assert_eq!(
        index.height, 1,
        "70 archives should require two index levels"
    );
    let manifest = KnowledgeMapManifest {
        schema_version: DIRECTORY_ARTIFACT_SCHEMA_VERSION,
        artifact_kind: Some("map".to_owned()),
        map_type: Some(crate::domain::RepositoryMapType::Knowledge),
        map_version: 71,
        updated_at: "fixture".to_owned(),
        directories: super::contracts::baseline_directories(
            crate::domain::RepositoryMapType::Knowledge,
        ),
        topics: Vec::new(),
        history: KnowledgeMapHistoryManifest {
            archived_through: 70,
            omitted_through: 0,
            archive: previous,
            index: Some(index.clone()),
            recent: vec![crate::domain::KnowledgeMapHistoryEntry {
                version: 71,
                action: "fixture".to_owned(),
                actor: "test".to_owned(),
                summary: "Recent entry".to_owned(),
            }],
        },
    };
    fs::write(
        service.map_path(),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");
    let (_, _, reads) = service
        .load_indexed_history_archive(&index, 1)
        .await
        .expect("oldest archive should load directly");
    assert!(reads <= history::MAX_HISTORY_LOOKUP_READS);
    assert_eq!(reads, usize::from(index.height) + 2);

    let page = service
        .history(
            &RequestContext::for_interface(crate::api::InterfaceKind::Cli),
            Some(32),
            3,
        )
        .await
        .expect("page should cross the balanced leaf boundary");
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.version)
            .collect::<Vec<_>>(),
        [32, 33, 34]
    );
    let _ = fs::remove_dir_all(root).await;
}

#[test]
fn balanced_prepend_shape_stays_logarithmic_past_two_full_levels() {
    #[derive(Clone)]
    enum Shape {
        Leaf(usize),
        Branch(Vec<Shape>),
    }
    fn prepend(node: &mut Shape) -> Option<Shape> {
        match node {
            Shape::Leaf(entries) => {
                *entries += 1;
                history::balanced_index_split(*entries).map(|split| {
                    let right = *entries - split;
                    *entries = split;
                    Shape::Leaf(right)
                })
            }
            Shape::Branch(children) => {
                if let Some(right) = prepend(&mut children[0]) {
                    children.insert(1, right);
                }
                history::balanced_index_split(children.len())
                    .map(|split| Shape::Branch(children.split_off(split)))
            }
        }
    }
    fn height(node: &Shape) -> u8 {
        match node {
            Shape::Leaf(_) => 0,
            Shape::Branch(children) => 1 + height(&children[0]),
        }
    }
    let mut root = Shape::Leaf(0);
    for _ in 0..=HISTORY_INDEX_FANOUT * HISTORY_INDEX_FANOUT {
        if let Some(right) = prepend(&mut root) {
            root = Shape::Branch(vec![root, right]);
        }
    }
    assert_eq!(height(&root), 2);
    assert!(usize::from(height(&root)) + 2 <= history::MAX_HISTORY_LOOKUP_READS);
}

#[tokio::test]
async fn map_validate_is_read_only_before_init_migrates_and_cleans_legacy_history() {
    let root = temp_root("legacy-v2-index-migration");
    fs::create_dir_all(&root).await.expect("root should create");
    fs::write(
        root.join("AGENTS.md"),
        format!("Knowledge map: {KNOWLEDGE_MAP_RELATIVE_PATH}"),
    )
    .await
    .expect("agents should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    for index in 0..RECENT_HISTORY_LIMIT {
        service
            .add_source(
                &context,
                KnowledgeMapSourceAddRequest {
                    id: format!("migration-source-{index}"),
                    topic: "migration".to_owned(),
                    kind: KnowledgeMapSourceKind::Config,
                    uri: format!("migration/{index}.toml"),
                    source_scope: Some("repo".to_owned()),
                    description: None,
                },
            )
            .await
            .expect("source should add");
    }
    let mut manifest = parse_manifest(
        &fs::read_to_string(service.map_path())
            .await
            .expect("manifest should read"),
    )
    .expect("manifest should parse");
    assert_eq!(manifest.history.omitted_through, 1);
    let archive = KnowledgeMapHistoryArchive {
        schema_version: DIRECTORY_ARTIFACT_SCHEMA_VERSION,
        from_version: 1,
        through_version: 1,
        previous: None,
        entries: vec![crate::domain::KnowledgeMapHistoryEntry {
            version: 1,
            action: "init".to_owned(),
            actor: "cli".to_owned(),
            summary: "Legacy history fixture".to_owned(),
        }],
    };
    let archive_yaml = serialize_yaml(&archive).expect("legacy archive should serialize");
    let archive_digest = content_digest(archive_yaml.as_bytes());
    let archive_ref = KnowledgeMapArchiveRef {
        r#ref: format!(
            "{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/{:020}-{:020}-{archive_digest}.yaml",
            archive.from_version, archive.through_version
        ),
        digest: archive_digest,
    };
    let history_directory = root
        .join(AGENT_CONTRACT_DIR_NAME)
        .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME);
    fs::create_dir_all(&history_directory)
        .await
        .expect("legacy history directory should create");
    fs::write(
        root.join(AGENT_CONTRACT_DIR_NAME).join(&archive_ref.r#ref),
        archive_yaml,
    )
    .await
    .expect("legacy archive should write");
    manifest.schema_version = DIRECTORY_ARTIFACT_SCHEMA_VERSION;
    manifest.history.archived_through = 1;
    manifest.history.omitted_through = 0;
    manifest.history.archive = Some(archive_ref.clone());
    manifest.history.index = None;
    fs::write(
        service.map_path(),
        serialize_yaml(&manifest).expect("legacy v2 manifest should serialize"),
    )
    .await
    .expect("legacy v2 manifest should write");

    let mut history_files = fs::read_dir(&history_directory)
        .await
        .expect("history directory should read");
    while let Some(entry) = history_files
        .next_entry()
        .await
        .expect("history entry should read")
    {
        if entry.file_name().to_string_lossy().starts_with("index-") {
            fs::remove_file(entry.path())
                .await
                .expect("legacy fixture index should remove");
        }
    }
    let root_before_validation = fs::read(service.map_path())
        .await
        .expect("legacy v2 root should read");
    let history_before_validation =
        super::history_cleanup_tests::history_file_contents(&history_directory).await;

    let validation = service
        .validate(&context)
        .await
        .expect("read-only validation should return diagnostics");
    assert!(!validation.valid);
    assert!(
        validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains(MISSING_HISTORY_INDEX_MESSAGE))
    );
    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("validated root should read"),
        root_before_validation,
        "map validate must not publish a migrated root"
    );
    assert_eq!(
        super::history_cleanup_tests::history_file_contents(&history_directory).await,
        history_before_validation,
        "map validate must not create or rewrite history artifacts"
    );

    let archive_path = root.join(AGENT_CONTRACT_DIR_NAME).join(&archive_ref.r#ref);
    let archive_content = fs::read(&archive_path)
        .await
        .expect("archive should read before corruption");
    fs::write(&archive_path, "corrupt archive").await.unwrap();
    let corrupted = service
        .validate(&context)
        .await
        .expect("validation should surface the blocking archive corruption");
    assert!(
        corrupted
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("digest mismatch")),
        "archive integrity must be checked before reporting a missing index"
    );
    assert!(
        !corrupted
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains(MISSING_HISTORY_INDEX_MESSAGE))
    );
    let migration_error = service
        .init(&context)
        .await
        .expect_err("migration must fail before deleting a corrupt archive");
    assert!(migration_error.to_string().contains("digest mismatch"));
    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("legacy root should remain readable"),
        root_before_validation
    );
    assert!(fs::try_exists(&archive_path).await.unwrap());
    fs::write(&archive_path, archive_content)
        .await
        .expect("archive should restore");

    let error = service
        .history(&context, Some(1), 1)
        .await
        .expect_err("history must not fall back to a reverse-chain scan");
    assert!(error.to_string().contains("relay-knowledge map init"));
    let migration = service.init(&context).await.expect("migration should work");
    assert!(migration.summary.contains("schema v4 recent-only history"));
    let migrated = parse_manifest(
        &fs::read_to_string(service.map_path())
            .await
            .expect("migrated manifest should read"),
    )
    .expect("migrated manifest should parse");
    assert_eq!(migrated.schema_version, ARTIFACT_SCHEMA_VERSION);
    assert_eq!(migrated.history.omitted_through, 2);
    assert!(migrated.history.index.is_none());
    assert!(migrated.history.archive.is_none());
    let fallback = parse_manifest(
        &fs::read_to_string(service.backup_path())
            .await
            .expect("reader fallback should remain available"),
    )
    .expect("reader fallback should parse");
    assert_eq!(fallback.schema_version, ARTIFACT_SCHEMA_VERSION);
    assert!(fallback.history.archive.is_none());
    assert!(
        !fs::try_exists(&history_directory)
            .await
            .expect("legacy history directory should be inspectable")
    );
    assert_eq!(
        service
            .history(&context, None, 1)
            .await
            .expect("retained history should work")
            .entries[0]
            .version,
        3
    );
    assert!(service.history(&context, Some(1), 1).await.is_err());
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn bounds_recent_history_without_creating_archive_artifacts() {
    let root = temp_root("history");
    fs::create_dir_all(&root).await.expect("root should create");
    fs::write(
        root.join("AGENTS.md"),
        format!("Knowledge map: {KNOWLEDGE_MAP_RELATIVE_PATH}"),
    )
    .await
    .expect("agents should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    for index in 0..RECENT_HISTORY_LIMIT * 2 + 2 {
        service
            .add_source(
                &context,
                KnowledgeMapSourceAddRequest {
                    id: format!("source-{index}"),
                    topic: "build".to_owned(),
                    kind: KnowledgeMapSourceKind::Config,
                    uri: format!("build/{index}.toml"),
                    source_scope: Some("repo".to_owned()),
                    description: None,
                },
            )
            .await
            .expect("source should add");
    }
    let manifest_text = fs::read_to_string(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
        .await
        .expect("manifest should read");
    let manifest = parse_manifest(&manifest_text).expect("manifest should parse");
    assert_eq!(manifest.history.recent.len(), RECENT_HISTORY_LIMIT);
    assert_eq!(
        manifest.history.omitted_through,
        (RECENT_HISTORY_LIMIT + 3) as u64
    );
    assert_eq!(manifest.history.archived_through, 0);
    assert!(manifest.history.archive.is_none());
    assert!(manifest.history.index.is_none());
    assert!(
        !fs::try_exists(
            root.join(AGENT_CONTRACT_DIR_NAME)
                .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME)
        )
        .await
        .expect("history path should be inspectable")
    );
    let retained = service
        .history(&context, None, RECENT_HISTORY_LIMIT)
        .await
        .expect("default history page should start at the retained boundary");
    assert_eq!(retained.entries.len(), RECENT_HISTORY_LIMIT);
    assert_eq!(retained.omitted_through, (RECENT_HISTORY_LIMIT + 3) as u64);
    assert_eq!(
        retained.earliest_available_version,
        retained.omitted_through + 1
    );
    let error = service
        .history(&context, Some(1), 1)
        .await
        .expect_err("omitted history must not be synthesized");
    assert!(error.to_string().contains("no longer retained"));
    assert!(
        service
            .history(&context, Some(1), MAX_HISTORY_PAGE_SIZE + 1)
            .await
            .is_err()
    );

    let shown = service
        .show(&context, None)
        .await
        .expect("default show should expose only retained history");
    assert!(!shown.map.history.complete);
    assert_eq!(
        shown.map.history.omitted_through,
        (RECENT_HISTORY_LIMIT + 3) as u64
    );
    service
        .route(&context, "build".to_owned())
        .await
        .expect("route should not depend on omitted history");
    let validation = service.validate(&context).await.expect("validate");
    assert!(validation.valid, "{:?}", validation.diagnostics);
    service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "after-compaction".to_owned(),
                topic: "build".to_owned(),
                kind: KnowledgeMapSourceKind::Doc,
                uri: "docs/history.md".to_owned(),
                source_scope: Some("repo".to_owned()),
                description: None,
            },
        )
        .await
        .expect("mutation should advance the bounded window");
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn legacy_show_returns_only_the_recent_history_window() {
    let root = temp_root("legacy-bounded-show");
    fs::create_dir_all(root.join(AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("contract directory should create");
    let mut map = KnowledgeMap::initial("initial".to_owned());
    for index in 0..RECENT_HISTORY_LIMIT + 4 {
        map.record_change(
            "fixture",
            format!("Legacy history {index}"),
            format!("time-{index}"),
        );
    }
    let service = KnowledgeMapService::new(root.clone());
    fs::write(
        service.map_path(),
        serialize_yaml(&map).expect("legacy map should serialize"),
    )
    .await
    .expect("legacy map should write");
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

    let shown = service
        .show(&context, None)
        .await
        .expect("show should work");

    assert_eq!(shown.map.history.recent.len(), RECENT_HISTORY_LIMIT);
    assert_eq!(shown.map.history.omitted_through, 5);
    assert!(!shown.map.history.complete);
    let first_page = service
        .history(&context, Some(1), 3)
        .await
        .expect("legacy history should remain pageable");
    assert_eq!(
        first_page
            .entries
            .iter()
            .map(|entry| entry.version)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn history_pages_reject_digest_valid_noncontiguous_archive_entries() {
    let root = temp_root("invalid-history-page");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    let manifest_text = fs::read_to_string(service.map_path())
        .await
        .expect("manifest should read");
    let mut manifest = parse_manifest(&manifest_text).expect("manifest should parse");
    let mut entries = (1..=RECENT_HISTORY_LIMIT as u64)
        .map(|version| crate::domain::KnowledgeMapHistoryEntry {
            version,
            action: "fixture".to_owned(),
            actor: "test".to_owned(),
            summary: format!("History entry {version}"),
        })
        .collect::<Vec<_>>();
    entries[5].version = 5;
    let archive = KnowledgeMapHistoryArchive {
        schema_version: DIRECTORY_ARTIFACT_SCHEMA_VERSION,
        from_version: 1,
        through_version: RECENT_HISTORY_LIMIT as u64,
        previous: None,
        entries,
    };
    let archive_yaml = serialize_yaml(&archive).expect("archive should serialize");
    let digest = content_digest(archive_yaml.as_bytes());
    let relative = format!(
        "{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/{:020}-{:020}-{digest}.yaml",
        archive.from_version, archive.through_version
    );
    fs::create_dir_all(
        root.join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME),
    )
    .await
    .expect("history directory should create");
    fs::write(
        root.join(AGENT_CONTRACT_DIR_NAME).join(&relative),
        archive_yaml,
    )
    .await
    .expect("archive should write");
    manifest.map_version = RECENT_HISTORY_LIMIT as u64 + 1;
    manifest.schema_version = DIRECTORY_ARTIFACT_SCHEMA_VERSION;
    manifest.history.archived_through = RECENT_HISTORY_LIMIT as u64;
    manifest.history.omitted_through = 0;
    let archive_ref = KnowledgeMapArchiveRef {
        r#ref: relative,
        digest,
    };
    manifest.history.index = Some(
        service
            .append_history_index(None, archive_ref.clone(), &archive)
            .await
            .expect("index should publish"),
    );
    manifest.history.archive = Some(archive_ref);
    manifest.history.recent[0].version = manifest.map_version;
    fs::write(
        service.map_path(),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");

    let error = service
        .history(&context, Some(1), RECENT_HISTORY_LIMIT)
        .await
        .expect_err("noncontiguous archive entries must fail");
    assert!(error.to_string().contains("not contiguous"));
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn an_unlocked_marked_lock_inode_does_not_block_a_writer() {
    let root = temp_root("stale-lock-inode");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(&contract)
        .await
        .expect("contract directory should create");
    let lock_path = contract.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
    fs::write(&lock_path, ADVISORY_LOCK_MARKER)
        .await
        .expect("persistent lock inode should seed");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

    service
        .init(&context)
        .await
        .expect("OS-released lock must be reusable after an owner exits");
    assert!(fs::try_exists(lock_path).await.expect("lock path check"));
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn an_unmarked_legacy_lock_is_not_stolen_during_upgrade() {
    let root = temp_root("legacy-lock");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(&contract)
        .await
        .expect("contract directory should create");
    let lock_path = contract.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
    fs::write(&lock_path, b"")
        .await
        .expect("legacy lock should seed");
    let service = KnowledgeMapService::new(root.clone());

    let error = service
        .acquire_write_lock(Duration::from_millis(50))
        .await
        .expect_err("an old-version writer lock must not be stolen");

    assert!(matches!(error, KnowledgeMapServiceError::LockTimeout(_)));
    assert_eq!(
        fs::read(lock_path).await.expect("legacy lock should read"),
        b""
    );
    let _ = fs::remove_dir_all(root).await;
}

#[test]
fn a_restarted_process_cannot_collide_with_young_staging_names_from_the_same_pid() {
    let lock_path = PathBuf::from(".knowledge").join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
    let process_id = 42;
    let previous_startup = "00112233445566778899aabbccddeeff";
    let restarted_startup = "ffeeddccbbaa99887766554433221100";
    let previous = (0..16)
        .map(|nonce| {
            transition_lock_prepared_path_with_identity(
                &lock_path,
                process_id,
                previous_startup,
                nonce,
            )
        })
        .collect::<std::collections::HashSet<_>>();

    for nonce in 0..16 {
        let restarted = transition_lock_prepared_path_with_identity(
            &lock_path,
            process_id,
            restarted_startup,
            nonce,
        );
        assert!(!previous.contains(&restarted));
    }
}

#[tokio::test]
async fn writer_lock_ignore_contract_is_target_local_preserved_and_idempotent() {
    let root = temp_root("target-lock-ignore-contract");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(&contract)
        .await
        .expect("contract directory should create");
    let ignore_path = contract.join(".gitignore");
    fs::write(&ignore_path, b"/user-owned-entry\n")
        .await
        .expect("existing ignore contract should seed");
    let first_service = KnowledgeMapService::new(root.clone());
    let second_service = KnowledgeMapService::new(root.clone());
    let first = async move {
        let lock = first_service
            .acquire_write_lock(Duration::from_millis(500))
            .await
            .expect("first writer should establish target ignore contract");
        drop(lock);
    };
    let second = async move {
        let lock = second_service
            .acquire_write_lock(Duration::from_millis(500))
            .await
            .expect("concurrent writer should reuse target ignore contract");
        drop(lock);
    };
    tokio::join!(first, second);

    let content = fs::read_to_string(ignore_path)
        .await
        .expect("target ignore contract should read");
    assert!(content.contains("/user-owned-entry\n"));
    assert_eq!(content.matches("/knowledge-map.yaml.lock\n").count(), 1);
    assert_eq!(
        content
            .matches("/knowledge-map.yaml.lock.prepared.*\n")
            .count(),
        1
    );
    assert_eq!(content.matches("/topics/*.retired\n").count(), 1);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn ignore_contract_failure_precedes_every_canonical_or_staging_lock_path() {
    let root = temp_root("lock-ignore-failure-boundary");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(contract.join(".gitignore"))
        .await
        .expect("invalid ignore directory should seed");
    let service = KnowledgeMapService::new(root.clone());

    service
        .acquire_write_lock(Duration::from_millis(50))
        .await
        .expect_err("invalid target ignore contract must stop lock publication");

    let lock_prefix = format!("{KNOWLEDGE_MAP_FILE_NAME}.lock");
    let mut entries = fs::read_dir(&contract)
        .await
        .expect("contract directory should read");
    while let Some(entry) = entries.next_entry().await.expect("entry should read") {
        assert!(
            !entry
                .file_name()
                .to_string_lossy()
                .starts_with(&lock_prefix),
            "ignore failure must not leave canonical or prepared lock paths"
        );
    }
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn an_incomplete_staging_lock_does_not_block_atomic_publication() {
    let root = temp_root("incomplete-prepared-lock");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(&contract)
        .await
        .expect("contract directory should create");
    let lock_path = contract.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
    let prepared_path =
        transition_lock_prepared_path(&lock_path).expect("prepared lock path should generate");
    fs::write(&prepared_path, &ADVISORY_LOCK_MARKER[..7])
        .await
        .expect("interrupted prepared lock should seed");
    let service = KnowledgeMapService::new(root.clone());

    let owned = service
        .acquire_write_lock(Duration::from_millis(50))
        .await
        .expect("an abandoned staging inode must not block publication");
    drop(owned);

    assert_eq!(
        fs::read(&lock_path)
            .await
            .expect("published lock should read"),
        ADVISORY_LOCK_MARKER
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn an_active_unique_staging_lock_does_not_share_the_canonical_inode() {
    let root = temp_root("active-prepared-lock");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(&contract)
        .await
        .expect("contract directory should create");
    let lock_path = contract.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
    let prepared_path =
        transition_lock_prepared_path(&lock_path).expect("prepared lock path should generate");
    let prepared = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&prepared_path)
        .expect("prepared lock should create");
    fs2::FileExt::try_lock_exclusive(&prepared).expect("initializer should own prepared lock");
    let service = KnowledgeMapService::new(root.clone());

    let owned = service
        .acquire_write_lock(Duration::from_millis(50))
        .await
        .expect("another unique staging inode should publish safely");
    drop(owned);

    assert_eq!(
        fs::read(&lock_path)
            .await
            .expect("published lock should read"),
        ADVISORY_LOCK_MARKER
    );
    drop(prepared);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn cleanup_preserves_an_old_staging_inode_while_its_initializer_is_active() {
    let root = temp_root("active-staging-cleanup");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(&contract)
        .await
        .expect("contract directory should create");
    let lock_path = contract.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
    let prepared_path =
        transition_lock_prepared_path(&lock_path).expect("prepared lock path should generate");
    let prepared = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&prepared_path)
        .expect("prepared lock should create");
    fs2::FileExt::try_lock_exclusive(&prepared).expect("initializer should own prepared lock");

    cleanup_transition_locks(&lock_path, Duration::ZERO);

    assert!(
        fs::try_exists(&prepared_path)
            .await
            .expect("active staging path check")
    );
    drop(prepared);
    cleanup_transition_locks(&lock_path, Duration::ZERO);
    assert!(
        !fs::try_exists(&prepared_path)
            .await
            .expect("retired staging path check")
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn cleanup_retires_a_legacy_pid_nonce_staging_name_after_upgrade() {
    let root = temp_root("legacy-staging-cleanup");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(&contract)
        .await
        .expect("contract directory should create");
    let lock_path = contract.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
    let legacy_path = contract.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock.prepared.4242.0"));
    fs::write(&legacy_path, &ADVISORY_LOCK_MARKER[..7])
        .await
        .expect("legacy staging residue should seed");

    cleanup_transition_locks(&lock_path, Duration::ZERO);

    assert!(
        !fs::try_exists(&legacy_path)
            .await
            .expect("legacy staging path check")
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn a_hard_linked_staging_name_is_never_opened_or_overwritten() {
    let root = temp_root("hard-linked-prepared-lock");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(&contract)
        .await
        .expect("contract directory should create");
    let outside = root.join("outside-prepared-target");
    fs::write(&outside, b"outside")
        .await
        .expect("outside file should seed");
    let lock_path = contract.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
    let prepared_path =
        transition_lock_prepared_path(&lock_path).expect("prepared lock path should generate");
    fs::hard_link(&outside, &prepared_path)
        .await
        .expect("prepared hard link should create");
    let service = KnowledgeMapService::new(root.clone());

    let owned = service
        .acquire_write_lock(Duration::from_millis(50))
        .await
        .expect("an unrelated staging name must not block publication");

    drop(owned);
    assert_eq!(
        fs::read(outside).await.expect("outside file should read"),
        b"outside"
    );
    let _ = fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn an_existing_writer_lock_symlink_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;

    let root = temp_root("writer-lock-symlink");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(&contract)
        .await
        .expect("contract directory should create");
    let outside = root.join("outside-lock-target");
    fs::write(&outside, ADVISORY_LOCK_MARKER)
        .await
        .expect("outside target should seed");
    let lock_path = contract.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
    symlink(&outside, &lock_path).expect("lock symlink should create");
    let service = KnowledgeMapService::new(root.clone());

    let error = service
        .acquire_write_lock(Duration::from_millis(50))
        .await
        .expect_err("writer lock symlink must not be followed");

    assert!(matches!(error, KnowledgeMapServiceError::Io(_)));
    assert_eq!(
        fs::read(&outside)
            .await
            .expect("outside target should read"),
        ADVISORY_LOCK_MARKER
    );
    assert!(
        fs::symlink_metadata(lock_path)
            .await
            .expect("lock symlink metadata should read")
            .file_type()
            .is_symlink()
    );
    let _ = fs::remove_dir_all(root).await;
}

#[cfg(windows)]
#[tokio::test]
async fn an_existing_writer_lock_reparse_point_is_rejected_without_following_it() {
    use std::os::windows::fs::symlink_file;

    let root = temp_root("writer-lock-reparse-point");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(&contract)
        .await
        .expect("contract directory should create");
    let outside = root.join("outside-lock-target");
    fs::write(&outside, ADVISORY_LOCK_MARKER)
        .await
        .expect("outside target should seed");
    let lock_path = contract.join(format!("{KNOWLEDGE_MAP_FILE_NAME}.lock"));
    if let Err(error) = symlink_file(&outside, &lock_path) {
        const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
        if error.kind() == std::io::ErrorKind::PermissionDenied
            || error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD)
        {
            let _ = fs::remove_dir_all(root).await;
            return;
        }
        panic!("lock reparse point should create: {error}");
    }
    let service = KnowledgeMapService::new(root.clone());

    let error = service
        .acquire_write_lock(Duration::from_millis(50))
        .await
        .expect_err("writer lock reparse point must not be followed");

    assert!(matches!(error, KnowledgeMapServiceError::Io(_)));
    assert_eq!(
        fs::read(&outside)
            .await
            .expect("outside target should read"),
        ADVISORY_LOCK_MARKER
    );
    assert!(
        fs::symlink_metadata(lock_path)
            .await
            .expect("lock reparse metadata should read")
            .file_type()
            .is_symlink()
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn an_active_writer_cannot_be_stolen_and_wait_is_bounded() {
    let root = temp_root("active-lock");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let active = service
        .acquire_write_lock(Duration::from_millis(50))
        .await
        .expect("first writer should acquire ownership");

    let error = service
        .acquire_write_lock(Duration::from_millis(50))
        .await
        .expect_err("a live writer must retain ownership");
    assert!(matches!(error, KnowledgeMapServiceError::LockTimeout(_)));

    drop(active);
    service
        .acquire_write_lock(Duration::from_millis(50))
        .await
        .expect("released ownership should be immediately reusable");
    let _ = fs::remove_dir_all(root).await;
}
