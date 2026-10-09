use super::super::{
    bundle::load_bundle,
    reader,
    test_support::{TEST_LOCK, fixture, save},
};
use super::*;

#[tokio::test]
async fn core_normalization_cannot_merge_distinct_authored_ids_or_rebase_pin_paths() {
    let _guard = TEST_LOCK.lock().await;
    let (root, mut bundle, _) = fixture().await;
    let old = bundle.graph.nodes[1].id.clone();
    let distinct = format!("{} ", bundle.graph.nodes[0].id);
    bundle.graph.nodes[1].id = distinct.clone();
    for edge in &mut bundle.graph.edges {
        if edge.source == old {
            edge.source = distinct.clone();
        }
        if edge.target == old {
            edge.target = distinct.clone();
        }
    }
    tokio::fs::create_dir_all(root.join("nested"))
        .await
        .unwrap();
    tokio::fs::write(root.join("nested/a.txt"), b"First\nSecond\n")
        .await
        .unwrap();
    bundle.evidence[0].artifact.path_base = crate::domain::research::ResearchPathBase::Catalog;
    save(&root, &bundle).await;
    tokio::fs::rename(root.join("bundle.json"), root.join("nested/bundle.json"))
        .await
        .unwrap();
    reader::run(root.clone(), |reader| {
        let loaded = load_bundle(
            reader,
            std::path::Path::new("nested/bundle.json"),
            "research",
        )?;
        assert!(loaded.report.valid);
        let request = ingest_request(&loaded)?;
        assert_ne!(
            request.evidence[0].entity_labels[0].trim(),
            request.evidence[0].entity_labels[1].trim()
        );
        assert_ne!(
            request.relations[0].source_entity_label,
            request.relations[0].target_entity_label
        );
        assert_eq!(
            request.evidence[1].source_path.as_deref(),
            Some("nested/a.txt")
        );
        assert_eq!(
            request.evidence[1]
                .extraction
                .as_ref()
                .unwrap()
                .source_hash
                .as_ref(),
            Some(&loaded.bundle.evidence[0].artifact.sha256)
        );
        Ok(())
    })
    .await
    .unwrap();
    tokio::fs::remove_dir_all(root).await.unwrap();
}
