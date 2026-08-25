//! Static first-page and continuation SQL for ordinary-reference resolution.

// A page scan remains one lazy PK statement. Rust stops `rows.next()` at the
// first over-budget candidate, so byte admission never materializes the tail.
// The fixed 153 bytes conservatively cover five integer payloads and serial
// types, eleven text serial types, and the SQLite record-header varint.
pub(super) const SCAN_FIRST: &str = "SELECT rowid,
         length(CAST(reference_id AS BLOB)), kind = 'call',
         CASE WHEN kind = 'call' THEN 0 ELSE
             length(CAST(repository_id AS BLOB))
             + length(CAST(source_scope AS BLOB))
             + length(CAST(reference_id AS BLOB))
             + length(CAST(file_id AS BLOB))
             + length(CAST(path AS BLOB))
             + 2 * length(CAST(name AS BLOB))
             + length(CAST(kind AS BLOB)) + 153
         END
     FROM code_repository_references
     WHERE source_scope = ?1
     ORDER BY reference_id LIMIT ?2";

pub(super) const SCAN_AFTER: &str = "SELECT rowid,
         length(CAST(reference_id AS BLOB)), kind = 'call',
         CASE WHEN kind = 'call' THEN 0 ELSE
             length(CAST(repository_id AS BLOB))
             + length(CAST(source_scope AS BLOB))
             + length(CAST(reference_id AS BLOB))
             + length(CAST(file_id AS BLOB))
             + length(CAST(path AS BLOB))
             + 2 * length(CAST(name AS BLOB))
             + length(CAST(kind AS BLOB)) + 153
         END
     FROM code_repository_references
     WHERE source_scope = ?1 AND reference_id > ?2
     ORDER BY reference_id LIMIT ?3";

pub(super) const FETCH_CANDIDATE: &str = "SELECT reference_id, path, name
     FROM code_repository_references WHERE source_scope = ?1 AND rowid = ?2";

pub(super) const NAME_OWNERS: &str = "SELECT length(CAST(symbol_snapshot_id AS BLOB))
     FROM code_repository_symbols INDEXED BY code_repository_symbols_name_path_lookup
     WHERE source_scope = ?1 AND name = ?2 LIMIT 2";

pub(super) const PATH_OWNERS: &str = "SELECT length(CAST(symbol_snapshot_id AS BLOB))
     FROM code_repository_symbols INDEXED BY code_repository_symbols_name_path_lookup
     WHERE source_scope = ?1 AND name = ?2 AND path = ?3 LIMIT 2";

pub(super) const UPDATE_FIRST: &str = "WITH limited AS MATERIALIZED (
         SELECT reference_id, path, name
         FROM code_repository_references
         WHERE source_scope = ?1 AND reference_id <= ?2 AND kind != 'call'
     ), page_names AS MATERIALIZED (
         SELECT DISTINCT name FROM limited
     ), name_summary AS MATERIALIZED (
         SELECT page_names.name,
                (SELECT symbol.symbol_snapshot_id
                 FROM code_repository_symbols symbol
                      INDEXED BY code_repository_symbols_name_path_lookup
                 WHERE symbol.source_scope = ?1 AND symbol.name = page_names.name
                 LIMIT 1) AS symbol_snapshot_id,
                EXISTS (
                    SELECT 1 FROM code_repository_symbols symbol
                         INDEXED BY code_repository_symbols_name_path_lookup
                    WHERE symbol.source_scope = ?1 AND symbol.name = page_names.name
                    LIMIT 1 OFFSET 1
                ) AS has_second
         FROM page_names
     ), page_pairs AS MATERIALIZED (
         SELECT DISTINCT name, path FROM limited
     ), path_summary AS MATERIALIZED (
         SELECT page_pairs.name, page_pairs.path,
                (SELECT symbol.symbol_snapshot_id
                 FROM code_repository_symbols symbol
                      INDEXED BY code_repository_symbols_name_path_lookup
                 WHERE symbol.source_scope = ?1 AND symbol.name = page_pairs.name
                   AND symbol.path = page_pairs.path LIMIT 1) AS symbol_snapshot_id,
                EXISTS (
                    SELECT 1 FROM code_repository_symbols symbol
                         INDEXED BY code_repository_symbols_name_path_lookup
                    WHERE symbol.source_scope = ?1 AND symbol.name = page_pairs.name
                      AND symbol.path = page_pairs.path LIMIT 1 OFFSET 1
                ) AS has_second
         FROM page_pairs
     ), decisions AS (
         SELECT limited.reference_id, limited.name,
                CASE
                    WHEN name_summary.symbol_snapshot_id IS NOT NULL
                         AND NOT name_summary.has_second THEN name_summary.symbol_snapshot_id
                    WHEN path_summary.symbol_snapshot_id IS NOT NULL
                         AND NOT path_summary.has_second THEN path_summary.symbol_snapshot_id
                    ELSE NULL
                END AS target_symbol_snapshot_id,
                CASE
                    WHEN (name_summary.symbol_snapshot_id IS NOT NULL
                          AND NOT name_summary.has_second)
                         OR (path_summary.symbol_snapshot_id IS NOT NULL
                             AND NOT path_summary.has_second)
                        THEN 'resolved'
                    WHEN name_summary.symbol_snapshot_id IS NOT NULL THEN 'ambiguous'
                    ELSE 'unresolved'
                END AS resolution_state,
                CASE
                    WHEN (name_summary.symbol_snapshot_id IS NOT NULL
                          AND NOT name_summary.has_second)
                         OR (path_summary.symbol_snapshot_id IS NOT NULL
                             AND NOT path_summary.has_second)
                        THEN 8000
                    WHEN name_summary.symbol_snapshot_id IS NOT NULL THEN 5000
                    ELSE 2500
                END AS confidence_basis_points,
                CASE
                    WHEN (name_summary.symbol_snapshot_id IS NOT NULL
                          AND NOT name_summary.has_second)
                         OR (path_summary.symbol_snapshot_id IS NOT NULL
                             AND NOT path_summary.has_second)
                        THEN 'inferred'
                    ELSE 'ambiguous'
                END AS confidence_tier
         FROM limited
         LEFT JOIN name_summary ON name_summary.name = limited.name
         LEFT JOIN path_summary
           ON path_summary.name = limited.name AND path_summary.path = limited.path
     )
     UPDATE code_repository_references AS reference
     SET target_symbol_snapshot_id = decisions.target_symbol_snapshot_id,
         target_hint = decisions.name,
         resolution_state = decisions.resolution_state,
         confidence_basis_points = decisions.confidence_basis_points,
         confidence_tier = decisions.confidence_tier
     FROM decisions
     WHERE reference.source_scope = ?1
       AND reference.reference_id = decisions.reference_id";

