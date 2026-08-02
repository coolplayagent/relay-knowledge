use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy, RepositoryCodeRange};

#[test]
fn scoped_definition_identity_bonus_prefers_member_over_owner_type() {
    let request = CodeRetrievalRequest::new(
        "RuntimeService::dispatch",
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate"),
        CodeQueryKind::Definition,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let identity = SymbolIdentityQuery::from_query(&request.query);
    let owner = symbol_row(
        "RuntimeService",
        "service::RuntimeService::dispatch",
        "struct",
        "pub struct RuntimeService;",
    );
    let member = symbol_row(
        "dispatch",
        "service::RuntimeService::dispatch",
        "method",
        "pub fn dispatch(&self) {}",
    );

    assert_eq!(
        scoped_member_identity_bonus(identity.as_ref(), &owner, &request),
        0.0
    );
    assert!(
        scoped_member_identity_bonus(identity.as_ref(), &member, &request)
            > type_symbol_identity_bonus(identity.as_ref(), &owner, &request)
    );
}

fn symbol_row(name: &str, qualified_name: &str, kind: &str, signature: &str) -> SymbolRow {
    SymbolRow {
        symbol_snapshot_id: "symbol".to_owned(),
        canonical_symbol_id: format!("repo://repo/{qualified_name}"),
        file_id: "file".to_owned(),
        path: "src/service.rs".to_owned(),
        language_id: "rust".to_owned(),
        is_generated: false,
        signature: signature.to_owned(),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 0, end: 0 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        name: name.to_owned(),
        qualified_name: qualified_name.to_owned(),
        kind: kind.to_owned(),
        previous_symbol_context_start: None,
    }
}
