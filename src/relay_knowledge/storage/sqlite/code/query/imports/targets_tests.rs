use super::*;
use crate::domain::{CodeRepositorySelector, CodeRepositoryStatus, FreshnessPolicy};
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

#[test]
fn target_symbol_expansion_is_imports_only_and_rejects_qualified_modules_before_sql() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_symbols (
                source_scope TEXT NOT NULL,
                symbol_snapshot_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                name TEXT NOT NULL,
                line_start INTEGER NOT NULL
            );
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                is_generated INTEGER NOT NULL
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            ",
        )
        .expect("target-symbol fixture schema should initialize");
    establish_exact_search_ownership(&connection);
    let symbol_reads = Arc::new(AtomicUsize::new(0));
    let observed_symbol_reads = Arc::clone(&symbol_reads);
    connection.authorizer(Some(move |context: AuthContext<'_>| {
        if matches!(
            context.action,
            AuthAction::Read {
                table_name: "code_repository_symbols",
                ..
            }
        ) {
            observed_symbol_reads.fetch_add(1, Ordering::Relaxed);
        }
        Authorization::Allow
    }));

    for (kind, query) in [
        (CodeQueryKind::Hybrid, "ObjectUtils"),
        (
            CodeQueryKind::Imports,
            "org.springframework.util.ObjectUtils",
        ),
        (CodeQueryKind::Imports, "Vendor::Client"),
    ] {
        let request = target_request(query, kind);
        let rows = search_imports_by_target_symbols(&connection, &status(), &request)
            .expect("ineligible target-symbol expansion should not prepare SQL");
        assert!(rows.is_empty());
    }
    assert_eq!(symbol_reads.load(Ordering::Relaxed), 0);

    let request = target_request("ObjectUtils", CodeQueryKind::Imports);
    let rows = search_imports_by_target_symbols(&connection, &status(), &request)
        .expect("eligible target-symbol expansion should prepare its symbol query");
    assert!(rows.is_empty());
    assert!(symbol_reads.load(Ordering::Relaxed) > 0);
}

#[test]
fn target_symbol_query_classifier_accepts_only_unqualified_identities() {
    assert_eq!(
        import_target_symbol_query("ObjectUtils"),
        Some("ObjectUtils")
    );
    assert_eq!(
        import_target_symbol_query("metav1 example.org/api/meta/v1 ObjectMeta"),
        Some("ObjectMeta")
    );
    assert_eq!(import_target_symbol_query("ObjectUtils importers"), None);
    assert_eq!(
        import_target_symbol_query("org.springframework.util.ObjectUtils"),
        None
    );
    assert_eq!(
        import_target_symbol_query("dotty.tools.dotc.core.Contexts.*"),
        None
    );
    assert_eq!(import_target_symbol_query("Vendor::Client"), None);
    assert_eq!(import_target_symbol_query("src/provider.ts"), None);
}

#[test]
fn target_symbol_usage_prefers_the_matched_identity_over_importer_alias_noise() {
    let mut row = import_row("versionedwidgets example.org/widgets");
    row.matched_symbol_name = Some("SharedWidgetFactory".to_owned());

    let terms = import_usage_context_terms_for_row("SharedWidgetFactory", &row);

    assert!(terms.contains(&"sharedwidgetfactory".to_owned()));
    assert!(!terms.contains(&"versionedwidgets".to_owned()));
}

#[test]
fn target_symbol_expansion_requires_the_explicit_identity_name() {
    assert!(symbol_matches_import_target_query(
        "RuntimeDescriptor",
        "RuntimeDescriptor"
    ));
    assert!(!symbol_matches_import_target_query(
        "RuntimeDescriptor",
        "RuntimeDescriptorList"
    ));
}

