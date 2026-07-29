use super::{
    affected_candidate_matches_changed_path, architecture_layer, domain_token, module_key,
    path_domain, route_domain, topological_tour,
};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn architecture_layer_uses_path_boundaries() {
    assert_eq!(
        architecture_layer("src/relay_knowledge/application/service.rs"),
        "application"
    );
    assert_eq!(
        architecture_layer("src/relay_knowledge/storage/sqlite/code.rs"),
        "storage"
    );
    assert_eq!(architecture_layer("tests/relay_knowledge/main.rs"), "tests");
    assert_eq!(
        architecture_layer("src/application/client.rs"),
        "application"
    );
    assert_eq!(architecture_layer("src/webhook/handler.rs"), "source");
}

#[test]
fn domain_rules_skip_generic_roots_and_api_prefixes() {
    assert_eq!(route_domain("/api/v1/users"), Some("users".to_owned()));
    assert_eq!(route_domain("/api/orders"), Some("orders".to_owned()));
    assert_eq!(
        route_domain("/api/[tenant]/orders"),
        Some("orders".to_owned())
    );
    assert_eq!(
        route_domain("/api/{tenant}/orders"),
        Some("orders".to_owned())
    );
    assert_eq!(
        route_domain("/api/:tenant/orders"),
        Some("orders".to_owned())
    );
    assert_eq!(route_domain("/api/*/orders"), Some("orders".to_owned()));
    assert_eq!(
        path_domain("src/relay_knowledge/application/service.rs"),
        Some("application".to_owned())
    );
    assert_eq!(path_domain("src/api/users.rs"), Some("users".to_owned()));
    assert_eq!(
        path_domain("app/controllers/orders.py"),
        Some("orders".to_owned())
    );
    assert_eq!(
        path_domain("src/orders/service.rs"),
        Some("orders".to_owned())
    );
    assert_eq!(
        path_domain("src/myapp/orders/service.py"),
        Some("orders".to_owned())
    );
    assert_eq!(
        path_domain("app/shop/payments/config.py"),
        Some("payments".to_owned())
    );
    assert_eq!(
        path_domain("crates/auth/src/users.rs"),
        Some("auth".to_owned())
    );
    assert_eq!(
        path_domain("packages/billing/app/orders.py"),
        Some("billing".to_owned())
    );
    assert_eq!(path_domain("src/users.rs"), Some("users".to_owned()));
    assert_eq!(path_domain("app/orders.py"), Some("orders".to_owned()));
    assert_eq!(path_domain("src/lib.rs"), None);
    assert_eq!(path_domain("src/mod.rs"), None);
    assert_eq!(path_domain("package.json"), None);
}

#[test]
fn domain_tokens_skip_boolean_feature_flag_prefixes() {
    assert_eq!(domain_token("enable_payments"), Some("payments".to_owned()));
    assert_eq!(domain_token("enablePayments"), Some("payments".to_owned()));
    assert_eq!(domain_token("use_checkout"), Some("checkout".to_owned()));
    assert_eq!(domain_token("useCheckout"), Some("checkout".to_owned()));
    assert_eq!(domain_token("is_orders_enabled"), Some("orders".to_owned()));
    assert_eq!(domain_token("isOrdersEnabled"), Some("orders".to_owned()));
    assert_eq!(
        domain_token("rollout.billing.v2"),
        Some("billing".to_owned())
    );
}

#[test]
fn module_keys_skip_common_source_roots() {
    assert_eq!(module_key("Cargo.toml"), "root");
    assert_eq!(module_key("package.json"), "root");
    assert_eq!(module_key("src/main.rs"), "root");
    assert_eq!(module_key("src/lib.rs"), "root");
    assert_eq!(module_key("src/application/service.rs"), "application");
    assert_eq!(
        module_key("src/relay_knowledge/application/service.rs"),
        "application"
    );
    assert_eq!(module_key("crates/auth/src/lib.rs"), "auth");
    assert_eq!(module_key("packages/api/src/routes.ts"), "api");
    assert_eq!(module_key("src/myapp/orders/service.py"), "orders");
    assert_eq!(module_key("app/shop/payments/config.py"), "payments");
    assert_eq!(module_key("docs/en/index.md"), "docs");
}

#[test]
fn affected_candidate_matches_root_changed_file_siblings() {
    assert!(affected_candidate_matches_changed_path(
        "Cargo.toml",
        "Cargo.lock"
    ));
    assert!(affected_candidate_matches_changed_path(
        "package.json",
        "README.md"
    ));
    assert!(!affected_candidate_matches_changed_path(
        "Cargo.toml",
        "src/lib.rs"
    ));
    assert!(affected_candidate_matches_changed_path(
        "src/myapp/orders/service.py",
        "src/myapp/orders/service_test.py"
    ));
    assert!(!affected_candidate_matches_changed_path(
        "src/myapp/orders/service.py",
        "src/myapp/payments/service_test.py"
    ));
}

#[test]
fn topological_tour_reports_cycles() {
    let modules = ["a".to_owned(), "b".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let graph = BTreeMap::from([
        ("a".to_owned(), BTreeSet::from(["b".to_owned()])),
        ("b".to_owned(), BTreeSet::from(["a".to_owned()])),
    ]);

    let (tour, cycle) = topological_tour(&modules, &graph);

    assert!(cycle);
    assert_eq!(tour.len(), 2);
}
