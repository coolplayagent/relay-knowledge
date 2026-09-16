use super::*;

#[test]
fn every_portable_source_format_shares_the_validated_service_contract() {
    for source in CODE_CONFIG_SOURCE_FORMATS {
        let filter = CodeConfigFilter {
            source: Some(format!(" {} ", source.to_uppercase())),
            ..Default::default()
        }
        .validate()
        .unwrap();
        assert_eq!(filter.source.as_deref(), Some(*source));
        let encoded = serde_json::to_string(&filter).unwrap();
        assert_eq!(
            serde_json::from_str::<CodeConfigFilter>(&encoded).unwrap(),
            filter
        );
    }
    let metadata: CodeConfigMetadata = serde_json::from_str("{}").unwrap();
    assert!(metadata.flow_incomplete.is_none());
    assert!(metadata.bindings.is_empty());
}
#[test]
fn filters_normalize_known_values_and_reject_unknown_sources() {
    let filter = CodeConfigFilter {
        domain: Some(" Business ".into()),
        source: Some("JAVA".into()),
        hot_reload: Some(false),
        consistency: true,
    }
    .validate()
    .unwrap();
    assert_eq!(filter.domain.as_deref(), Some("business"));
    assert_eq!(filter.source.as_deref(), Some("java"));
    for source in ["", "runtime", "x"] {
        assert!(
            CodeConfigFilter {
                source: Some(source.into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
#[test]
fn old_metadata_json_retains_unknown_values() {
    let metadata: CodeConfigMetadata = serde_json::from_str("{}").unwrap();
    assert_eq!(metadata, CodeConfigMetadata::default());
    assert!(metadata.default_value.is_none());
    assert!(metadata.hot_reload.is_none());
}
