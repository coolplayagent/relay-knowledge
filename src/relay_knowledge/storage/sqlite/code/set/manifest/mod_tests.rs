use rusqlite::Connection;

use crate::domain::{CodeRepositorySetMember, CodeRepositorySetMemberStatus};

use super::manifest_module_prefixes_for_members;

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
