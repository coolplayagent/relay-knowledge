//! Direct contracts for stable SQLite evidence identities.

use super::stable_id;

#[test]
fn stable_entity_ids_are_case_insensitive_and_deterministic() {
    assert_eq!(stable_id("entity", "Rust"), "entity:bffedf1f6f66c727");
    assert_eq!(stable_id("entity", "Rust"), stable_id("entity", "rust"));
}
