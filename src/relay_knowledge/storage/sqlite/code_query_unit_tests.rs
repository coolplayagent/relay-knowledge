use super::*;

#[test]
fn path_filters_accept_trailing_slashes() {
    assert!(path_matches_filter("src/lib.rs", "src/"));
    assert!(path_matches_filter("src/lib.rs", "src"));
    assert!(path_matches_filter("src/lib.rs", "."));
    assert!(path_matches_filter("src/lib.rs", "./"));
    assert!(path_matches_filter("src/lib.rs", "./src"));
    assert!(!path_matches_filter("src-other/lib.rs", "src/"));
}

#[test]
fn candidate_condition_preserves_all_query_terms() {
    let (condition, values) = candidate_condition(&["lower(name)", "lower(path)"], "retry budget");

    assert!(condition.contains("lower(name) LIKE ?"));
    assert_eq!(values.len(), 4);
    assert!(values.contains(&Value::Text("%retry%".to_owned())));
    assert!(values.contains(&Value::Text("%budget%".to_owned())));
}

#[test]
fn candidate_condition_caps_bind_values_for_long_queries() {
    let query = (0..300)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let fields = ["a", "b", "c", "d", "e"];

    let (_, values) = candidate_condition(&fields, &query);

    assert!(values.len() <= MAX_CANDIDATE_BIND_VALUES);
}

#[test]
fn symbol_fts_query_uses_any_term_for_fuzzy_recall() {
    assert_eq!(
        symbol_fts_match_query("checkpoint metadata version constant"),
        "\"checkpoint\" OR \"metadata\" OR \"version\" OR \"constant\""
    );
    assert_eq!(
        fts_match_query("checkpoint metadata version constant"),
        "\"checkpoint\" \"metadata\" \"version\" \"constant\""
    );
}

#[test]
fn score_text_matches_identifier_parts_inside_snake_case_names() {
    let score = score_text(
        "archive output directory",
        ["def archive_output_dir(output_dir: Path) -> Path:"],
    );

    assert!(score >= 4.0);
}
