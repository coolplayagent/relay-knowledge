use rusqlite::{Connection, params};

use crate::storage::StorageError;

use super::{CandidateBudgets, visit_candidates, visit_candidates_with_budgets};

#[test]
fn candidate_stream_preserves_every_supported_path_shape_and_ordered_lines() {
    let connection = candidate_database();
    let supported = [
        ("Cargo.toml", "toml"),
        ("web/package.json", "json"),
        ("python/pyproject.toml", "toml"),
        ("service/go.mod", "gomod"),
        ("native/CMakeLists.txt", "cmake"),
        ("Makefile", "make"),
        ("gradle/build.gradle.kts", "kotlin"),
        (".github/workflows/ci.yml", "yaml"),
        ("nested/.gitlab-ci.yaml", "yaml"),
        ("images/Dockerfile", "dockerfile"),
        ("images/Dockerfile.dev", "dockerfile"),
        ("infra/main.tf", "hcl"),
        ("service/relay.service", "ini"),
        ("mac/relay.plist", "xml"),
        ("deploy/arbitrary-config", "yaml"),
        ("docs/architecture.MD", "markdown"),
        ("docs/design.mdx", "mdx"),
    ];
    for (index, (path, language)) in supported.iter().enumerate() {
        insert_chunk(
            &connection,
            "scope",
            &format!("supported-{index}"),
            path,
            language,
            "first\nsecond",
            1,
        );
    }
    insert_chunk(
        &connection,
        "scope",
        "cargo-tail",
        "Cargo.toml",
        "toml",
        "third",
        3,
    );
    for (index, path) in [
        "src/lib.rs",
        "docs/guide.txt",
        "nested/Dockerfilex",
        "github/workflows/not-root.yml.txt",
        "Cargo.toml.bak",
    ]
    .iter()
    .enumerate()
    {
        insert_chunk(
            &connection,
            "scope",
            &format!("ignored-{index}"),
            path,
            "rust",
            "ignored",
            1,
        );
    }
    let mut documents = Vec::new();

    let stats = visit_candidates(&connection, "scope", |document| {
        documents.push(document);
        Ok(())
    })
    .expect("candidate documents should stream");

    assert_eq!(stats.document_count, supported.len());
    assert_eq!(stats.chunk_count, supported.len() + 1);
    assert!(
        documents
            .iter()
            .all(|document| document.path != "src/lib.rs")
    );
    let cargo = documents
        .iter()
        .find(|document| document.path == "Cargo.toml")
        .expect("Cargo manifest should load");
    assert_eq!(
        cargo
            .lines
            .iter()
            .map(|line| (line.number, line.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, "first"), (2, "second"), (3, "third")]
    );
}

#[test]
fn unrelated_source_chunks_do_not_cross_the_materialization_boundary() {
    let connection = candidate_database();
    for index in 0..2_000 {
        insert_chunk(
            &connection,
            "scope",
            &format!("source-{index:04}"),
            &format!("src/module_{index:04}.rs"),
            "rust",
            &"x".repeat(4_096),
            1,
        );
    }
    insert_chunk(
        &connection,
        "scope",
        "manifest",
        "Cargo.toml",
        "toml",
        "[package]\nname = \"bounded\"",
        1,
    );
    let mut paths = Vec::new();

    let stats = visit_candidates(&connection, "scope", |document| {
        paths.push(document.path);
        Ok(())
    })
    .expect("bounded candidate load should succeed");

    assert_eq!(paths, ["Cargo.toml"]);
    assert_eq!(stats.document_count, 1);
    assert_eq!(stats.chunk_count, 1);
    assert!(stats.materialized_bytes < 128);
}

#[test]
fn candidate_bytes_are_rejected_before_content_materialization() {
    let connection = candidate_database();
    connection
        .execute(
            "INSERT INTO code_repository_chunks (
                 repository_id, source_scope, chunk_id, path, language_id, content, line_start
             ) VALUES ('repo', 'scope', 'oversized', 'README.md', 'markdown', zeroblob(?1), 1)",
            params![1_025_i64],
        )
        .expect("oversized fixture should insert");

    let error = visit_candidates_with_budgets(
        &connection,
        "scope",
        CandidateBudgets {
            documents: 10,
            chunks: 10,
            bytes: 1_024,
        },
        |_| Ok(()),
    )
    .expect_err("candidate byte budget should reject oversized content");

    assert!(matches!(error, StorageError::CapacityExceeded(message)
        if message.contains("candidate content bytes")));
}

#[test]
fn candidate_chunk_and_document_limits_report_capacity_errors() {
    let connection = candidate_database();
    insert_chunk(&connection, "scope", "one", "Cargo.toml", "toml", "one", 1);
    insert_chunk(
        &connection,
        "scope",
        "two",
        "nested/Cargo.toml",
        "toml",
        "two",
        1,
    );

    let chunk_error = visit_candidates_with_budgets(
        &connection,
        "scope",
        CandidateBudgets {
            documents: 2,
            chunks: 1,
            bytes: 100,
        },
        |_| Ok(()),
    )
    .expect_err("chunk budget should reject cap plus one");
    assert!(
        matches!(chunk_error, StorageError::CapacityExceeded(message)
        if message.contains("candidate chunk count"))
    );

    let mut visits = 0_usize;
    let document_error = visit_candidates_with_budgets(
        &connection,
        "scope",
        CandidateBudgets {
            documents: 1,
            chunks: 2,
            bytes: 100,
        },
        |_| {
            visits += 1;
            Ok(())
        },
    )
    .expect_err("document budget should reject cap plus one");
    assert!(
        matches!(document_error, StorageError::CapacityExceeded(message)
        if message.contains("candidate document count"))
    );
    assert_eq!(visits, 0, "document overflow must fail in SQL preflight");
}

#[test]
fn inconsistent_candidate_chunk_identity_remains_invalid_data() {
    let connection = candidate_database();
    insert_chunk(&connection, "scope", "one", "Cargo.toml", "toml", "one", 1);
    connection
        .execute(
            "INSERT INTO code_repository_chunks (
                 repository_id, source_scope, chunk_id, path, language_id, content, line_start
             ) VALUES ('other-repo', 'scope', 'two', 'Cargo.toml', 'toml', 'two', 2)",
            [],
        )
        .expect("inconsistent chunk should insert");

    let error = visit_candidates(&connection, "scope", |_| Ok(()))
        .expect_err("inconsistent document identity should fail");
    assert!(matches!(error, StorageError::InvalidInput(message)
        if message.contains("inconsistent indexed chunk identity")));
}

fn candidate_database() -> Connection {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_chunks (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                chunk_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL
            );
            CREATE INDEX code_repository_chunks_lookup
                ON code_repository_chunks(source_scope, path);
            ",
        )
        .expect("candidate schema should initialize");
    connection
}

fn insert_chunk(
    connection: &Connection,
    source_scope: &str,
    chunk_id: &str,
    path: &str,
    language_id: &str,
    content: &str,
    line_start: u32,
) {
    connection
        .execute(
            "INSERT INTO code_repository_chunks (
                 repository_id, source_scope, chunk_id, path, language_id, content, line_start
             ) VALUES ('repo', ?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                source_scope,
                chunk_id,
                path,
                language_id,
                content,
                line_start
            ],
        )
        .expect("chunk should insert");
}