#[test]
fn usage_context_requires_row_local_binding_evidence() {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let plain = CodeRetrievalRequest::new(
        "./protocol",
        selector.clone(),
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let target_symbol = CodeRetrievalRequest::new(
        "StreamEnvelope",
        selector,
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    assert!(import_usage_context_terms_for_row(&plain.query, &import_row("./protocol")).is_empty());
    let mut target_row = import_row("./protocol");
    target_row.matched_symbol_name = Some("StreamEnvelope".to_owned());
    assert!(!import_usage_context_terms_for_row(&target_symbol.query, &target_row).is_empty());

    let mut row = import_row("./protocol");
    row.target_symbol_names = Some("StreamEnvelope".to_owned());
    assert!(!import_usage_context_terms_for_row(&plain.query, &row).is_empty());
}

#[test]
fn usage_context_includes_namespace_and_wildcard_bindings() {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let namespace = CodeRetrievalRequest::new(
        "Illuminate\\Container\\Container",
        selector.clone(),
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let wildcard = CodeRetrievalRequest::new(
        "dotty.tools.dotc.core.Contexts.*",
        selector,
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    assert!(
        !import_usage_context_terms_for_row(
            &namespace.query,
            &import_row("use Illuminate\\Container\\Container;"),
        )
        .is_empty()
    );
    assert!(
        !import_usage_context_terms_for_row(
            &wildcard.query,
            &import_row("import dotty.tools.dotc.core.Contexts.*"),
        )
        .is_empty()
    );
}

#[test]
fn usage_context_uses_local_bindings_instead_of_namespace_fragments() {
    assert_eq!(
        import_usage_context_terms_for_row(
            "org.springframework.util.ObjectUtils",
            &import_row("import org.springframework.util.ObjectUtils;"),
        ),
        ["objectutils"]
    );
    assert_eq!(
        import_usage_context_terms_for_row(
            "dotty.tools.dotc.core.Contexts.*",
            &import_row("import dotty.tools.dotc.core.Contexts.*"),
        ),
        ["context", "contexts"]
    );
    assert_eq!(
        import_usage_context_terms_for_row(
            "HTTPClient",
            &import_row("use Vendor::Client as HTTPClient;"),
        ),
        ["httpclient"]
    );
    assert!(
        import_usage_context_terms_for_row(
            "org.springframework.util.ObjectUtils",
            &import_row("import org.example.framework.LoggingSupport;"),
        )
        .is_empty()
    );

    let mut matched_symbol_noise = import_row("import org.example.framework.LoggingSupport;");
    matched_symbol_noise.matched_symbol_name = Some("LoggingSupport".to_owned());
    assert!(
        import_usage_context_terms_for_row(
            "org.springframework.util.ObjectUtils",
            &matched_symbol_noise,
        )
        .is_empty(),
        "an unrelated matched symbol must not authorize its local terminal"
    );

    let mut target_symbol_noise = import_row("import org.example.framework.LoggingSupport;");
    target_symbol_noise.target_symbol_names = Some("LoggingSupport".to_owned());
    assert!(
        import_usage_context_terms_for_row(
            "org.springframework.util.ObjectUtils",
            &target_symbol_noise,
        )
        .is_empty(),
        "an unrelated resolved target must not authorize its local terminal"
    );
}

#[test]
fn irrelevant_candidates_do_not_consume_the_eligible_path_budget() {
    let mut rows = (0..=MAX_IMPORT_USAGE_CONTEXT_PATHS)
        .map(|index| {
            let mut row = import_row("import org.example.framework.LoggingSupport;");
            row.path = format!("src/noise-{index:03}.java");
            row.matched_symbol_name = Some("LoggingSupport".to_owned());
            row.target_symbol_names = Some("LoggingSupport".to_owned());
            row
        })
        .collect::<Vec<_>>();
    let mut eligible = import_row("import org.springframework.util.ObjectUtils;");
    eligible.path = "src/EligibleImporter.java".to_owned();
    eligible.matched_symbol_name = Some("ObjectUtils".to_owned());
    rows.push(eligible);
    let usage_terms_by_row = rows
        .iter()
        .map(|row| import_usage_context_terms_for_row("org.springframework.util.ObjectUtils", row))
        .collect::<Vec<_>>();

    let paths = eligible_import_usage_context_paths(&rows, &usage_terms_by_row)
        .expect("irrelevant rows must not saturate the eligible path budget");

    assert_eq!(paths, ["src/EligibleImporter.java"]);
}

#[test]
fn eligible_candidates_still_obey_the_path_budget() {
    let rows = (0..=MAX_IMPORT_USAGE_CONTEXT_PATHS)
        .map(|index| {
            let mut row = import_row("import org.springframework.util.ObjectUtils;");
            row.path = format!("src/importer-{index:03}.java");
            row
        })
        .collect::<Vec<_>>();
    let usage_terms_by_row = rows
        .iter()
        .map(|row| import_usage_context_terms_for_row("org.springframework.util.ObjectUtils", row))
        .collect::<Vec<_>>();

    assert!(eligible_import_usage_context_paths(&rows, &usage_terms_by_row).is_none());
}

#[test]
fn import_usage_context_page_samples_each_path_before_reusing_its_budget() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_chunks (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                chunk_id TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            WITH RECURSIVE sequence(value) AS (
                VALUES (0)
                UNION ALL
                SELECT value + 1 FROM sequence
                WHERE value < 2111
            )
            INSERT INTO code_repository_chunks (
                source_scope, path, content, line_start, chunk_id
            )
            SELECT 'scope',
                   printf('src/file-%02d.rs', value / 64),
                   'Client',
                   value % 64 + 1,
                   printf('chunk-%04d', value)
            FROM sequence;
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            SELECT source_scope, 'chunk', chunk_id, path, 'rust', content
            FROM code_repository_chunks;
            ",
        )
        .expect("context chunks should persist");
    establish_exact_search_ownership(&connection);
    let paths = (0..33)
        .map(|index| format!("src/file-{index:02}.rs"))
        .collect::<Vec<_>>();
    let path_refs = paths.iter().map(String::as_str).collect::<Vec<_>>();

    let saturated = import_context_chunks(
        &connection,
        &status(),
        &path_refs,
        &symbol_fts_match_query("Client"),
        MAX_IMPORT_USAGE_CONTEXT_CHUNKS_TOTAL,
        MAX_IMPORT_USAGE_CONTEXT_BYTES_TOTAL,
    )
    .expect("bounded context query should succeed");
    let bounded = import_context_chunks(
        &connection,
        &status(),
        &[path_refs[0]],
        &symbol_fts_match_query("Client"),
        MAX_IMPORT_USAGE_CONTEXT_CHUNKS_TOTAL,
        MAX_IMPORT_USAGE_CONTEXT_BYTES_TOTAL,
    )
    .expect("one-path context query should succeed");

    assert!(!saturated.saturated);
    assert_eq!(saturated.chunks.len(), 33 * 2);
    let sampled_paths =
        saturated
            .chunks
            .iter()
            .fold(BTreeMap::<&str, usize>::new(), |mut counts, (path, _)| {
                *counts.entry(path).or_default() += 1;
                counts
            });
    assert!(sampled_paths.values().all(|count| *count == 2));
    assert!(!bounded.saturated);
    assert_eq!(
        bounded.chunks.len(),
        MAX_IMPORT_USAGE_CONTEXT_CHUNKS_PER_PATH
    );
}

#[test]
fn importer_context_sql_work_budget_disables_optional_scoring() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_chunks (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                chunk_id TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            INSERT INTO code_repository_chunks VALUES (
                'scope', 'src/provider.rs', 'Client::connect();', 1, 'chunk-1'
            );
            INSERT INTO code_repository_search VALUES (
                'scope', 'chunk', 'chunk-1', 'src/provider.rs', 'rust', 'Client::connect();'
            );
            ",
        )
        .expect("context fixture should persist");
    establish_exact_search_ownership(&connection);

    let probe = import_context_chunks_with_progress_budget(
        &connection,
        &status(),
        &["src/provider.rs"],
        &symbol_fts_match_query("Client"),
        ImportContextProbeBudget {
            max_chunks: MAX_IMPORT_USAGE_CONTEXT_CHUNKS_TOTAL,
            max_bytes: MAX_IMPORT_USAGE_CONTEXT_BYTES_TOTAL,
            progress_interval: 1,
            max_progress_callbacks: 0,
        },
    )
    .expect("a work-budget interruption should be an optional saturation");

    assert!(probe.saturated);
    assert!(probe.chunks.is_empty());
    let value = connection
        .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
        .expect("the interrupted probe must remove its progress handler");
    assert_eq!(value, 1);
}

#[test]
fn importer_context_byte_budget_disables_the_complete_optional_window() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_chunks (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                chunk_id TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            INSERT INTO code_repository_chunks VALUES (
                'scope', 'src/provider.rs', 'Client::connect();', 1, 'chunk-1'
            );
            INSERT INTO code_repository_search VALUES (
                'scope', 'chunk', 'chunk-1', 'src/provider.rs', 'rust', 'Client::connect();'
            );
            ",
        )
        .expect("context fixture should persist");
    establish_exact_search_ownership(&connection);

    let probe = import_context_chunks_with_progress_budget(
        &connection,
        &status(),
        &["src/provider.rs"],
        &symbol_fts_match_query("Client"),
        ImportContextProbeBudget {
            max_chunks: MAX_IMPORT_USAGE_CONTEXT_CHUNKS_TOTAL,
            max_bytes: "Client::connect();".len() - 1,
            progress_interval: IMPORT_CONTEXT_SQL_PROGRESS_INTERVAL,
            max_progress_callbacks: MAX_IMPORT_CONTEXT_SQL_PROGRESS_CALLBACKS,
        },
    )
    .expect("a byte-budget overflow should be an optional saturation");

    assert!(probe.saturated);
    assert!(probe.chunks.is_empty());
    assert_eq!(probe.byte_len, 0);
}

#[test]
fn importer_context_budget_is_fair_across_sql_bind_batches() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_chunks (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                chunk_id TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            WITH RECURSIVE sequence(value) AS (
                VALUES (0)
                UNION ALL
                SELECT value + 1 FROM sequence
                WHERE value < 2051
            )
            INSERT INTO code_repository_chunks (
                source_scope, path, content, line_start, chunk_id
            )
            SELECT 'scope',
                   CASE
                     WHEN value < 1984 THEN printf('src/file-%03d.rs', value / 4)
                     ELSE printf('src/file-%03d.rs', 496 + (value - 1984) / 17)
                   END,
                   'Client Client',
                   CASE
                     WHEN value < 1984 THEN value % 4 + 1
                     ELSE (value - 1984) % 17 + 1
                   END,
                   printf('chunk-%04d', value)
            FROM sequence;
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            SELECT source_scope, 'chunk', chunk_id, path, 'rust', content
            FROM code_repository_chunks;
            ",
        )
        .expect("cross-batch context chunks should persist");
    establish_exact_search_ownership(&connection);
    let mut rows = (0..500)
        .map(|index| {
            let mut row = import_row("use Vendor::Client;");
            row.path = format!("src/file-{index:03}.rs");
            row
        })
        .collect::<Vec<_>>();
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "Vendor::Client",
        selector,
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    attach_import_query_usage_context(&connection, &status(), &request, &mut rows)
        .expect("bounded importer context should attach");

    assert!(
        rows.iter().all(|row| row.same_file_query_usage_count > 0),
        "each eligible importer path should receive a bounded usage sample"
    );
}

