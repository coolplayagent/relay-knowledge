use rusqlite::{Connection, params_from_iter};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest,
    },
    storage::StorageError,
};

use super::{row_to_symbol, symbol_rows_to_hits};
use crate::storage::sqlite::code::{
    code_query::code_query_api_identities::ApiSymbolIdentity,
    code_query::code_query_hybrid_planning::{
        hybrid_query_prefers_chunk_first, hybrid_sequence_terms,
    },
    code_query::code_query_rows::SymbolRow,
    code_query::code_query_support::*,
    code_query::{dedupe_sort_truncate, prepare_code_search_statement, required_scope},
};

pub(super) fn search_hybrid_direct_symbol_hits(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    api_identities: &[ApiSymbolIdentity],
) -> Result<Option<Vec<CodeRetrievalHit>>, StorageError> {
    if request.code_query_kind != CodeQueryKind::Hybrid
        || !hybrid_query_prefers_chunk_first(request)
    {
        return Ok(None);
    }
    let rows = search_hybrid_direct_symbol_rows(connection, status, request)?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut hits = symbol_rows_to_hits(status, request, rows, api_identities);
    dedupe_sort_truncate(&mut hits, request.limit);
    if hybrid_direct_symbol_hits_can_answer_without_fts(request, &hits) {
        Ok(Some(hits))
    } else {
        Ok(None)
    }
}

