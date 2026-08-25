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

#[test]
fn broad_hybrid_type_documentation_requires_surface_and_semantic_coverage() {
    let request = CodeRetrievalRequest::new(
        "central controller servlet dispatch web mvc framework",
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
            .expect("selector should validate"),
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let strong = hybrid_type_documentation_surface_bonus(
        &request.query,
        "class",
        "GatewayServlet",
        "public class GatewayServlet extends WebFramework",
        Some("Central MVC controller dispatch for web requests."),
        &request,
    );
    let weak = hybrid_type_documentation_surface_bonus(
        &request.query,
        "class",
        "GatewayServlet",
        "public class GatewayServlet",
        Some("Creates a gateway."),
        &request,
    );
    let non_type = hybrid_type_documentation_surface_bonus(
        &request.query,
        "method",
        "dispatchWebRequest",
        "void dispatchWebRequest()",
        Some("Central MVC controller servlet in a framework."),
        &request,
    );

    assert!(strong > 0.0);
    assert_eq!(weak, 0.0);
    assert_eq!(non_type, 0.0);
}

#[test]
fn exact_hybrid_type_identity_prefers_concrete_production_declarations() {
    let request = CodeRetrievalRequest::new(
        "GatewayClient",
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), vec!["go".to_owned()])
            .expect("selector should validate"),
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let identity = SymbolIdentityQuery::from_query(&request.query);
    let mut concrete = symbol_row(
        "GatewayClient",
        "client::GatewayClient",
        "struct",
        "type GatewayClient struct",
    );
    concrete.path = "client/gateway.go".to_owned();
    let mut interface = symbol_row(
        "GatewayClient",
        "client::GatewayClient",
        "interface",
        "type GatewayClient interface",
    );
    interface.path = "client/interface.go".to_owned();
    let mut fake = symbol_row(
        "GatewayClient",
        "client::GatewayClient",
        "struct",
        "type GatewayClient struct",
    );
    fake.path = "client/fake/fake.go".to_owned();

    let concrete_bonus = hybrid_exact_type_role_bonus(identity.as_ref(), &concrete, &request);
    let interface_bonus = hybrid_exact_type_role_bonus(identity.as_ref(), &interface, &request);
    let fake_bonus = hybrid_exact_type_role_bonus(identity.as_ref(), &fake, &request);

    assert!(concrete_bonus > interface_bonus);
    assert!(concrete_bonus > fake_bonus);
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
