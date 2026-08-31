use super::*;

#[test]
fn topic_row_mapping_preserves_source_range_and_version() {
    let connection = Connection::open_in_memory().expect("database should open");

    let topic = connection
        .query_row(
            "
            SELECT 'topic', 'repository', 'scope', 'Architecture',
                   'document_heading', 'docs/architecture.md', 4, 8, 9
            ",
            [],
            topic_from_row,
        )
        .expect("software topic should decode");

    assert_eq!(topic.topic_id, "topic");
    assert_eq!(topic.source_path, "docs/architecture.md");
    assert_eq!(topic.line_range, RepositoryCodeRange { start: 4, end: 8 });
    assert_eq!(topic.created_graph_version, GraphVersion::new(9));
}

#[test]
fn knowledge_map_topic_page_reads_only_the_owned_contract_path() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_symbols (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            ",
        )
        .expect("topic source schema should initialize");
    connection
        .execute(
            "
            INSERT INTO code_repository_symbols
                (repository_id, source_scope, path, name, kind, line_start, line_end)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                "repository",
                "scope",
                KNOWLEDGE_MAP_RELATIVE_PATH,
                "Storage",
                "knowledge_map_topic",
                2_u64,
                5_u64,
            ],
        )
        .expect("topic should seed");

    let topics = knowledge_map_topic_page(&connection, "scope", GraphVersion::new(4), 10, 0)
        .expect("knowledge-map topics should load");

    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].name, "Storage");
    assert_eq!(topics[0].created_graph_version, GraphVersion::new(4));
}

#[test]
fn knowledge_map_topic_page_uses_only_root_authorized_v2_shards() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_symbols (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            INSERT INTO code_repository_symbols VALUES
                ('repository', 'scope', '.knowledge/knowledge-map.yaml',
                 '.knowledge/topics/current.yaml', 'knowledge_map_topic_shard_ref', 5, 5),
                ('repository', 'scope', '.knowledge/knowledge-map.yaml',
                 'Build', 'knowledge_map_topic_shard_topic', 5, 5),
                ('repository', 'scope', '.knowledge/knowledge-map.yaml',
                 'matching-identity', 'knowledge_map_topic_shard_identity', 5, 5),
                ('repository', 'scope', '.knowledge/topics/current.yaml',
                 'Build', 'knowledge_map_topic_shard', 3, 3),
                ('repository', 'scope', '.knowledge/topics/current.yaml',
                 'matching-identity', 'knowledge_map_topic_shard_identity', 3, 3),
                ('repository', 'scope', '.knowledge/knowledge-map.yaml',
                 '.knowledge/topics/mismatch.yaml', 'knowledge_map_topic_shard_ref', 6, 6),
                ('repository', 'scope', '.knowledge/knowledge-map.yaml',
                 'Mismatch', 'knowledge_map_topic_shard_topic', 6, 6),
                ('repository', 'scope', '.knowledge/knowledge-map.yaml',
                 'root-identity', 'knowledge_map_topic_shard_identity', 6, 6),
                ('repository', 'scope', '.knowledge/topics/mismatch.yaml',
                 'Mismatch', 'knowledge_map_topic_shard', 3, 3),
                ('repository', 'scope', '.knowledge/topics/mismatch.yaml',
                 'different-shard-identity', 'knowledge_map_topic_shard_identity', 3, 3),
                ('repository', 'scope', '.knowledge/topics/orphan.yaml',
                 'Orphan', 'knowledge_map_topic_shard', 3, 3);
            ",
        )
        .expect("v2 topic source schema should initialize");

    let topics = knowledge_map_topic_page(&connection, "scope", GraphVersion::new(5), 10, 0)
        .expect("v2 knowledge-map topics should load");

    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].name, "Build");
    assert_eq!(topics[0].source_path, ".knowledge/topics/current.yaml");
}

#[test]
fn topic_queries_prioritize_specific_documents_before_root_overviews() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE software_topics (
                topic_id TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                name TEXT NOT NULL,
                topic_kind TEXT NOT NULL,
                source_path TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                created_graph_version INTEGER NOT NULL
            );
            CREATE TABLE software_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL
            );
            INSERT INTO software_files VALUES
                ('scope', 'README.md', 'markdown'),
                ('scope', 'docs/architecture.md', 'markdown'),
                ('scope', '.knowledge/knowledge-map.yaml', 'yaml');
            INSERT INTO software_topics VALUES
                ('root', 'repo', 'scope', 'Getting Started', 'document_heading',
                 'README.md', 1, 2, 1),
                ('architecture', 'repo', 'scope', 'Runtime Architecture', 'document_heading',
                 'docs/architecture.md', 1, 2, 1),
                ('knowledge', 'repo', 'scope', 'runtime', 'knowledge_map_topic',
                 '.knowledge/knowledge-map.yaml', 3, 3, 1);
            ",
        )
        .expect("topic rows should seed");
    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        crate::domain::SoftwareGlobalKind::Topics,
        crate::domain::FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate");

    let topics = topics_for_scope(&connection, "scope", &request, 10).expect("topics should load");

    assert_eq!(
        topics
            .iter()
            .map(|topic| topic.name.as_str())
            .collect::<Vec<_>>(),
        ["Runtime Architecture", "runtime", "Getting Started"]
    );
}
