use super::super::test_support::{create_test_schema, seed_scope};
use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn dependency_pages_preserve_every_component_and_usage_without_duplicates() {
    let mut connection = Connection::open_in_memory().unwrap();
    create_test_schema(&connection);
    super::super::super::schema::initialize_schema(&connection).unwrap();
    seed_scope(&connection);
    super::super::refresh_projection(&mut connection, "scope-1").unwrap();
    connection.execute_batch("INSERT INTO software_dependency_usages SELECT 'paging-use', component_id, repository_id, source_scope, ecosystem, name, language_id, name, NULL, 'resolved', evidence_path, 1, 1, 10000, 1 FROM software_components LIMIT 1").unwrap();
    let mut request = SoftwareGlobalRequest::new(
        CodeRepositorySelector::new("repo", "commit-1", vec![], vec![]).unwrap(),
        SoftwareGlobalKind::Dependencies,
        FreshnessPolicy::AllowStale,
        500,
    )
    .unwrap();
    let expected = page(&connection, "scope-1", &request).unwrap();
    assert!(expected.next_cursor.is_none());
    assert!(!expected.components.is_empty());
    assert!(!expected.dependency_usages.is_empty());
    request.limit = 1;
    let mut ids = BTreeSet::new();
    for _ in 0..100 {
        let result = page(&connection, "scope-1", &request).unwrap();
        assert_eq!(result.components.len() + result.dependency_usages.len(), 1);
        for id in result
            .components
            .iter()
            .map(|fact| fact.component_id.clone())
            .chain(
                result
                    .dependency_usages
                    .iter()
                    .map(|fact| fact.usage_id.clone()),
            )
        {
            assert!(ids.insert(id));
        }
        let Some(cursor) = result.next_cursor else {
            break;
        };
        request.cursor = Some(cursor);
    }
    assert_eq!(
        ids.len(),
        expected.components.len() + expected.dependency_usages.len()
    );
    let mut changed = request.clone();
    changed.repository.path_filters = vec!["src".into()];
    assert!(page(&connection, "scope-1", &changed).is_err());
    assert!(page(&connection, "other-scope", &request).is_err());
    connection
        .execute(
            "UPDATE software_global_status SET projected_graph_version=99",
            [],
        )
        .unwrap();
    assert!(page(&connection, "scope-1", &request).is_err());
}

#[test]
fn cursors_reject_malformed_wrong_kind_and_oversized_input() {
    for token in ["", "no-prefix", "sw1:0", "sw1:zz", "sw1:é", "sw1:00"] {
        assert!(Cursor::read(Some(token), 7, 4).is_err());
    }
    let cursor = Cursor {
        fingerprint: 7,
        phase: 3,
        key: "key:123".into(),
    };
    let token = cursor.encode().unwrap();
    assert_eq!(Cursor::read(Some(&token), 7, 4).unwrap().key, "key:123");
    assert!(Cursor::read(Some(&token), 8, 4).is_err());
    assert!(Cursor::read(Some(&token), 7, 2).is_err());
    assert!(Cursor::read(Some(&format!("sw1:{}", "00".repeat(2048))), 7, 4).is_err());
    let large = Cursor {
        key: "x".repeat(3000),
        ..cursor
    };
    assert!(large.encode().is_err());
}
