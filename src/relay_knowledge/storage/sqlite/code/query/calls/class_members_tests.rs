use super::super::class_test_support::database;
use super::*;

#[test]
fn language_selection_precedes_the_type_candidate_budget() {
    let db = database();
    for index in 0..65 {
        db.execute("INSERT INTO code_repository_symbols SELECT repository_id,source_scope,?1,canonical_symbol_id,file_id,'target.py','python',name,qualified_name,kind,signature,doc_comment,byte_start,byte_end,line_start,line_end,symbol_role_json,json_set(type_owner_json,'$.identity',?2),?2 FROM code_repository_symbols WHERE symbol_snapshot_id='target'", [format!("python-{index}"), format!("python|module{index}|Target")]).unwrap();
    }
    assert_eq!(
        resolve(
            &db,
            "scope",
            "Target",
            "AND language_id=?",
            &[Value::Text("java".into())]
        )
        .unwrap()
        .unwrap()
        .len(),
        4
    );
    assert!(
        resolve(
            &db,
            "scope",
            "Target",
            "AND language_id=?",
            &[Value::Text("c".into())]
        )
        .unwrap()
        .is_some_and(|members| members.is_empty())
    );
}

#[test]
fn selects_direct_members_overloads_and_constructor_without_nested_or_sibling_types() {
    let connection = database();
    let mut members = resolve(&connection, "scope", "Target", "", &[])
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
            resolve(&connection, scope, name, "", &[])
                .unwrap()
                .is_some(),
            expected
        );
    }
    let nested = resolve(&connection, "scope", "Nested", "", &[])
        .unwrap()
        .unwrap();
    assert_eq!(nested.len(), 2);
}

#[test]
fn refuses_to_truncate_class_or_member_identity_sets() {
    let connection = database();
    for i in 0..MAX_CLASSES {
        connection.execute("INSERT INTO code_repository_symbols SELECT repository_id,source_scope,?1,canonical_symbol_id,file_id,path,language_id,name,qualified_name,kind,signature,doc_comment,byte_start,byte_end,line_start,line_end,symbol_role_json,type_owner_json,type_owner_identity FROM code_repository_symbols WHERE symbol_snapshot_id='target'", [format!("class-{i}")]).unwrap();
    }
    assert!(matches!(
        resolve(&connection, "scope", "Target", "", &[]),
        Err(StorageError::CapacityExceeded(_))
    ));
    let connection = database();
    for i in 0..MAX_MEMBERS {
        connection.execute("INSERT INTO code_repository_symbols SELECT repository_id,source_scope,?1,canonical_symbol_id,file_id,path,language_id,name,qualified_name,kind,signature,doc_comment,byte_start,byte_end,line_start,line_end,symbol_role_json,type_owner_json,type_owner_identity FROM code_repository_symbols WHERE symbol_snapshot_id='method'", [format!("method-{i}")]).unwrap();
    }
    assert!(matches!(
        resolve(&connection, "scope", "Target", "", &[]),
        Err(StorageError::CapacityExceeded(_))
    ));
}

#[test]
fn persisted_type_ownership_is_language_independent() {
    let connection = database();
    connection
        .execute(
            "UPDATE code_repository_symbols SET language_id='python'",
            [],
        )
        .unwrap();
    assert_eq!(
        resolve(&connection, "scope", "Target", "", &[])
            .unwrap()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn code_index_persistence_performance_suite_type_ownership_uses_indexed_membership() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let db = database();
    db.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<16384)
        INSERT INTO code_repository_symbols (repository_id,source_scope,symbol_snapshot_id,canonical_symbol_id,file_id,path,language_id,name,qualified_name,kind,signature,byte_start,byte_end,line_start,line_end,type_owner_identity,type_owner_json)
        SELECT 'repo','scope','noise-'||x,'noise-'||x,'target-file','noise.rs','rust','noise','noise','method','',0,1,1,1,'rust|noise|'||x,'{\"relation\":\"direct_member\"}' FROM n;").unwrap();
    let steps = Arc::new(AtomicUsize::new(0));
    let measured = steps.clone();
    db.progress_handler(
        100,
        Some(move || {
            measured.fetch_add(100, Ordering::Relaxed);
            false
        }),
    );
    let members = resolve(&db, "scope", "Target", "", &[]).unwrap().unwrap();
    db.progress_handler(0, None::<fn() -> bool>);
    assert_eq!(members.len(), 4);
    assert!(
        steps.load(Ordering::Relaxed) < 5000,
        "membership must not scan the unrelated symbol tail"
    );
}
