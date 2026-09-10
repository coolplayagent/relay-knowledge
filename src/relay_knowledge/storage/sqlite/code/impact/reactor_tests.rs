use super::*;

#[test]
fn empty_changes_do_not_require_reactor_tables() {
    let connection = Connection::open_in_memory().unwrap();
    assert!(
        crate::storage::sqlite::maven::reactor::downstream(
            &connection,
            "scope",
            &BTreeSet::new(),
            &[]
        )
        .unwrap()
        .is_empty()
    );
}
