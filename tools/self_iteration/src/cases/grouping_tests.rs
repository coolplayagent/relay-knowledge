use super::*;

#[test]
fn groups_cases_by_repository_in_stable_key_order() {
    let cases = vec![
        serde_json::json!({"id":"two","repository":"zeta"}),
        serde_json::json!({"id":"one","repository":"alpha"}),
        serde_json::json!({"id":"three","repository":"zeta"}),
    ];

    let grouped = objects_by_repository(&cases);

    assert_eq!(
        grouped.keys().cloned().collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );
    assert_eq!(grouped["alpha"].len(), 1);
    assert_eq!(grouped["zeta"].len(), 2);
}

#[test]
fn ignores_cases_without_string_repository_identity() {
    let cases = vec![
        serde_json::json!({"id":"missing"}),
        serde_json::json!({"id":"numeric","repository":42}),
        serde_json::json!({"id":"valid","repository":"repo"}),
    ];

    let grouped = objects_by_repository(&cases);

    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped["repo"][0]["id"], "valid");
}
