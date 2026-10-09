use super::*;
use crate::domain::research::*;

async fn fixture() -> (std::path::PathBuf, SourceCatalog) {
    let mut nonce = [0; 16];
    getrandom::getrandom(&mut nonce).unwrap();
    let root = std::env::temp_dir().join(format!("relay-research-audit-{}", digest(&nonce)));
    tokio::fs::create_dir_all(root.join("sources"))
        .await
        .unwrap();
    let raw = b"<html>\r\n  <body>Original </body>\r\n</html>";
    let text = b"# Title\n\n## Details\nBody\n";
    tokio::fs::write(root.join("sources/raw.html"), raw)
        .await
        .unwrap();
    tokio::fs::write(root.join("sources/text.txt"), text)
        .await
        .unwrap();
    let source = SourceCapture {
        id: "first".into(),
        url: "https://example.org/original".into(),
        transport: Some(CaptureTransport {
            http_status: Some(200),
            final_url: Some("https://example.org/final".into()),
            redirects: vec!["https://example.org/redirect".into()],
            access_error: None,
        }),
        raw: Some(ResearchArtifact {
            path_base: ResearchPathBase::Catalog,
            path: "raw.html".into(),
            sha256: digest(raw),
        }),
        extraction: Some(CaptureExtraction {
            artifact: ResearchArtifact {
                path_base: ResearchPathBase::Repository,
                path: "sources/text.txt".into(),
                sha256: digest(text),
            },
            raw_sha256: digest(raw),
            extractor: "test-extractor".into(),
            extractor_version: "1".into(),
            expected_extractor: None,
            expected_extractor_version: None,
        }),
        parent_source: None,
        references: Vec::new(),
        expected_sections: vec!["Title".into(), "Details".into()],
        declared_coverage: Some(CaptureCoverage::FullBody),
        review: Some(ResearchReviewClaim {
            content_sha256: digest(text),
            reviewer: "Author".into(),
            origin: "catalog-declaration".into(),
            event_id: Some("untrusted-event".into()),
        }),
    };
    (
        root,
        SourceCatalog {
            schema_version: 1,
            adapter: "relay-capture-v1".into(),
            sources: vec![source],
        },
    )
}

async fn audit(root: &Path, catalog: &SourceCatalog) -> SourceAuditReport {
    tokio::fs::write(
        root.join("sources/catalog.json"),
        serde_json::to_vec(catalog).unwrap(),
    )
    .await
    .unwrap();
    super::super::ResearchService::new(root.into())
        .audit_sources("sources/catalog.json".into())
        .await
        .unwrap()
}

