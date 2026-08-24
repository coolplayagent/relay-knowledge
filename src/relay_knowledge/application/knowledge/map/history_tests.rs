use super::*;

#[tokio::test]
async fn bounds_recent_history_and_detects_archive_tampering() {
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
    assert_eq!(manifest.history.recent.len(), 3);
    assert_eq!(
        manifest.history.archived_through,
        (RECENT_HISTORY_LIMIT * 2) as u64
    );
    let archive_ref = manifest.history.archive.expect("archive should exist");
    let mut archive_entries = fs::read_dir(
        root.join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME),
    )
    .await
    .expect("history directory should read");
    let mut archive_count = 0;
    while archive_entries
        .next_entry()
        .await
        .expect("archive entry should read")
        .is_some()
    {
        archive_count += 1;
    }
    assert_eq!(archive_count, 2);
    let first_page = service
        .history(&context, 1, RECENT_HISTORY_LIMIT)
        .await
        .expect("first history page should load");
    let second_page = service
        .history(
            &context,
            first_page.next_from_version.expect("next page"),
            RECENT_HISTORY_LIMIT,
        )
        .await
        .expect("second history page should load");
    assert_eq!(first_page.entries.len(), RECENT_HISTORY_LIMIT);
    assert_eq!(first_page.through_version + 1, second_page.from_version);
    assert!(
        service
            .history(&context, 1, MAX_HISTORY_PAGE_SIZE + 1)
            .await
            .is_err()
    );

    let head_archive_text =
        fs::read_to_string(root.join(AGENT_CONTRACT_DIR_NAME).join(&archive_ref.r#ref))
            .await
            .expect("head archive should read");
    let head_archive = serde_norway::from_str::<KnowledgeMapHistoryArchive>(&head_archive_text)
        .expect("head archive should parse");
    let older_ref = head_archive.previous.expect("older archive should exist");
    let archive_path = root.join(AGENT_CONTRACT_DIR_NAME).join(&older_ref.r#ref);
    fs::remove_file(&archive_path)
        .await
        .expect("archive should be removed");
    let shown = service
        .show(&context, None)
        .await
        .expect("default show must not load old archives");
    assert!(!shown.map.history.complete);
    assert_eq!(
        shown.map.history.archived_through,
        (RECENT_HISTORY_LIMIT * 2) as u64
    );
    service
        .route(&context, "build".to_owned())
        .await
        .expect("route must not load old archives");
    let head_page = service
        .history(&context, head_archive.from_version, RECENT_HISTORY_LIMIT)
        .await
        .expect("a page in the head archive must not load older chunks");
    assert_eq!(head_page.entries[0].version, head_archive.from_version);
    assert!(service.history(&context, 1, 1).await.is_err());
    let validation = service.validate(&context).await.expect("validate");
    assert!(!validation.valid);
    assert!(validation.diagnostics[0].contains(&older_ref.r#ref));
    assert!(validation.diagnostics[0].contains(&older_ref.digest));
    let mutation = service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "after-tamper".to_owned(),
                topic: "build".to_owned(),
                kind: KnowledgeMapSourceKind::Doc,
                uri: "docs/tamper.md".to_owned(),
                source_scope: Some("repo".to_owned()),
                description: None,
            },
        )
        .await;
    assert!(mutation.is_err(), "mutation must verify archived history");
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
    assert_eq!(shown.map.history.archived_through, 5);
    assert!(!shown.map.history.complete);
    let first_page = service
        .history(&context, 1, 3)
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
        schema_version: ARTIFACT_SCHEMA_VERSION,
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
    manifest.history.archived_through = RECENT_HISTORY_LIMIT as u64;
    manifest.history.archive = Some(KnowledgeMapArchiveRef {
        r#ref: relative,
        digest,
    });
    manifest.history.recent[0].version = manifest.map_version;
    fs::write(
        service.map_path(),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");

    let error = service
        .history(&context, 1, RECENT_HISTORY_LIMIT)
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