#[test]
fn importer_context_reads_matching_chunks_instead_of_an_ordered_prefix() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_chunks (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                chunk_id TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            WITH RECURSIVE sequence(value) AS (
                VALUES (0)
                UNION ALL
                SELECT value + 1 FROM sequence
                WHERE value < 63
            )
            INSERT INTO code_repository_chunks (
                source_scope, path, content, line_start, chunk_id
            )
            SELECT 'scope', 'src/provider.rs', 'unrelated context', value + 1,
                   printf('chunk-%02d', value)
            FROM sequence;
            INSERT INTO code_repository_chunks VALUES (
                'scope', 'src/provider.rs',
                'use Vendor::Client; Client::connect(); Client::close();',
                65, 'usage-chunk'
            );
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            SELECT source_scope, 'chunk', chunk_id, path, 'rust', content
            FROM code_repository_chunks;
            ",
        )
        .expect("context chunks should persist");
    establish_exact_search_ownership(&connection);
    let mut row = import_row("use Vendor::Client;");
    row.path = "src/provider.rs".to_owned();
    row.language_id = "rust".to_owned();
    let mut rows = vec![row];
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "Vendor::Client",
        selector,
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    attach_import_query_usage_context(&connection, &status(), &request, &mut rows)
        .expect("query-aware importer context should attach");

    assert_eq!(rows[0].same_file_query_usage_count, 2);
}

