//! Bounded indexed-document streaming for lifecycle projections.

use rusqlite::{Connection, params};

use crate::storage::StorageError;

/// Lifecycle extractors intentionally recognize only manifests, CI/IaC files,
/// and design documents. Keep this predicate a superset of the Rust-side
/// collectors so SQL can discard ordinary source chunks without changing
/// extraction semantics.
const CANDIDATE_PREDICATE: &str = "
    (
        path IN (
            'Cargo.toml', 'package.json', 'pyproject.toml', 'go.mod',
            'CMakeLists.txt', 'Makefile', 'makefile', 'GNUmakefile',
            'build.gradle', 'build.gradle.kts', '.gitlab-ci.yml', '.gitlab-ci.yaml',
            'Dockerfile', 'Containerfile'
        )
        OR path LIKE '%/Cargo.toml'
        OR path LIKE '%/package.json'
        OR path LIKE '%/pyproject.toml'
        OR path LIKE '%/go.mod'
        OR path LIKE '%/CMakeLists.txt'
        OR path LIKE '%/Makefile'
        OR path LIKE '%/makefile'
        OR path LIKE '%/GNUmakefile'
        OR path LIKE '%/build.gradle'
        OR path LIKE '%/build.gradle.kts'
        OR path LIKE '%/.gitlab-ci.yml'
        OR path LIKE '%/.gitlab-ci.yaml'
        OR path LIKE 'Dockerfile.%'
        OR path LIKE '%/Dockerfile'
        OR path LIKE '%/Dockerfile.%'
        OR path LIKE 'Containerfile.%'
        OR path LIKE '%/Containerfile'
        OR path LIKE '%/Containerfile.%'
        OR path LIKE '.github/workflows/%'
        OR lower(path) LIKE '%.tf'
        OR lower(path) LIKE '%.service'
        OR lower(path) LIKE '%.plist'
        OR language_id = 'yaml'
        OR lower(path) LIKE '%.yml'
        OR lower(path) LIKE '%.yaml'
        OR lower(path) LIKE '%.md'
        OR lower(path) LIKE '%.mdx'
    )
";

// These are safety ceilings, not workload targets. They bound a corrupted or
// adversarial scope while remaining well above the supported benchmark repos.
const MAX_CANDIDATE_DOCUMENTS: usize = 32_768;
const MAX_CANDIDATE_CHUNKS: usize = 262_144;
const MAX_CANDIDATE_BYTES: usize = 256 * 1_024 * 1_024;

pub(super) struct IndexedDocument {
    pub(super) repository_id: String,
    pub(super) source_scope: String,
    pub(super) path: String,
    pub(super) language_id: String,
    pub(super) lines: Vec<IndexedLine>,
}

