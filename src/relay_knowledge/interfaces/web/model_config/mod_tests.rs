//! Direct tests for model-configuration Web query decoding.

use serde_json::json;

use super::*;

#[test]
fn model_catalog_query_preserves_optional_refresh_intent() {
    let refresh: ModelCatalogQuery =
        serde_json::from_value(json!({ "refresh": true })).expect("refresh query should decode");
    let omitted: ModelCatalogQuery =
        serde_json::from_value(json!({})).expect("empty query should decode");

    assert_eq!(refresh.refresh, Some(true));
    assert_eq!(omitted.refresh, None);
}
