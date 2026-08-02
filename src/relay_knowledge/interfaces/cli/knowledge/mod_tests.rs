//! Direct contracts for knowledge CLI parsing.

use super::*;

#[test]
fn query_parser_preserves_scope_limit_and_freshness() {
    let action = parse_query(&[
        "durable".to_owned(),
        "tasks".to_owned(),
        "--source".to_owned(),
        "repo".to_owned(),
        "--limit".to_owned(),
        "7".to_owned(),
        "--freshness".to_owned(),
        "graph-only".to_owned(),
    ])
    .expect("knowledge query should parse");

    assert_eq!(
        action,
        CliAction::Query {
            query: "durable tasks".to_owned(),
            source_scope: Some("repo".to_owned()),
            limit: 7,
            freshness: FreshnessPolicy::GraphOnly,
        }
    );
}

#[test]
fn index_refresh_parser_validates_every_requested_layer() {
    let action = parse_index(&[
        "refresh".to_owned(),
        "--kind".to_owned(),
        "bm25".to_owned(),
        "--kind".to_owned(),
        "semantic".to_owned(),
        "--kind".to_owned(),
        "vector".to_owned(),
    ])
    .expect("all retrieval layers should parse");

    assert_eq!(
        action,
        CliAction::IndexRefresh {
            kinds: vec![IndexKind::Bm25, IndexKind::Semantic, IndexKind::Vector],
        }
    );
    assert_eq!(
        parse_index_kind("dense").expect_err("unknown layer should fail"),
        CliError::InvalidIndexKind("dense".to_owned())
    );
}
