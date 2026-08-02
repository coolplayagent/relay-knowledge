use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy, RepositoryCodeRange};
use crate::storage::sqlite::code::query::rows::SymbolRow;

#[test]
fn typed_function_surface_extracts_exported_name_type_and_signature_terms() {
    let surface = typed_function_value_surface(
        "export const onSuccess: ResultCallback = value => notify(value)",
    )
    .expect("typed function surface should parse");

    assert!(surface.exported);
    assert!(surface.name_terms.contains(&"success".to_owned()));
    assert!(surface.declared_type_terms.contains(&"callback".to_owned()));
    assert!(surface.signature_terms.contains(&"notify".to_owned()));
}

#[test]
fn typed_function_bonus_requires_hybrid_query_coverage() {
    let query = "typed callback on success payload result notify";
    let request = CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), vec!["typescript".to_owned()])
            .expect("selector should validate"),
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let row = SymbolRow {
        symbol_snapshot_id: "symbol".to_owned(),
        canonical_symbol_id: "repo://repo/src::callback::onSuccess".to_owned(),
        file_id: "file".to_owned(),
        path: "src/callback.ts".to_owned(),
        language_id: "typescript".to_owned(),
        is_generated: false,
        signature:
            "export const onSuccess: (payload: ResultValue) => void = value => notify(value)"
                .to_owned(),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 0, end: 1 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        name: "onSuccess".to_owned(),
        qualified_name: "callback.onSuccess".to_owned(),
        kind: "constant".to_owned(),
        previous_symbol_context_start: None,
    };
    let parsed_query =
        TypedFunctionValueQuery::from_request(query, &request).expect("query should qualify");

    assert!(typed_function_value_surface_bonus(&row, Some(&parsed_query), false) > 1.0);
}
