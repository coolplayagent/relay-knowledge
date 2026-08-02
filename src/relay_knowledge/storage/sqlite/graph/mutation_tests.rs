use std::collections::BTreeMap;

use rusqlite::Connection;

use super::validate_parent_evidence;

#[test]
fn parent_evidence_must_be_distinct_and_share_the_batch_scope() {
    let connection = Connection::open_in_memory().expect("connection should open");
    let scopes = BTreeMap::from([("parent".to_owned(), "docs".to_owned())]);

    validate_parent_evidence(&connection, &scopes, "child", "docs", "parent")
        .expect("same-scope parent should validate");

    let cross_scope = validate_parent_evidence(&connection, &scopes, "child", "other", "parent")
        .expect_err("cross-scope parent should fail");
    assert!(cross_scope.to_string().contains("instead of 'other'"));

    let self_parent = validate_parent_evidence(&connection, &scopes, "parent", "docs", "parent")
        .expect_err("self-parent should fail");
    assert!(
        self_parent
            .to_string()
            .contains("must reference a different evidence record")
    );
}