#[test]
fn importer_context_counts_raw_chunk_source_instead_of_fts_metadata() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_chunks (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                chunk_id TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            INSERT INTO code_repository_chunks VALUES (
                'scope', 'src/provider.java', 'unrelated source body', 1, 'chunk-1'
            );
            INSERT INTO code_repository_search VALUES (
                'scope', 'chunk', 'chunk-1', 'src/provider.java', 'java',
                'ObjectUtils ObjectUtils metadata-only search surface'
            );
            ",
        )
        .expect("chunk and search metadata should persist");
    establish_exact_search_ownership(&connection);
    let mut row = import_row("import org.springframework.util.ObjectUtils;");
    row.path = "src/provider.java".to_owned();
    row.language_id = "java".to_owned();
    let mut rows = vec![row];
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "org.springframework.util.ObjectUtils",
        selector,
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    attach_import_query_usage_context(&connection, &status(), &request, &mut rows)
        .expect("bounded importer context should attach");

    assert_eq!(rows[0].same_file_query_usage_count, 0);
}

#[test]
fn importer_context_masks_comment_and_literal_mentions_across_languages() {
    let terms = vec!["Client".to_owned()];
    for (language_id, non_code, real_usage) in [
        (
            "java",
            "// Client documentation\nString name = \"Client\";",
            "Client.connect();",
        ),
        (
            "c",
            "/* Client documentation */\nconst char *name = \"Client\";",
            "Client();",
        ),
        (
            "javascript",
            "// Client documentation\nconst name = `Client`;",
            "Client.connect();",
        ),
        (
            "python",
            "# Client documentation\nname = \"Client\"",
            "Client.connect()",
        ),
    ] {
        let masked_non_code = code_outside_comments_and_literals(language_id, non_code);
        let masked_real_usage = code_outside_comments_and_literals(language_id, real_usage);
        assert_eq!(
            identifier_occurrences(&masked_non_code, &terms),
            0,
            "{language_id} comments and literals must not become importer usage"
        );
        assert_eq!(
            identifier_occurrences(&masked_real_usage, &terms),
            1,
            "{language_id} code identifiers must remain importer usage"
        );
    }
}

