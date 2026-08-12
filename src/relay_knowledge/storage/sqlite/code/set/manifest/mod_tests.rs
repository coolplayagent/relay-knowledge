use rusqlite::Connection;

use crate::{
    domain::{CodeRepositorySetMember, CodeRepositorySetMemberStatus},
    storage::StorageError,
};

use super::{MAX_REPOSITORY_SET_MANIFEST_BYTES, MAX_REPOSITORY_SET_MANIFEST_ITEMS, consume_budget};
use super::{MAX_REPOSITORY_SET_MANIFEST_CHUNKS, manifest_module_prefixes_for_members};

#[test]
fn manifest_prefix_discovery_is_scoped_to_each_repository_set_member() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_chunks (
                source_scope TEXT NOT NULL,
                chunk_id TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL
            );
            INSERT INTO code_repository_chunks (source_scope, chunk_id, path, content)
            VALUES
                ('scope-go', 'go-module', 'services/api/go.mod', 'module example.com/api'),
                ('scope-npm', 'package', 'packages/ui/package.json',
                 '{\"name\":\"@example/ui\",\"main\":\"src/index.ts\"}'),
                ('scope-noise', 'ignored', 'README.md', '# ignored');
            ",
        )
        .expect("manifest chunks should insert");
    let members = vec![
        member("scope-go"),
        member("scope-npm"),
        member("scope-noise"),
    ];

    let prefixes = manifest_module_prefixes_for_members(&mut connection, &members)
        .expect("manifest prefixes should load");

    assert_eq!(prefixes.len(), 2);
    assert_eq!(prefixes["scope-go"][0].module_key, "example.com.api");
    assert_eq!(prefixes["scope-npm"][0].module_key, "@example.ui");
    assert!(!prefixes.contains_key("scope-noise"));
}

#[test]
fn manifest_chunk_capacity_is_shared_across_repository_set_members() {
    let mut connection = manifest_connection();
    insert_manifest_chunks(&mut connection, "scope-a", 2_048);
    insert_manifest_chunks(&mut connection, "scope-b", 2_048);
    manifest_module_prefixes_for_members(&mut connection, &[member("scope-a"), member("scope-b")])
        .expect("the shared manifest chunk capacity should be accepted");

    insert_manifest_chunks(&mut connection, "scope-c", 1);
    let error = manifest_module_prefixes_for_members(
        &mut connection,
        &[member("scope-a"), member("scope-b"), member("scope-c")],
    )
    .expect_err("the shared manifest chunk capacity should reject cap plus one");

    assert!(matches!(error, StorageError::CapacityExceeded(_)));
}

#[test]
fn manifest_byte_and_derived_item_budgets_are_shared_and_reject_cap_plus_one() {
    for (capacity, kind) in [
        (MAX_REPOSITORY_SET_MANIFEST_BYTES, "manifest byte"),
        (MAX_REPOSITORY_SET_MANIFEST_ITEMS, "manifest-derived item"),
    ] {
        let mut remaining = capacity;
        consume_budget(&mut remaining, capacity - 1, kind, capacity)
            .expect("the first member should consume its shared portion");
        consume_budget(&mut remaining, 1, kind, capacity)
            .expect("the exact shared capacity should be accepted");
        let error = consume_budget(&mut remaining, 1, kind, capacity)
            .expect_err("cap plus one should reject");
        assert!(matches!(error, StorageError::CapacityExceeded(_)));
    }
}

fn manifest_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_chunks (
                 source_scope TEXT NOT NULL,
                 chunk_id TEXT NOT NULL,
                 path TEXT NOT NULL,
                 content TEXT NOT NULL
             );",
        )
        .expect("chunk schema should create");
    connection
}

fn insert_manifest_chunks(connection: &mut Connection, scope: &str, count: usize) {
    let transaction = connection
        .transaction()
        .expect("fixture transaction should open");
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO code_repository_chunks (source_scope, chunk_id, path, content)
                 VALUES (?1, ?2, ?3, '{}')",
            )
            .expect("chunk insert should prepare");
        for index in 0..count {
            insert
                .execute(rusqlite::params![
                    scope,
                    format!("chunk-{index}"),
                    format!("packages/package-{index}/package.json"),
                ])
                .expect("manifest chunk should insert");
        }
    }
    transaction
        .commit()
        .expect("fixture transaction should commit");
    assert!(count <= MAX_REPOSITORY_SET_MANIFEST_CHUNKS);
}

fn member(source_scope: &str) -> CodeRepositorySetMemberStatus {
    CodeRepositorySetMemberStatus {
        member: CodeRepositorySetMember {
            set_id: "set".to_owned(),
            repository_id: format!("repo-{source_scope}"),
            repository_alias: source_scope.to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: format!("commit-{source_scope}"),
            source_scope: source_scope.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            priority: 0,
        },
        tree_hash: format!("tree-{source_scope}"),
        indexed_path_filters: Vec::new(),
        indexed_language_filters: Vec::new(),
        freshness_state: "fresh".to_owned(),
        stale: false,
        indexed_file_count: 1,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 1,
        degraded_reason: None,
    }
}
