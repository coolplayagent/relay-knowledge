//! Direct unit contract for named-import binding and usage-term extraction.

use super::*;

#[test]
fn import_binding_terms_keep_specific_local_names() {
    assert_eq!(
        named_import_binding_terms(
            "import { JsonObject, optionalArray, type ProviderShared as Shared } from './shared'",
        ),
        vec![
            "jsonobject".to_owned(),
            "object".to_owned(),
            "optionalarray".to_owned(),
            "optional".to_owned(),
            "array".to_owned(),
            "shared".to_owned()
        ]
    );
}

#[test]
fn import_binding_terms_for_query_keep_only_matching_binding() {
    assert_eq!(
        named_import_binding_terms_for_query(
            "import { Target as LocalTarget, VeryCommon } from './module'",
            "Target",
            Some("Target"),
        ),
        vec![
            "localtarget".to_owned(),
            "local".to_owned(),
            "target".to_owned()
        ]
    );
}

#[test]
fn usage_terms_split_specific_snake_and_camel_identifiers_without_duplicates() {
    assert_eq!(
        import_usage_identifier_terms("HTTPClient http_client HTTPClient"),
        ["httpclient", "client", "http_client"]
    );
}

#[test]
fn terminal_binding_terms_cover_namespaces_and_wildcard_companions() {
    assert_eq!(
        terminal_import_binding_terms("use Illuminate\\Container\\Container;"),
        ["container"]
    );
    assert_eq!(
        terminal_import_binding_terms("import dotty.tools.dotc.core.Contexts.*"),
        ["contexts", "context"]
    );
    assert_eq!(
        terminal_import_binding_terms("import compiler.Contexts._"),
        ["contexts", "context"]
    );
    assert_eq!(
        terminal_import_binding_terms("use Vendor\\Client as HttpClient;"),
        ["httpclient", "client"]
    );
    assert_eq!(
        terminal_import_binding_terms("clientset k8s.io/client-go/kubernetes"),
        ["clientset"]
    );
    assert_eq!(
        terminal_import_binding_terms("k8s.io/apimachinery/pkg/runtime"),
        ["runtime"]
    );
    assert!(terminal_import_binding_terms("_ embed").is_empty());
    assert!(terminal_import_binding_terms(". example.org/dot/import").is_empty());
}

#[test]
fn terminal_binding_terms_avoid_named_and_dynamic_import_paths() {
    assert!(terminal_import_binding_terms("import { Client } from './client'").is_empty());
    assert!(terminal_import_binding_terms("await import('./client')").is_empty());
    assert!(terminal_import_binding_terms("./client").is_empty());
}

#[test]
fn query_local_binding_terms_keep_structured_and_terminal_identifiers() {
    assert_eq!(
        query_local_binding_terms("ExtendedBeanInfo org.springframework.util.ObjectUtils"),
        ["extendedbeaninfo", "objectutils"]
    );
    assert_eq!(
        query_local_binding_terms("service org.springframework.util.ObjectUtils"),
        ["objectutils"]
    );
    assert_eq!(
        query_local_binding_terms("cache_consumer store/cache.hpp"),
        ["cache_consumer", "hpp"]
    );
}
