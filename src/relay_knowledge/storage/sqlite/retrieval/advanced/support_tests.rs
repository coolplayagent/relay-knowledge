use super::*;

fn context(
    content: &str,
    source_path: Option<&str>,
    evidence_ids: &[&str],
    entity_labels: &[&str],
) -> SupportContext {
    SupportContext {
        group_id: "group".to_owned(),
        source_scope: "repo".to_owned(),
        source_path: source_path.map(str::to_owned),
        content: content.to_owned(),
        entity_labels: entity_labels
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        evidence_ids: evidence_ids
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        modality: "text_span".to_owned(),
    }
}

#[test]
fn merge_preserves_distinct_support_and_first_available_path() {
    let mut combined = context("first", None, &["e1"], &["Alpha"]);
    combined.merge(context(
        "second",
        Some("src/lib.rs"),
        &["e1", "e2"],
        &["Alpha", "Beta"],
    ));

    assert_eq!(combined.content, "first\n\nsecond");
    assert_eq!(combined.source_path.as_deref(), Some("src/lib.rs"));
    assert_eq!(combined.evidence_ids, ["e1", "e2"]);
    assert_eq!(combined.entity_labels, ["Alpha", "Beta"]);
}

#[test]
fn merge_does_not_duplicate_existing_support() {
    let mut combined = context("same", Some("original.rs"), &["e1"], &["Alpha"]);
    combined.merge(context("same", Some("replacement.rs"), &["e1"], &["Alpha"]));

    assert_eq!(combined.content, "same");
    assert_eq!(combined.source_path.as_deref(), Some("original.rs"));
    assert_eq!(combined.evidence_ids, ["e1"]);
    assert_eq!(combined.entity_labels, ["Alpha"]);
}
