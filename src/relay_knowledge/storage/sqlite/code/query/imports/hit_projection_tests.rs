//! Direct hit-projection contract for excerpts and resolution scoring.

use super::*;

#[test]
fn grouped_go_import_excerpts_include_source_like_siblings() {
    let excerpt = import_excerpt(
        "ctxalias context",
        None,
        &[
            ". strings".to_owned(),
            "_ embed".to_owned(),
            "ctxalias context".to_owned(),
        ],
    );

    assert!(excerpt.contains("ctxalias \"context\""), "{excerpt}");
    assert!(excerpt.contains(". \"strings\""), "{excerpt}");
    assert!(excerpt.contains("_ \"embed\""), "{excerpt}");
}

#[test]
fn import_excerpts_keep_target_symbol_context() {
    let excerpt = import_excerpt(
        "#include \"leveldb/filter_policy.h\"",
        Some("FilterPolicy"),
        &[],
    );

    assert!(excerpt.contains("leveldb/filter_policy.h"));
    assert!(excerpt.contains("FilterPolicy"));
}

#[test]
fn import_resolution_confidence_scores_resolved_edges_above_unresolved_edges() {
    assert!(
        import_resolution_confidence_bonus(2.0, "resolved", 8_000, CodeQueryKind::Imports) > 0.0
    );
    assert!(
        import_resolution_confidence_bonus(2.0, "unresolved", 2_500, CodeQueryKind::Imports) < 0.0
    );
    assert_eq!(
        import_resolution_confidence_bonus(2.0, "resolved", 8_000, CodeQueryKind::Hybrid),
        0.0
    );
}
