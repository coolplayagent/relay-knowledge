use super::*;

#[test]
fn criteria_need_unique_ids_bounded_bindings_and_versioned_review() {
    let valid: ResearchRequirements = serde_json::from_value(serde_json::json!({"schema_version":1,"requirements":[{"id":"r1","description":"Evidence requirement","evidence":[{"path_base":"repository","path":"a.txt","sha256":"a".repeat(64)}],"review_claim":{"reviewer":"author","origin":"local","content_sha256":"b".repeat(64)}}]})).unwrap();
    valid.validate().unwrap();
    let mut value = valid.clone();
    value.schema_version = 2;
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.requirements.clear();
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.requirements.push(value.requirements[0].clone());
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.requirements[0].evidence.clear();
    assert!(value.validate().is_err());
    let mut value = valid.clone();
    value.requirements[0]
        .review_claim
        .as_mut()
        .unwrap()
        .content_sha256 = "bad".into();
    assert!(value.validate().is_err());
    let mut value = valid;
    value.requirements[0].evidence[0].sha256 = "bad".into();
    assert!(value.validate().is_err());
}