#[test]
fn importer_context_disables_optional_scoring_when_term_budget_saturates() {
    let rows = (0..=MAX_IMPORT_USAGE_CONTEXT_TERMS)
        .map(|index| import_row(&format!("use vendor::{{Binding{index:03}Specific}};")))
        .collect::<Vec<_>>();
    let usage_terms_by_row = rows
        .iter()
        .map(|row| import_usage_context_terms_for_row("./vendor", row))
        .collect::<Vec<_>>();

    assert!(import_usage_context_fts_query(&usage_terms_by_row).is_none());
}

#[test]
fn importer_context_read_model_outage_keeps_direct_import_rows_available() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_chunks (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                chunk_id TEXT NOT NULL
            );
            INSERT INTO code_repository_chunks VALUES (
                'scope', 'src/provider.rs',
                'Client::connect(); Client::close(); Client::flush();', 1, 'chunk-1'
            );
            ",
        )
        .expect("direct chunk facts should persist without the optional FTS table");
    let mut rows = vec![import_row("use Vendor::Client;")];
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "Vendor::Client",
        selector,
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    attach_import_query_usage_context(&connection, &status(), &request, &mut rows)
        .expect("optional importer context outage must not hide direct import evidence");

    assert_eq!(rows[0].same_file_query_usage_count, 0);
}

fn target_request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}

fn establish_exact_search_ownership(connection: &Connection) {
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_search_metadata (
                source_scope TEXT NOT NULL,
                document_kind TEXT NOT NULL,
                record_id TEXT NOT NULL,
                path TEXT NOT NULL,
                search_rowid INTEGER PRIMARY KEY,
                UNIQUE (source_scope, document_kind, record_id)
            );
            INSERT INTO code_repository_search_metadata (
                source_scope, document_kind, record_id, path, search_rowid
            )
            SELECT source_scope, document_kind, record_id, path, rowid
            FROM code_repository_search;
            ",
        )
        .expect("every healthy search fixture row should have one exact metadata owner");
}

fn status() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        root_path: "/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some("scope".to_owned()),
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "fresh".to_owned(),
        indexed_file_count: 33,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: MAX_IMPORT_USAGE_CONTEXT_CHUNKS_TOTAL,
        stale: false,
        degraded_reason: None,
    }
}

fn import_row(module: &str) -> ImportRow {
    ImportRow {
        file_id: "file".to_owned(),
        path: "src/provider.ts".to_owned(),
        language_id: "typescript".to_owned(),
        is_generated: false,
        source_line_count: 1,
        module: module.to_owned(),
        matched_symbol_name: None,
        target_symbol_names: None,
        same_file_query_usage_count: 0,
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        target_hint: None,
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 0,
        confidence_tier: "none".to_owned(),
    }
}
