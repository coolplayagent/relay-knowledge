use serde_json::json;

use super::{ProductBinaryProfile, run_matches_product_binary_profile};

#[test]
fn non_smoke_profiles_select_the_release_product_binary() {
    for profile in ["fast", "full", "exhaustive"] {
        assert_eq!(
            ProductBinaryProfile::for_evaluation_profile(profile),
            Some(ProductBinaryProfile::Release)
        );
    }
    assert_eq!(ProductBinaryProfile::for_evaluation_profile("smoke"), None);
}

#[test]
fn legacy_history_uses_the_previous_fast_debug_and_non_fast_release_contract() {
    let legacy = json!({"profile": "fast"});
    assert!(!run_matches_product_binary_profile(&legacy, "fast"));

    let current = json!({"profile": "fast", "product_binary_profile": "release"});
    assert!(run_matches_product_binary_profile(&current, "fast"));

    let legacy_full = json!({"profile": "full"});
    assert!(run_matches_product_binary_profile(&legacy_full, "full"));
}

#[test]
fn smoke_history_records_that_no_product_binary_was_selected() {
    let current = json!({"profile": "smoke", "product_binary_profile": null});
    assert!(run_matches_product_binary_profile(&current, "smoke"));
    assert!(!run_matches_product_binary_profile(
        &json!({"profile": "smoke"}),
        "smoke"
    ));
}