pub(super) struct IndexedLine {
    pub(super) number: u32,
    pub(super) text: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct CandidateLoadStats {
    pub(super) document_count: usize,
    pub(super) chunk_count: usize,
    pub(super) materialized_bytes: usize,
}

/// Streams one complete candidate document at a time. Ordinary source chunks
/// never cross the SQLite/Rust boundary, and only one document's lines are
/// resident before the visitor can release them.
pub(super) fn visit_candidates(
    connection: &Connection,
    source_scope: &str,
    visit: impl FnMut(IndexedDocument) -> Result<(), StorageError>,
) -> Result<CandidateLoadStats, StorageError> {
    visit_candidates_with_budgets(
        connection,
        source_scope,
        CandidateBudgets {
            documents: MAX_CANDIDATE_DOCUMENTS,
            chunks: MAX_CANDIDATE_CHUNKS,
            bytes: MAX_CANDIDATE_BYTES,
        },
        visit,
    )
}

fn visit_candidates_with_budgets(
    connection: &Connection,
    source_scope: &str,
    budgets: CandidateBudgets,
    mut visit: impl FnMut(IndexedDocument) -> Result<(), StorageError>,
) -> Result<CandidateLoadStats, StorageError> {
    preflight_candidate_budget(connection, source_scope, budgets)?;
    let query = format!(
        "
        SELECT repository_id, source_scope, path, language_id, content, line_start
        FROM code_repository_chunks
        WHERE source_scope = ?1
          AND {CANDIDATE_PREDICATE}
        ORDER BY path ASC, line_start ASC, chunk_id ASC
        "
    );
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(RawCandidateChunk {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            path: row.get(2)?,
            language_id: row.get(3)?,
            content: row.get(4)?,
            line_start: row.get(5)?,
        })
    })?;
    let mut current = None::<IndexedDocument>;
    let mut stats = CandidateLoadStats::default();
    for row in rows {
        let chunk = row?;
        stats.chunk_count = stats.chunk_count.saturating_add(1);
        stats.materialized_bytes = stats.materialized_bytes.saturating_add(chunk.content.len());
        if current
            .as_ref()
            .is_some_and(|document| document.path != chunk.path)
        {
            stats.document_count = stats.document_count.saturating_add(1);
            validate_document_count(stats.document_count, budgets.documents)?;
            visit(current.take().expect("candidate document must exist"))?;
        }
        let document = current.get_or_insert_with(|| IndexedDocument {
            repository_id: chunk.repository_id.clone(),
            source_scope: chunk.source_scope.clone(),
            path: chunk.path.clone(),
            language_id: chunk.language_id.clone(),
            lines: Vec::new(),
        });
        validate_chunk_identity(document, &chunk)?;
        for (offset, text) in chunk.content.lines().enumerate() {
            document.lines.push(IndexedLine {
                number: chunk.line_start.saturating_add(offset as u32),
                text: text.to_owned(),
            });
        }
    }
    if let Some(document) = current {
        stats.document_count = stats.document_count.saturating_add(1);
        validate_document_count(stats.document_count, budgets.documents)?;
        visit(document)?;
    }

    Ok(stats)
}

fn preflight_candidate_budget(
    connection: &Connection,
    source_scope: &str,
    budgets: CandidateBudgets,
) -> Result<(), StorageError> {
    let query = format!(
        "
        SELECT COUNT(*), coalesce(SUM(length(CAST(content AS BLOB))), 0)
        FROM code_repository_chunks
        WHERE source_scope = ?1
          AND {CANDIDATE_PREDICATE}
        "
    );
    let (chunk_count, byte_count) = connection.query_row(&query, params![source_scope], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    let chunk_count = usize::try_from(chunk_count).unwrap_or(usize::MAX);
    let byte_count = usize::try_from(byte_count).unwrap_or(usize::MAX);
    let document_query = format!(
        "SELECT DISTINCT path FROM code_repository_chunks
         WHERE source_scope = ?1 AND {CANDIDATE_PREDICATE}
         ORDER BY path ASC LIMIT ?2"
    );
    let mut document_statement = connection.prepare(&document_query)?;
    let document_count = document_statement
        .query_map(
            params![source_scope, budgets.documents.saturating_add(1) as i64],
            |_| Ok(()),
        )?
        .count();
    validate_document_count(document_count, budgets.documents)?;
    if chunk_count > budgets.chunks {
        return Err(StorageError::CapacityExceeded(format!(
            "software lifecycle candidate chunk count {chunk_count} exceeds the bounded limit {}",
            budgets.chunks
        )));
    }
    if byte_count > budgets.bytes {
        return Err(StorageError::CapacityExceeded(format!(
            "software lifecycle candidate content bytes {byte_count} exceed the bounded limit {}",
            budgets.bytes
        )));
    }

    Ok(())
}

fn validate_chunk_identity(
    document: &IndexedDocument,
    chunk: &RawCandidateChunk,
) -> Result<(), StorageError> {
    if document.repository_id != chunk.repository_id
        || document.source_scope != chunk.source_scope
        || document.language_id != chunk.language_id
    {
        return Err(StorageError::InvalidInput(format!(
            "software lifecycle candidate '{}' has inconsistent indexed chunk identity",
            document.path
        )));
    }
    Ok(())
}

fn validate_document_count(observed: usize, limit: usize) -> Result<(), StorageError> {
    if observed > limit {
        return Err(StorageError::CapacityExceeded(format!(
            "software lifecycle candidate document count {observed} exceeds the bounded limit {limit}"
        )));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct CandidateBudgets {
    documents: usize,
    chunks: usize,
    bytes: usize,
}

struct RawCandidateChunk {
    repository_id: String,
    source_scope: String,
    path: String,
    language_id: String,
    content: String,
    line_start: u32,
}

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;
