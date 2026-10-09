use super::*;

fn catalog() -> SourceCatalog {
    serde_json::from_value(serde_json::json!({"schema_version":1,"adapter":"relay-capture-v1","sources":[{"id":"a","url":"https://example.org"}]})).unwrap()
}

#[test]
fn validates_explicit_adapter_and_resource_limits() {
    let valid = catalog();
    valid.validate().unwrap();
    let mut value = valid.clone();
    value.schema_version = 2;
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.adapter = "guess".into();
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.sources.clear();
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.sources = vec![valid.sources[0].clone(); 257];
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.sources[0].id.clear();
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.sources[0].url = "x".repeat(4097);
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.sources[0].expected_sections = vec!["section".into(); 65];
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.sources[0].expected_sections.push(" ".into());
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.sources[0].transport = Some(CaptureTransport {
        http_status: Some(600),
        final_url: None,
        redirects: Vec::new(),
        access_error: None,
    });
    assert!(value.validate().is_err());
    value.sources[0].transport.as_mut().unwrap().http_status = Some(200);
    value.sources[0].transport.as_mut().unwrap().redirects = vec!["url".into(); 17];
    assert!(value.validate().is_err());
    value.sources[0]
        .transport
        .as_mut()
        .unwrap()
        .redirects
        .clear();
    value.validate().unwrap();
    value.sources[0].transport.as_mut().unwrap().access_error =
        Some("x".repeat(MAX_RESEARCH_JSON_BYTES));
    assert!(value.validate().is_err());
    for hash in ["", "ABC", &"G".repeat(64)] {
        assert!(validate_sha256(hash).is_err());
    }
    validate_sha256(&"a".repeat(64)).unwrap();
}

#[test]
fn requires_extraction_identity_review_binding_and_nonempty_artifact_paths() {
    let mut value = catalog();
    let artifact = ResearchArtifact {
        path_base: ResearchPathBase::Catalog,
        path: "raw".into(),
        sha256: "a".repeat(64),
    };
    value.sources[0].raw = Some(artifact.clone());
    value.sources[0].extraction = Some(CaptureExtraction {
        artifact,
        raw_sha256: "b".repeat(64),
        extractor: "test".into(),
        extractor_version: "1".into(),
        expected_extractor: None,
        expected_extractor_version: None,
    });
    value.sources[0].review = Some(ResearchReviewClaim {
        content_sha256: "b".repeat(64),
        reviewer: "author".into(),
        origin: "self".into(),
        event_id: None,
    });
    value.validate().unwrap();
    let mut invalid = value.clone();
    invalid.sources[0].raw.as_mut().unwrap().path.clear();
    assert!(invalid.validate().is_err());
    let mut invalid = value.clone();
    invalid.sources[0]
        .extraction
        .as_mut()
        .unwrap()
        .extractor
        .clear();
    assert!(invalid.validate().is_err());
    let mut invalid = value.clone();
    invalid.sources[0]
        .extraction
        .as_mut()
        .unwrap()
        .extractor_version
        .clear();
    assert!(invalid.validate().is_err());
    let mut invalid = value.clone();
    invalid.sources[0]
        .extraction
        .as_mut()
        .unwrap()
        .raw_sha256
        .clear();
    assert!(invalid.validate().is_err());
    let mut invalid = value.clone();
    invalid.sources[0]
        .review
        .as_mut()
        .unwrap()
        .content_sha256
        .clear();
    assert!(invalid.validate().is_err());
    let mut invalid = value.clone();
    invalid.sources[0].review.as_mut().unwrap().reviewer.clear();
    assert!(invalid.validate().is_err());
    let mut invalid = value.clone();
    invalid.sources[0].review.as_mut().unwrap().origin.clear();
    assert!(invalid.validate().is_err());
    let mut invalid = value.clone();
    invalid.sources[0].references = vec![value.sources[0].raw.clone().unwrap(); 33];
    assert!(invalid.validate().is_err());
}
