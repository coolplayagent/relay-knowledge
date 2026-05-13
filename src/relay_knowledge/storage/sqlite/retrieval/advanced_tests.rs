use super::*;

#[test]
fn temporal_query_extracts_as_of_and_years() {
    let temporal = TemporalQuery::parse("as_of:2026-05-13 timeline Rust 2024");

    assert!(temporal.requested);
    assert_eq!(temporal.as_of.as_deref(), Some("2026-05-13"));
    assert!(temporal.matches(Some("2024-01-01")));
    assert!(!temporal.time_terms.contains(&"2026".to_owned()));
}

#[test]
fn temporal_query_matches_keyword_only_timelines() {
    let temporal = TemporalQuery::parse("timeline of Rust releases");

    assert!(temporal.requested);
    assert!(temporal.matches(Some("2026-05-13")));
    assert!(!temporal.matches(None));
}

#[test]
fn temporal_query_applies_case_insensitive_as_of_dates_strictly() {
    let temporal = TemporalQuery::parse("AS_OF:2026-10-01 timeline");

    assert_eq!(temporal.as_of.as_deref(), Some("2026-10-01"));
    assert!(temporal.matches(Some("2026-2-01")));
    assert!(temporal.matches(Some("2026-10-01T23:59:59Z")));
    assert!(!temporal.matches(Some("2026-11-01")));
    assert!(!temporal.matches(Some("undated event")));
}

#[test]
fn bounded_candidate_limit_scales_with_request_limit() {
    let request = GraphSearchRequest {
        query: "semantic".to_owned(),
        source_scope: None,
        graph_version: crate::domain::GraphVersion::new(1),
        limit: 10,
    };

    assert_eq!(bounded_candidate_limit(&request).unwrap(), 80);
}