fn search_hybrid_direct_symbol_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<SymbolRow>, StorageError> {
    let patterns = candidate_patterns(&request.query, 8);
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    let path_filter = path_filter_sql_for_column("path", status, request);
    let language_filter = language_filter_sql_for_column("language_id", status, request);
    let generated_filter = if request.exclude_generated {
        "AND coalesce((
                   SELECT file.is_generated
                   FROM code_repository_files file
                   WHERE file.source_scope = code_repository_symbols.source_scope
                     AND file.path = code_repository_symbols.path
                   LIMIT 1
               ), 0) = 0"
    } else {
        ""
    };
    let mut values = vec![rusqlite::types::Value::Text(
        required_scope(status)?.to_owned(),
    )];
    let candidate_filter = direct_symbol_candidate_filter(&patterns, &mut values);
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    let limit = request.limit.saturating_mul(8).clamp(24, 96);
    values.push(rusqlite::types::Value::Integer(limit as i64));
    let sql = format!(
        "
        SELECT symbol_snapshot_id, canonical_symbol_id, file_id, path, language_id, signature, doc_comment,
               byte_start, byte_end, line_start, line_end, name, qualified_name, kind,
               coalesce((
                   SELECT file.is_generated
                   FROM code_repository_files file
                   WHERE file.source_scope = code_repository_symbols.source_scope
                     AND file.path = code_repository_symbols.path
                   LIMIT 1
               ), 0) AS is_generated,
               NULL AS previous_symbol_context_start
        FROM code_repository_symbols
        WHERE source_scope = ?
          AND ({candidate_filter})
          {generated_filter}
          {path_filter}
          {language_filter}
        ORDER BY is_generated ASC,
                 path ASC,
                 line_start ASC,
                 CASE kind
                     WHEN 'function' THEN 0
                     WHEN 'method' THEN 1
                     WHEN 'class' THEN 2
                     WHEN 'interface' THEN 3
                     WHEN 'struct' THEN 4
                     WHEN 'enum' THEN 5
                     ELSE 6
                 END ASC,
                 name ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), row_to_symbol)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn direct_symbol_candidate_filter(
    patterns: &[String],
    values: &mut Vec<rusqlite::types::Value>,
) -> String {
    patterns
        .iter()
        .map(|pattern| {
            [
                "lower(name)",
                "lower(qualified_name)",
                "lower(signature)",
                "lower(path)",
            ]
            .iter()
            .map(|field| {
                values.push(rusqlite::types::Value::Text(pattern.clone()));
                format!("{field} LIKE ? ESCAPE '\\'")
            })
            .collect::<Vec<_>>()
            .join(" OR ")
        })
        .map(|group| format!("({group})"))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn hybrid_direct_symbol_hits_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    let terms = hybrid_sequence_terms(&request.query);
    if terms.len() < 5 {
        return false;
    }
    let required_coverage = terms.len().saturating_mul(2).div_ceil(3).max(4);
    let mut covered_terms = Vec::new();
    let mut supporting_hits = 0usize;
    for hit in hits.iter().take(request.limit.max(1)) {
        if !hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
            || !hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::Definition)
        {
            continue;
        }
        let surface = format!(
            "{} {} {}",
            hit.excerpt.to_ascii_lowercase(),
            hit.canonical_symbol_id
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase(),
            hit.path.to_ascii_lowercase()
        );
        let mut matched = 0usize;
        for term in &terms {
            if surface.contains(term.as_str()) {
                matched += 1;
                if !covered_terms.contains(term) {
                    covered_terms.push(term.clone());
                }
            }
        }
        if matched >= 2 && hit.score >= 4.0 {
            supporting_hits += 1;
        }
    }

    supporting_hits >= 2 && covered_terms.len() >= required_coverage
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration, CodeRepositorySelector,
            FreshnessPolicy, RepositoryCodeFileRecord, RepositoryCodeRange,
            RepositoryCodeSymbolRecord,
        },
        storage::{SqliteGraphStore, code::CodeRepositoryStore},
    };

    const TEST_SOURCE_SCOPE: &str = "code:test:hybrid-direct-generated:commit:tree";

    #[tokio::test]
    async fn direct_rows_prefer_handwritten_candidates_before_limit() {
        let store = store_with_snapshot(snapshot_with_generated_symbol_noise()).await;
        let status = store
            .code_repository_status("repo".to_owned())
            .await
            .expect("status should load")
            .expect("repository should exist");
        let selector =
            CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
                .expect("selector should validate");
        let request = CodeRetrievalRequest::new(
            "Recover alpha beta gamma delta epsilon",
            selector,
            CodeQueryKind::Hybrid,
            5,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate");
        let rows = store
            .run_read(move |connection| {
                search_hybrid_direct_symbol_rows(connection, &status, &request)
            })
            .await
            .expect("hybrid direct rows should load");

        assert_eq!(
            rows.first().map(|row| row.path.as_str()),
            Some("src/zz_handwritten.rs")
        );
    }

    fn snapshot_with_generated_symbol_noise() -> CodeIndexSnapshot {
        let mut files = Vec::new();
        let mut symbols = Vec::new();
        for index in 0..120 {
            let file_id = format!("generated-file-{index:03}");
            let path = format!("generated/recover_{index:03}.rs");
            let mut generated_file = file(&file_id, &path);
            generated_file.is_generated = true;
            files.push(generated_file);
            symbols.push(symbol(
                &format!("generated-recover-{index:03}"),
                &file_id,
                &path,
            ));
        }
        files.push(file("handwritten-file", "src/zz_handwritten.rs"));
        symbols.push(symbol(
            "handwritten-recover",
            "handwritten-file",
            "src/zz_handwritten.rs",
        ));

        CodeIndexSnapshot {
            repository_id: "repo".to_owned(),
            source_scope: TEST_SOURCE_SCOPE.to_owned(),
            base_resolved_commit_sha: None,
            resolved_commit_sha: "commit".to_owned(),
            tree_hash: "tree".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            full_replace: true,
            changed_path_count: files.len(),
            skipped_unchanged_count: 0,
            deleted_paths: Vec::new(),
            tombstones: Vec::new(),
            files,
            symbols,
            references: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            workspaces: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn file(file_id: &str, path: &str) -> RepositoryCodeFileRecord {
        RepositoryCodeFileRecord {
            repository_id: "repo".to_owned(),
            source_scope: TEST_SOURCE_SCOPE.to_owned(),
            file_id: file_id.to_owned(),
            path: path.to_owned(),
            language_id: "rust".to_owned(),
            blob_hash: format!("hash-{file_id}"),
            byte_len: 0,
            line_count: 1,
            parse_status: CodeParseStatus::Parsed,
            is_generated: false,
            degraded_reason: None,
        }
    }

    fn symbol(symbol_snapshot_id: &str, file_id: &str, path: &str) -> RepositoryCodeSymbolRecord {
        RepositoryCodeSymbolRecord {
            repository_id: "repo".to_owned(),
            source_scope: TEST_SOURCE_SCOPE.to_owned(),
            symbol_snapshot_id: symbol_snapshot_id.to_owned(),
            canonical_symbol_id: format!("repo://repo/{}::Recover", path.replace('/', "::")),
            file_id: file_id.to_owned(),
            path: path.to_owned(),
            language_id: "rust".to_owned(),
            name: "Recover".to_owned(),
            qualified_name: "RecoverAlphaBetaGammaDeltaEpsilon".to_owned(),
            kind: "function".to_owned(),
            signature: "fn Recover_alpha_beta_gamma_delta_epsilon()".to_owned(),
            doc_comment: None,
            byte_range: RepositoryCodeRange { start: 1, end: 1 },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
        }
    }

    async fn store_with_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
        let store = SqliteGraphStore::open_in_memory().expect("store should open");
        let registration =
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate");
        store
            .upsert_code_repository(registration)
            .await
            .expect("repository should persist");
        store
            .apply_code_index_snapshot(snapshot)
            .await
            .expect("snapshot should apply");

        store
    }
}
