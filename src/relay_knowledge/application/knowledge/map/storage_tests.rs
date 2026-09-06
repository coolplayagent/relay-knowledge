//! Disk-footprint regressions exercised by the self-iteration fast gate.

use std::{collections::BTreeMap, path::Path, time::SystemTime};

use super::*;
use crate::domain::{KnowledgeMapChange, KnowledgeMapSourceKind};

#[tokio::test]
async fn repeated_equal_updates_do_not_grow_shards_history_or_rewrite_roots() {
    let (root, service, context) = fixture("equal-updates").await;
    let initial = disk_snapshot(&root).await;
    let version = service.show(&context, None).await.unwrap().map.map_version;
    for _ in 0..32 {
        let response = service
            .update_source(&context, change("revision zero"))
            .await
            .unwrap();
        assert_eq!(response.map_version, version);
    }
    let after = disk_snapshot(&root).await;
    assert_eq!(
        after, initial,
        "equal updates must add zero bytes/files and preserve mtimes"
    );
    eprintln!("map_storage_equal_updates=32 added_files=0 added_bytes=0");

    let response = service
        .update_source(&context, change("revision one"))
        .await
        .unwrap();
    assert_eq!(response.map_version, version + 1);
    let shown = service.show(&context, None).await.unwrap();
    let source = shown
        .map
        .sources
        .iter()
        .find(|source| source.id == "guide")
        .unwrap();
    assert_eq!(source.version, 2);
    assert_eq!(source.description.as_deref(), Some("revision one"));
    let committed = disk_snapshot(&root).await;
    let mut invalid = change("revision one");
    invalid.uri = Some(" ".to_owned());
    assert!(service.update_source(&context, invalid).await.is_err());
    assert_eq!(disk_snapshot(&root).await, committed);
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn equal_source_update_still_publishes_required_legacy_migration() {
    let (root, service, context) = fixture("equal-legacy").await;
    migration::rewrite_contract_schema_for_test(
        &root,
        service.contract_dir_name(),
        &service.map_path(),
        DIRECTORY_ARTIFACT_SCHEMA_VERSION,
        None,
    )
    .await
    .unwrap();
    let before = service.show(&context, None).await.unwrap().map.map_version;
    let response = service
        .update_source(&context, change("revision zero"))
        .await
        .unwrap();
    assert_eq!(response.map_version, before + 1);
    let manifest = parse_manifest(&fs::read_to_string(service.map_path()).await.unwrap()).unwrap();
    assert_eq!(manifest.schema_version, ARTIFACT_SCHEMA_VERSION);
    assert_eq!(
        service.show(&context, None).await.unwrap().map.map_version,
        before + 1
    );
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn init_reclaims_expired_shards_without_changing_map_or_recovery_roots() {
    let (root, service, context) = fixture("idle-cleanup").await;
    for revision in ["revision one", "revision two", "revision three"] {
        service
            .update_source(&context, change(revision))
            .await
            .unwrap();
    }
    let before = disk_snapshot(&root).await;
    let expired = before
        .keys()
        .filter(|path| path.ends_with(".retired"))
        .cloned()
        .collect::<Vec<_>>();
    assert!(!expired.is_empty());
    for path in &expired {
        // Test-only filesystem clock setup avoids sleeping through the reader grace.
        std::fs::File::options()
            .write(true)
            .open(root.join(path))
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH))
            .unwrap();
    }
    let topics = root.join("knowledge/topics");
    fs::write(topics.join("manual.yaml"), "authored data")
        .await
        .unwrap();
    let response = service.init(&context).await.unwrap();
    let after = disk_snapshot(&root).await;
    for path in expired {
        assert!(!after.contains_key(&path));
        assert!(!after.contains_key(path.strip_suffix(".retired").unwrap()));
    }
    for path in [
        "knowledge/knowledge-map.yaml",
        "knowledge/knowledge-map.yaml.previous",
    ] {
        assert_eq!(
            after.get(path),
            before.get(path),
            "maintenance must preserve {path}"
        );
    }
    assert_eq!(after["knowledge/topics/manual.yaml"].0, b"authored data");
    let manifest = parse_manifest(&fs::read_to_string(service.map_path()).await.unwrap()).unwrap();
    assert_eq!(response.map_version, manifest.map_version);
    for topic in &manifest.topics {
        assert!(after.contains_key(&format!("knowledge/{}", topic.r#ref)));
    }
    let before_bytes: usize = before.values().map(|(bytes, _)| bytes.len()).sum();
    let after_bytes: usize = after.values().map(|(bytes, _)| bytes.len()).sum();
    assert!(after_bytes < before_bytes);
    eprintln!("map_storage_idle_cleanup before_bytes={before_bytes} after_bytes={after_bytes}");
    service.show(&context, None).await.unwrap();
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn init_preserves_young_retirements_and_does_not_create_codespec_topics() {
    let (root, service, context) = fixture("young-cleanup").await;
    for revision in ["revision one", "revision two"] {
        service
            .update_source(&context, change(revision))
            .await
            .unwrap();
    }
    let before = disk_snapshot(&root).await;
    service.init(&context).await.unwrap();
    assert_eq!(disk_snapshot(&root).await, before);
    service
        .for_type(RepositoryMapType::Codespec)
        .init(&context)
        .await
        .unwrap();
    assert!(!fs::try_exists(root.join("codespec/topics")).await.unwrap());
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn cleanup_limits_retirements_per_attempt_and_resumes_without_losing_live_topics() {
    let (root, service, context) = fixture("bounded-cleanup").await;
    let manifest = parse_manifest(&fs::read_to_string(service.map_path()).await.unwrap()).unwrap();
    let topics = root.join("knowledge/topics");
    for index in 0..=TOPIC_CLEANUP_ENTRY_LIMIT {
        let name = format!("topic-{:016x}-{:064x}.yaml", index, index);
        fs::write(topics.join(name), "unreferenced generated shard")
            .await
            .unwrap();
    }
    let before = disk_snapshot(&root).await;
    cleanup_superseded_topic_shards(&root, &service.backup_path(), &manifest, Duration::ZERO).await;
    let after = disk_snapshot(&root).await;
    assert_eq!(before.len() - after.len(), TOPIC_CLEANUP_ENTRY_LIMIT);
    cleanup_superseded_topic_shards(&root, &service.backup_path(), &manifest, Duration::ZERO).await;
    assert_eq!(after.len() - disk_snapshot(&root).await.len(), 1);
    service.show(&context, None).await.unwrap();
    fs::remove_dir_all(root).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn cleanup_preserves_unknown_and_linked_entries() {
    let (root, service, context) = fixture("linked-cleanup").await;
    let manifest = parse_manifest(&fs::read_to_string(service.map_path()).await.unwrap()).unwrap();
    let topics = root.join("knowledge/topics");
    let name = format!("topic-{}-{}.yaml", "a".repeat(16), "b".repeat(64));
    let target = topics.join("authored.yaml");
    fs::write(&target, "authored data").await.unwrap();
    std::os::unix::fs::symlink(&target, topics.join(&name)).unwrap();
    cleanup_superseded_topic_shards(&root, &service.backup_path(), &manifest, Duration::ZERO).await;
    assert!(
        fs::symlink_metadata(topics.join(&name))
            .await
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&target).await.unwrap(), b"authored data");
    assert!(
        !fs::try_exists(topics.join(format!("{name}.retired")))
            .await
            .unwrap()
    );
    service.show(&context, None).await.unwrap();
    fs::remove_dir_all(root).await.unwrap();
}

async fn fixture(label: &str) -> (PathBuf, KnowledgeMapService, RequestContext) {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-map-storage-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).await.unwrap();
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.unwrap();
    service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "guide".to_owned(),
                topic: "development".to_owned(),
                kind: KnowledgeMapSourceKind::Doc,
                uri: "docs/guide.md".to_owned(),
                source_scope: Some("repo".to_owned()),
                description: Some("revision zero".to_owned()),
            },
        )
        .await
        .unwrap();
    (root, service, context)
}

fn change(description: &str) -> KnowledgeMapChange {
    KnowledgeMapChange {
        id: "guide".to_owned(),
        topic: Some("development".to_owned()),
        kind: Some(KnowledgeMapSourceKind::Doc),
        uri: Some("docs/guide.md".to_owned()),
        source_scope: Some("repo".to_owned()),
        description: Some(description.to_owned()),
    }
}

async fn disk_snapshot(root: &Path) -> BTreeMap<String, (Vec<u8>, SystemTime)> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(directory).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let metadata = entry.metadata().await.unwrap();
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                // Advisory locks update operational state, not map storage.
                if !relative.contains(".lock") {
                    files.insert(
                        relative,
                        (fs::read(path).await.unwrap(), metadata.modified().unwrap()),
                    );
                }
            }
        }
    }
    files
}