pub(super) const UPDATE_AFTER: &str = "WITH limited AS MATERIALIZED (
         SELECT reference_id, path, name
         FROM code_repository_references
         WHERE source_scope = ?1 AND reference_id > ?2 AND reference_id <= ?3
           AND kind != 'call'
     ), page_names AS MATERIALIZED (
         SELECT DISTINCT name FROM limited
     ), name_summary AS MATERIALIZED (
         SELECT page_names.name,
                (SELECT symbol.symbol_snapshot_id
                 FROM code_repository_symbols symbol
                      INDEXED BY code_repository_symbols_name_path_lookup
                 WHERE symbol.source_scope = ?1 AND symbol.name = page_names.name
                 LIMIT 1) AS symbol_snapshot_id,
                EXISTS (
                    SELECT 1 FROM code_repository_symbols symbol
                         INDEXED BY code_repository_symbols_name_path_lookup
                    WHERE symbol.source_scope = ?1 AND symbol.name = page_names.name
                    LIMIT 1 OFFSET 1
                ) AS has_second
         FROM page_names
     ), page_pairs AS MATERIALIZED (
         SELECT DISTINCT name, path FROM limited
     ), path_summary AS MATERIALIZED (
         SELECT page_pairs.name, page_pairs.path,
                (SELECT symbol.symbol_snapshot_id
                 FROM code_repository_symbols symbol
                      INDEXED BY code_repository_symbols_name_path_lookup
                 WHERE symbol.source_scope = ?1 AND symbol.name = page_pairs.name
                   AND symbol.path = page_pairs.path LIMIT 1) AS symbol_snapshot_id,
                EXISTS (
                    SELECT 1 FROM code_repository_symbols symbol
                         INDEXED BY code_repository_symbols_name_path_lookup
                    WHERE symbol.source_scope = ?1 AND symbol.name = page_pairs.name
                      AND symbol.path = page_pairs.path LIMIT 1 OFFSET 1
                ) AS has_second
         FROM page_pairs
     ), decisions AS (
         SELECT limited.reference_id, limited.name,
                CASE
                    WHEN name_summary.symbol_snapshot_id IS NOT NULL
                         AND NOT name_summary.has_second THEN name_summary.symbol_snapshot_id
                    WHEN path_summary.symbol_snapshot_id IS NOT NULL
                         AND NOT path_summary.has_second THEN path_summary.symbol_snapshot_id
                    ELSE NULL
                END AS target_symbol_snapshot_id,
                CASE
                    WHEN (name_summary.symbol_snapshot_id IS NOT NULL
                          AND NOT name_summary.has_second)
                         OR (path_summary.symbol_snapshot_id IS NOT NULL
                             AND NOT path_summary.has_second)
                        THEN 'resolved'
                    WHEN name_summary.symbol_snapshot_id IS NOT NULL THEN 'ambiguous'
                    ELSE 'unresolved'
                END AS resolution_state,
                CASE
                    WHEN (name_summary.symbol_snapshot_id IS NOT NULL
                          AND NOT name_summary.has_second)
                         OR (path_summary.symbol_snapshot_id IS NOT NULL
                             AND NOT path_summary.has_second)
                        THEN 8000
                    WHEN name_summary.symbol_snapshot_id IS NOT NULL THEN 5000
                    ELSE 2500
                END AS confidence_basis_points,
                CASE
                    WHEN (name_summary.symbol_snapshot_id IS NOT NULL
                          AND NOT name_summary.has_second)
                         OR (path_summary.symbol_snapshot_id IS NOT NULL
                             AND NOT path_summary.has_second)
                        THEN 'inferred'
                    ELSE 'ambiguous'
                END AS confidence_tier
         FROM limited
         LEFT JOIN name_summary ON name_summary.name = limited.name
         LEFT JOIN path_summary
           ON path_summary.name = limited.name AND path_summary.path = limited.path
     )
     UPDATE code_repository_references AS reference
     SET target_symbol_snapshot_id = decisions.target_symbol_snapshot_id,
         target_hint = decisions.name,
         resolution_state = decisions.resolution_state,
         confidence_basis_points = decisions.confidence_basis_points,
         confidence_tier = decisions.confidence_tier
     FROM decisions
     WHERE reference.source_scope = ?1
       AND reference.reference_id = decisions.reference_id";

pub(super) const FETCH_CURSOR: &str = "SELECT reference_id
     FROM code_repository_references WHERE source_scope = ?1 AND rowid = ?2";
