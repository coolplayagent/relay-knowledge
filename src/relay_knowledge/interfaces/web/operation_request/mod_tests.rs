//! Direct tests for Web operation request mapping.

use serde_json::json;

use super::*;

#[test]
fn knowledge_map_history_page_requires_positive_bounded_inputs() {
    let page = knowledge_map_history_page(&serde_json::json!({
        "repository": " relay ",
        "from_version": 2,
        "limit": 16
    }))
    .expect("valid page");
    assert_eq!(page.repository, "relay");
    assert_eq!(page.from_version, 2);
    assert_eq!(page.limit, 16);
    assert!(
        knowledge_map_history_page(&serde_json::json!({
            "repository": "relay",
            "from_version": 0,
            "limit": 16
        }))
        .is_err()
    );
    assert!(
        knowledge_map_history_page(&serde_json::json!({
            "repository": "relay",
            "from_version": 2,
            "limit": 0
        }))
        .is_err()
    );
    assert!(
        knowledge_map_history_page(&serde_json::json!({
            "from_version": 2,
            "limit": 16
        }))
        .is_err()
    );
}

#[test]
fn scalar_fields_enforce_type_and_range_contracts() {
    let payload = json!({
        "name": "value",
        "limit": 4,
        "enabled": false,
        "kinds": ["bm25", 42],
    });

    assert_eq!(
        string_field(&payload, "name").expect("name should parse"),
        "value"
    );
    assert_eq!(
        usize_field(&payload, "limit").expect("limit should parse"),
        4
    );
    assert_eq!(
        optional_bool_field(&payload, "enabled").expect("boolean should parse"),
        Some(false)
    );
    assert!(string_field(&json!({"name": "  "}), "name").is_err());
    assert!(usize_field(&json!({"limit": 0}), "limit").is_err());
    assert!(optional_bool_field(&json!({"enabled": "false"}), "enabled").is_err());
    assert_eq!(
        string_array_field(&payload, "missing")
            .expect_err("array should be required")
            .message,
        "missing must be an array"
    );
    assert_eq!(
        string_array_field(&payload, "kinds")
            .expect_err("array values should be strings")
            .message,
        "kinds contains a non-string value"
    );
}

#[test]
fn code_selector_normalizes_optional_filters() {
    let selector = code_selector(&json!({
        "alias": "relay-knowledge",
        "ref": "main",
        "path_filters": [" src ", "docs"],
        "language_filters": [" rust "],
    }))
    .expect("selector should parse");

    assert_eq!(selector.repository, "relay-knowledge");
    assert_eq!(selector.ref_selector, "main");
    assert_eq!(selector.path_filters, vec!["src", "docs"]);
    assert_eq!(selector.language_filters, vec!["rust"]);
}

#[test]
fn maps_operation_domain_request_variants() {
    let payload = json!({
        "root_path": "/repo",
        "alias": "relay",
        "ref": "main",
        "path_filters": [" src/ ", "tests"],
        "language_filters": [" rust "],
        "query": "handler",
        "kind": "definition",
        "freshness": "wait-until-fresh",
        "limit": 7,
        "base_ref": "main",
        "head_ref": "feature"
    });

    let registration = code_register_request(&payload).expect("registration should parse");
    assert_eq!(registration.alias, "relay");
    assert_eq!(registration.path_filters, ["src/", "tests"]);
    assert_eq!(registration.language_filters, ["rust"]);

    let default_alias =
        code_register_request(&json!({"root_path": "/repo"})).expect("default alias should parse");
    assert!(default_alias.alias.is_empty());
    assert!(code_register_request(&json!({"root_path": "/repo", "alias": 123})).is_err());

    let query = code_query_request(&payload).expect("code query should parse");
    assert_eq!(query.code_query_kind, CodeQueryKind::Definition);
    assert_eq!(query.freshness_policy, FreshnessPolicy::WaitUntilFresh);

    let impact = code_impact_request(&payload).expect("impact request should parse");
    assert_eq!(impact.base_ref, "main");
    assert_eq!(impact.head_ref, "feature");

    let software = code_software_request(&json!({
        "alias": "relay",
        "ref": "main",
        "kind": "dependencies",
        "freshness": "wait-until-fresh",
        "limit": 7
    }))
    .expect("software request should parse");
    assert_eq!(software.kind, SoftwareGlobalKind::Dependencies);
    assert_eq!(software.freshness_policy, FreshnessPolicy::WaitUntilFresh);
}

#[test]
fn enum_parsers_reject_unknown_values() {
    assert_eq!(
        parse_freshness("wait-until-fresh").expect("freshness should parse"),
        FreshnessPolicy::WaitUntilFresh
    );
    assert!(parse_freshness("eventually").is_err());
    assert_eq!(
        parse_code_query_kind("sbom").expect("query kind should parse"),
        CodeQueryKind::Sbom
    );
    assert_eq!(
        parse_code_query_kind("impact")
            .expect_err("query kind should be rejected")
            .message,
        "unsupported code query kind 'impact'"
    );
    assert!(parse_software_kind("unknown").is_err());
}
