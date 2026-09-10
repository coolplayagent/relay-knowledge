use super::super::class_test_support::database;
use super::*;

#[test]
fn selects_direct_members_overloads_and_constructor_without_nested_or_sibling_types() {
    let connection = database();
    let mut members = resolve(&connection, "scope", "Target")
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|m| m.snapshot)
        .collect::<Vec<_>>();
    members.sort();
    assert_eq!(members, ["constructor", "method", "overload", "target"]);
}

#[test]
fn names_and_scopes_select_only_the_requested_class() {
    let connection = database();
    for (scope, name, expected) in [
        ("scope", "Target", true),
        ("scope", "Missing", false),
        ("old-scope", "Target", false),
        ("scope", "execute", false),
    ] {
        assert_eq!(
            resolve(&connection, scope, name).unwrap().is_some(),
            expected
        );
    }
    let nested = resolve(&connection, "scope", "Nested").unwrap().unwrap();
    assert_eq!(nested.len(), 2);
}

#[test]
fn refuses_to_truncate_class_or_member_identity_sets() {
    let connection = database();
    for i in 0..MAX_CLASSES {
        connection.execute("INSERT INTO code_repository_symbols SELECT repository_id,source_scope,?1,canonical_symbol_id,file_id,path,language_id,name,qualified_name,kind,signature,doc_comment,byte_start,byte_end,line_start,line_end,symbol_role_json FROM code_repository_symbols WHERE symbol_snapshot_id='target'", [format!("class-{i}")]).unwrap();
    }
    assert!(matches!(
        resolve(&connection, "scope", "Target"),
        Err(StorageError::CapacityExceeded(_))
    ));
    let connection = database();
    for i in 0..MAX_MEMBERS {
        connection.execute("INSERT INTO code_repository_symbols SELECT repository_id,source_scope,?1,canonical_symbol_id,file_id,path,language_id,name,qualified_name,kind,signature,doc_comment,byte_start,byte_end,line_start,line_end,symbol_role_json FROM code_repository_symbols WHERE symbol_snapshot_id='method'", [format!("method-{i}")]).unwrap();
    }
    assert!(matches!(
        resolve(&connection, "scope", "Target"),
        Err(StorageError::CapacityExceeded(_))
    ));
}

#[test]
fn unsupported_languages_and_nonclass_names_do_not_trigger_class_selection() {
    let connection = database();
    connection
        .execute(
            "UPDATE code_repository_symbols SET language_id='python'",
            [],
        )
        .unwrap();
    assert!(resolve(&connection, "scope", "Target").unwrap().is_none());
}
