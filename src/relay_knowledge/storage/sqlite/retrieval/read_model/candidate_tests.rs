use super::evidence_group_key;

#[test]
fn evidence_group_key_has_a_stable_namespace() {
    assert_eq!(evidence_group_key("ev-1"), "evidence_group:ev-1");
}
