use super::*;
use crate::{
    api::InterfaceKind,
    domain::{KnowledgeMapSourceKind, map_batch::MapSourceOperation},
};

async fn fixture() -> (std::path::PathBuf, KnowledgeMapService, RequestContext) {
    let mut nonce = [0; 16];
    getrandom::getrandom(&mut nonce).unwrap();
    let root = std::env::temp_dir().join(format!("relay-map-batch-{}", content_digest(&nonce)));
    fs::create_dir_all(&root).await.unwrap();
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    service.init(&context).await.unwrap();
    (root, service, context)
}

fn request() -> MapBatchRequest {
    MapBatchRequest {
        schema_version: 1,
        transaction_id: "ten-sources".into(),
        expected_map_version: None,
        expected_digest: None,
        operations: (0..10)
            .map(|i| MapSourceOperation::Add {
                id: format!("source-{i}"),
                topic: "research".into(),
                kind: KnowledgeMapSourceKind::File,
                uri: format!("sources/{i}.txt"),
                source_scope: None,
                description: None,
            })
            .collect(),
    }
}

#[tokio::test]
async fn plans_without_publication_and_applies_ten_sources_once_with_replay() {
    let (root, service, context) = fixture().await;
    let original = service.read_root_content().await.unwrap();
    let plan = service
        .source_batch(&context, request(), false)
        .await
        .unwrap();
    assert_eq!(plan.state, "planned");
    assert_eq!(service.read_root_content().await.unwrap(), original);
    let applied = service
        .source_batch(&context, plan.transaction.clone(), true)
        .await
        .unwrap();
    assert_eq!(applied.state, "applied");
    assert_eq!(applied.map_version, plan.map_version + 1);
    assert_eq!(applied.cleanup.state, "deferred");
    let snapshot = service.load_for_mutation().await.unwrap();
    assert_eq!(snapshot.map.sources.len(), 12);
    assert_eq!(snapshot.map.history.len(), 2);
    let receipt: BatchReceipt =
        serde_json::from_str(&snapshot.map.history.last().unwrap().summary).unwrap();
    assert_eq!(receipt.transaction.operations.len(), 10);
    let committed = service.read_root_content().await.unwrap();
    let replay = service
        .source_batch(&context, plan.transaction, true)
        .await
        .unwrap();
    assert_eq!(replay.state, "already_applied");
    assert_eq!(service.read_root_content().await.unwrap(), committed);
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn rejects_invalid_batches_changed_preconditions_and_transaction_reuse() {
    let (root, service, context) = fixture().await;
    assert!(
        service
            .source_batch(&context, request(), true)
            .await
            .is_err()
    );
    let plan = service
        .source_batch(&context, request(), false)
        .await
        .unwrap();
    let mut invalid = plan.transaction.clone();
    invalid.operations.push(MapSourceOperation::Remove {
        id: "repository-business-glossary".into(),
    });
    let original = service.read_root_content().await.unwrap();
    assert_eq!(
        service
            .source_batch(&context, invalid, true)
            .await
            .unwrap()
            .state,
        "invalid"
    );
    assert_eq!(service.read_root_content().await.unwrap(), original);
    let mut conflict = plan.transaction.clone();
    conflict.expected_digest = Some("0".repeat(64));
    assert_eq!(
        service
            .source_batch(&context, conflict, true)
            .await
            .unwrap()
            .state,
        "conflict"
    );
    service
        .source_batch(&context, plan.transaction.clone(), true)
        .await
        .unwrap();
    let mut conflict = plan.transaction.clone();
    conflict.operations.pop();
    assert_eq!(
        service
            .source_batch(&context, conflict, true)
            .await
            .unwrap()
            .state,
        "conflict"
    );
    let mut conflict = plan.transaction;
    conflict.transaction_id = "different".into();
    assert_eq!(
        service
            .source_batch(&context, conflict, true)
            .await
            .unwrap()
            .state,
        "conflict"
    );
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn concurrent_apply_and_interrupted_root_recovery_publish_one_batch() {
    let (root, service, context) = fixture().await;
    let plan = service
        .source_batch(&context, request(), false)
        .await
        .unwrap();
    // Simulate interruption after moving the old root to the recovery path.
    fs::rename(service.map_path(), service.backup_path())
        .await
        .unwrap();
    let (left, right) = tokio::join!(
        service.source_batch(&context, plan.transaction.clone(), true),
        service.source_batch(&context, plan.transaction, true)
    );
    let states = [left.unwrap().state, right.unwrap().state];
    assert!(states.contains(&"applied".to_owned()));
    assert!(states.contains(&"already_applied".to_owned()));
    assert_eq!(
        service.load_for_mutation().await.unwrap().map.map_version,
        2
    );
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn input_and_schema_errors_leave_the_root_untouched() {
    let (root, service, context) = fixture().await;
    let input = root.join("batch.json");
    fs::write(&input, "not JSON").await.unwrap();
    assert!(
        service
            .batch_from_file(&context, &input, false)
            .await
            .is_err()
    );
    fs::write(&input, vec![b' '; MAX_BATCH_BYTES + 1])
        .await
        .unwrap();
    assert!(
        service
            .batch_from_file(&context, &input, false)
            .await
            .is_err()
    );
    fs::write(&input, serde_json::to_vec(&request()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        service
            .batch_from_file(&context, &input, false)
            .await
            .unwrap()
            .state,
        "planned"
    );
    assert!(
        service
            .batch_from_file(&context, &root, false)
            .await
            .is_err()
    );
    assert!(
        service
            .for_type(crate::domain::RepositoryMapType::Codespec)
            .source_batch(&context, request(), false)
            .await
            .is_err()
    );
    let original = service.read_root_content().await.unwrap();
    fs::write(
        service.map_path(),
        original.replace("schema_version: 4", "schema_version: 3"),
    )
    .await
    .unwrap();
    assert!(
        service
            .source_batch(&context, request(), false)
            .await
            .is_err()
    );
    fs::remove_dir_all(root).await.unwrap();
}
