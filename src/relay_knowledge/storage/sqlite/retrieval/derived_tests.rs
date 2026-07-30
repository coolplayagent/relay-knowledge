use super::*;

#[test]
fn bounded_candidate_limit_scales_with_request_limit() {
    let request = GraphSearchRequest {
        query: "semantic".to_owned(),
        source_scope: None,
        graph_version: crate::domain::GraphVersion::new(1),
        limit: 10,
        disabled_retriever_sources: Vec::new(),
    };

    assert_eq!(bounded_candidate_limit(&request), 80);
}

#[test]
fn derived_scope_version_filter_uses_indexable_scope_predicate() {
    let scoped_request = GraphSearchRequest {
        query: "semantic".to_owned(),
        source_scope: Some("repo-a".to_owned()),
        graph_version: crate::domain::GraphVersion::new(7),
        limit: 10,
        disabled_retriever_sources: Vec::new(),
    };
    let unscoped_request = GraphSearchRequest {
        source_scope: None,
        ..scoped_request.clone()
    };

    let (scoped_condition, scoped_values) =
        derived_scope_version_filter(&scoped_request, "doc").expect("scoped filter should build");
    let (unscoped_condition, unscoped_values) =
        derived_scope_version_filter(&unscoped_request, "doc")
            .expect("unscoped filter should build");

    assert_eq!(
        scoped_condition,
        "doc.source_scope = ? AND doc.created_graph_version <= ?"
    );
    assert_eq!(
        scoped_values,
        vec![Value::Text("repo-a".to_owned()), Value::Integer(7)]
    );
    assert_eq!(unscoped_condition, "doc.created_graph_version <= ?");
    assert_eq!(unscoped_values, vec![Value::Integer(7)]);
}

#[test]
fn derived_candidate_filter_caps_query_terms() {
    let query_terms = (0..40)
        .map(|index| format!("term{index}"))
        .collect::<BTreeSet<_>>();
    let fields = [
        DerivedCandidateField::contains("lower(content)"),
        DerivedCandidateField::contains("lower(source_path)"),
        DerivedCandidateField::contains("lower(entity_labels_json)"),
    ];

    let (condition, ranking, values) = derived_candidate_filter(&query_terms, &fields);

    assert!(condition.contains("lower(content) LIKE ? ESCAPE '\\'"));
    assert!(ranking.contains("CASE WHEN lower(content) LIKE ? ESCAPE '\\'"));
    assert_eq!(values.len(), MAX_DERIVED_QUERY_TERMS * fields.len() * 2);
}

#[test]
fn derived_candidate_filter_prefers_high_signal_terms_when_capped() {
    let mut query_terms = (0..20)
        .map(|index| format!("a{}", char::from(b'a' + index as u8)))
        .collect::<BTreeSet<_>>();
    query_terms.insert("zzcriticalidentity".to_owned());
    let fields = [DerivedCandidateField::contains("lower(content)")];

    let (_, _, values) = derived_candidate_filter(&query_terms, &fields);
    let patterns = values
        .iter()
        .filter_map(|value| match value {
            Value::Text(pattern) => Some(pattern.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(patterns.contains(&"%zzcriticalidentity%"));
    assert!(!patterns.contains(&"%at%"));
    assert_eq!(patterns.len(), MAX_DERIVED_QUERY_TERMS * 2);
}

#[test]
fn derived_candidate_filter_uses_literal_patterns_for_identifiers() {
    let query_terms = BTreeSet::from(["retry_policy".to_owned()]);

    let (_, _, contains_values) = derived_candidate_filter(
        &query_terms,
        &[DerivedCandidateField::contains("lower(content)")],
    );
    let (_, _, token_values) = derived_candidate_filter(
        &query_terms,
        &[DerivedCandidateField::json_token(
            "lower(token_signature_json)",
        )],
    );

    assert_eq!(
        contains_values,
        vec![
            Value::Text("%retry\\_policy%".to_owned()),
            Value::Text("%retry\\_policy%".to_owned()),
        ]
    );
    assert_eq!(
        token_values,
        vec![
            Value::Text("%\"retry\\_policy\"%".to_owned()),
            Value::Text("%\"retry\\_policy\"%".to_owned()),
        ]
    );
}

#[test]
fn query_vector_cache_reuses_vectors_by_dimension() {
    let mut cache = QueryVectorCache::new("semantic vector freshness");
    let first = cache.vector(16).to_vec();
    let second = cache.vector(16).to_vec();

    assert_eq!(first, second);
    assert_eq!(cache.vectors.len(), 1);
    assert_eq!(cache.vector(8).len(), 8);
    assert_eq!(cache.vectors.len(), 2);
}

#[test]
fn vector_source_score_uses_lexical_coverage_as_bounded_tie_breaker() {
    let fuller_match = vector_source_score(0.40, 4.0, 4);
    let sparse_match = vector_source_score(0.42, 1.0, 4);
    let stronger_vector_match = vector_source_score(0.70, 1.0, 4);

    assert!(fuller_match > sparse_match);
    assert!(stronger_vector_match > fuller_match);
    assert_eq!(vector_source_score(-0.5, 4.0, 4), 0.0);
    assert_eq!(vector_source_score(0.5, 4.0, 0), 0.5);
}