#[tokio::test]
async fn preserves_original_bytes_and_separates_declared_review_from_integrity() {
    let _guard = super::super::test_support::TEST_LOCK.lock().await;
    let (root, mut catalog) = fixture().await;
    let original = tokio::fs::read(root.join("sources/raw.html"))
        .await
        .unwrap();
    for _ in 0..2 {
        let report = audit(&root, &catalog).await;
        assert!(report.integrity_valid);
        let capture = &report.captures[0];
        assert_eq!(capture.coverage, "declared_sections_present");
        assert_eq!(capture.review, "self_reported_unverified");
        assert_eq!(capture.transport_state, "reported_only");
        assert_eq!(capture.index_freshness, "not_assessed");
        assert_eq!(capture.transport.as_ref().unwrap().redirects.len(), 1);
        assert_eq!(
            tokio::fs::read(root.join("sources/raw.html"))
                .await
                .unwrap(),
            original
        );
    }
    catalog.sources[0].expected_sections.clear();
    assert_eq!(audit(&root, &catalog).await.captures[0].coverage, "unknown");
    catalog.sources[0].declared_coverage = Some(CaptureCoverage::Shell);
    assert_eq!(
        audit(&root, &catalog).await.captures[0].coverage,
        "declared_shell"
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn detects_hash_damage_extractor_changes_and_reviewed_content_changes() {
    let _guard = super::super::test_support::TEST_LOCK.lock().await;
    let (root, mut catalog) = fixture().await;
    tokio::fs::write(root.join("sources/text.txt"), b"Title only")
        .await
        .unwrap();
    let report = audit(&root, &catalog).await;
    assert!(!report.integrity_valid);
    assert_eq!(report.captures[0].review, "needs_review");
    assert_eq!(report.captures[0].coverage, "unverifiable");
    catalog.sources[0]
        .extraction
        .as_mut()
        .unwrap()
        .artifact
        .sha256 = digest(b"Title only");
    let report = audit(&root, &catalog).await;
    assert_eq!(report.captures[0].coverage, "missing_declared_sections");
    catalog.sources[0]
        .extraction
        .as_mut()
        .unwrap()
        .expected_extractor_version = Some("2".into());
    assert_eq!(
        audit(&root, &catalog).await.captures[0].extraction_state,
        "extractor_changed"
    );
    tokio::fs::write(root.join("sources/raw.html"), b"changed")
        .await
        .unwrap();
    let report = audit(&root, &catalog).await;
    assert!(
        report.captures[0]
            .diagnostics
            .iter()
            .any(|item| item.code == "extraction_source_changed")
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn reports_identical_bytes_for_different_urls_without_merging_source_identity() {
    let _guard = super::super::test_support::TEST_LOCK.lock().await;
    let (root, mut catalog) = fixture().await;
    let mut other = catalog.sources[0].clone();
    other.id = "second".into();
    other.url = "https://example.net/different".into();
    catalog.sources.push(other);
    let report = audit(&root, &catalog).await;
    assert_eq!(report.identical_byte_groups, [vec!["first", "second"]]);
    assert!(report.integrity_valid);
    catalog.sources[1].id = "first".into();
    assert_eq!(
        audit(&root, &catalog).await.diagnostics[0].code,
        "duplicate_id"
    );
    catalog.sources[1].id = "second".into();
    catalog.sources[0].parent_source = Some("second".into());
    catalog.sources[1].parent_source = Some("first".into());
    assert!(
        audit(&root, &catalog)
            .await
            .diagnostics
            .iter()
            .any(|d| d.code == "source_chain_cycle")
    );
    catalog.sources[1].parent_source = Some("missing".into());
    assert!(
        audit(&root, &catalog)
            .await
            .diagnostics
            .iter()
            .any(|d| d.code == "missing_parent_source")
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn distinguishes_pdf_access_failure_and_unreadable_or_out_of_scope_artifacts() {
    let _guard = super::super::test_support::TEST_LOCK.lock().await;
    let (root, mut catalog) = fixture().await;
    let pdf = b"%PDF-1.7\r\n\xff\x00";
    tokio::fs::write(root.join("sources/raw.html"), pdf)
        .await
        .unwrap();
    catalog.sources[0].raw.as_mut().unwrap().sha256 = digest(pdf);
    catalog.sources[0].extraction = None;
    catalog.sources[0].review = None;
    assert_eq!(
        audit(&root, &catalog).await.captures[0]
            .local_capture
            .as_ref()
            .unwrap()
            .state,
        "verified"
    );
    catalog.sources[0].transport.as_mut().unwrap().http_status = Some(404);
    catalog.sources[0].raw = None;
    let report = audit(&root, &catalog).await;
    assert_eq!(
        report.captures[0].transport_state,
        "reported_access_failure"
    );
    assert_eq!(report.captures[0].diagnostics[0].code, "missing_capture");
    catalog.sources[0].references.push(ResearchArtifact {
        path_base: ResearchPathBase::Catalog,
        path: "../../outside".into(),
        sha256: digest(b""),
    });
    assert!(
        audit(&root, &catalog).await.captures[0]
            .diagnostics
            .iter()
            .any(|d| d.code == "artifact_unreadable")
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}
