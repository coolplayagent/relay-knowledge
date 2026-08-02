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
