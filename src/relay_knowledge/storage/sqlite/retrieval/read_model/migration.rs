use std::{thread, time::Duration};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{EvidenceExtractionMetadata, EvidenceModality},
    storage::StorageError,
};

use super::{
    super::{
        bm25_routing,
        context::{entities_for_evidence, parse_fact_status},
    },
    documents::{
        Bm25WriteTarget, EvidenceDocumentInput, LOCAL_TOKENIZER_VERSION, RetrievalWriteContext,
        insert_code_chunk_document, insert_code_symbol_document, replace_evidence_document,
    },
    identity::derived_document_identity_mismatch,
    rebuild_budget::{
        CodeRebuildKey, EvidenceRebuildKey, code_chunk_rebuild_page, code_symbol_rebuild_page,
        decode_code_rebuild_cursor, encode_code_rebuild_cursor, evidence_rebuild_page,
    },
    schema,
};

const REBUILD_DELETE_BATCH_SIZE: usize = 2_048;
const REBUILD_LEASE_WAIT_DELAYS_MS: [u64; 5] = [50, 150, 450, 1_350, 4_050];
const PHASE_PREPARE: &str = "prepare";
const PHASE_CLEAR_ROUTE_DOCUMENTS: &str = "clear_route_documents";
const PHASE_CLEAR_ROUTE_GROUPS: &str = "clear_route_groups";
const PHASE_CLEAR_ROUTE_TERMS: &str = "clear_route_terms";
const PHASE_CLEAR_ROUTE_TOTALS: &str = "clear_route_totals";
const PHASE_EVIDENCE: &str = "evidence";
const PHASE_CODE_SYMBOLS: &str = "code_symbols";
const PHASE_CODE_CHUNKS: &str = "code_chunks";
const PHASE_STALE_LABELS: &str = "stale_labels";
const PHASE_STALE_SEMANTIC: &str = "stale_semantic";
const PHASE_STALE_VECTOR: &str = "stale_vector";
const PHASE_ACTIVATE: &str = "activate";

#[derive(Clone, Copy)]
struct CompanionRebuildPlan {
    semantic: bool,
    vector: bool,
}

pub(super) fn rebuild_bm25_documents<F>(
    connection: &Connection,
    finalize_generation: F,
) -> Result<(), StorageError>
where
    F: FnOnce(&Connection) -> Result<(), StorageError>,
{
    let Some(lease) = acquire_rebuild_lease(connection)? else {
        return Ok(());
    };
    let proposed_plan = companion_rebuild_plan(connection)?;
    let (mut phase, mut cursor, rebuild_semantic, rebuild_vector) =
        bm25_routing::configure_rebuild(
            connection,
            &lease,
            proposed_plan.semantic,
            proposed_plan.vector,
        )?;
    let companion_plan = CompanionRebuildPlan {
        semantic: rebuild_semantic,
        vector: rebuild_vector,
    };

    if phase == PHASE_PREPARE {
        prepare_rebuild_generation(connection, &lease)?;
        phase = PHASE_CLEAR_ROUTE_DOCUMENTS.to_owned();
        cursor = None;
    }
    for (expected_phase, table, next_phase) in [
        (
            PHASE_CLEAR_ROUTE_DOCUMENTS,
            "graph_bm25_route_documents",
            PHASE_CLEAR_ROUTE_GROUPS,
        ),
        (
            PHASE_CLEAR_ROUTE_GROUPS,
            "graph_bm25_route_groups",
            PHASE_CLEAR_ROUTE_TERMS,
        ),
        (
            PHASE_CLEAR_ROUTE_TERMS,
            "graph_bm25_route_terms",
            PHASE_CLEAR_ROUTE_TOTALS,
        ),
        (
            PHASE_CLEAR_ROUTE_TOTALS,
            "graph_bm25_route_term_totals",
            PHASE_EVIDENCE,
        ),
    ] {
        if phase == expected_phase {
            delete_table_in_bounded_batches(connection, &lease, table, expected_phase, next_phase)?;
            phase = next_phase.to_owned();
            cursor = None;
        }
    }
    if phase == PHASE_EVIDENCE {
        rebuild_evidence_documents(connection, &lease, companion_plan, cursor.as_deref())?;
        phase = PHASE_CODE_SYMBOLS.to_owned();
        cursor = None;
    }
    if phase == PHASE_CODE_SYMBOLS {
        rebuild_code_symbol_documents(connection, &lease, companion_plan, cursor.as_deref())?;
        phase = PHASE_CODE_CHUNKS.to_owned();
        cursor = None;
    }
    if phase == PHASE_CODE_CHUNKS {
        rebuild_code_chunk_documents(connection, &lease, companion_plan, cursor.as_deref())?;
        phase = PHASE_STALE_LABELS.to_owned();
        cursor = None;
    }
    for (expected_phase, table, enabled, next_phase) in [
        (
            PHASE_STALE_LABELS,
            "graph_bm25_label_grams",
            true,
            PHASE_STALE_SEMANTIC,
        ),
        (
            PHASE_STALE_SEMANTIC,
            "graph_semantic_documents",
            companion_plan.semantic,
            PHASE_STALE_VECTOR,
        ),
        (
            PHASE_STALE_VECTOR,
            "graph_vector_documents",
            companion_plan.vector,
            PHASE_ACTIVATE,
        ),
    ] {
        if phase == expected_phase {
            delete_stale_table_rows_in_bounded_batches(
                connection,
                &lease,
                table,
                expected_phase,
                next_phase,
                cursor.as_deref(),
                enabled,
            )?;
            phase = next_phase.to_owned();
            cursor = None;
        }
    }
    if phase != PHASE_ACTIVATE {
        return Err(StorageError::InvalidInput(format!(
            "unknown BM25 rebuild checkpoint phase '{phase}'"
        )));
    }
    activate_rebuild_generation(connection, &lease, companion_plan, finalize_generation)?;

    Ok(())
}

fn companion_rebuild_plan(connection: &Connection) -> Result<CompanionRebuildPlan, StorageError> {
    let expected_count = retrievable_source_document_count(connection)?;
    let (semantic_generation, vector_generation) = connection.query_row(
        "SELECT semantic_generation, vector_generation
         FROM graph_bm25_route_state WHERE id = 1",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    Ok(CompanionRebuildPlan {
        semantic: table_row_count(connection, "graph_semantic_documents")? != expected_count
            || semantic_generation != LOCAL_TOKENIZER_VERSION
            || table_has_mismatched_tokenizer_version(connection, "graph_semantic_documents")?
            || derived_document_identity_mismatch(connection, "graph_semantic_documents")?,
        vector: table_row_count(connection, "graph_vector_documents")? != expected_count
            || vector_generation != LOCAL_TOKENIZER_VERSION
            || table_has_mismatched_tokenizer_version(connection, "graph_vector_documents")?
            || derived_document_identity_mismatch(connection, "graph_vector_documents")?,
    })
}

fn prepare_rebuild_generation(
    connection: &Connection,
    lease: &bm25_routing::RebuildLease,
) -> Result<(), StorageError> {
    let transaction = connection.unchecked_transaction()?;
    bm25_routing::renew_rebuild(&transaction, lease)?;
    schema::prepare_bm25_rebuild_table(&transaction)?;
    bm25_routing::checkpoint_rebuild(&transaction, lease, PHASE_CLEAR_ROUTE_DOCUMENTS, None)?;
    transaction.commit()?;
    Ok(())
}

fn activate_rebuild_generation<F>(
    connection: &Connection,
    lease: &bm25_routing::RebuildLease,
    companion_plan: CompanionRebuildPlan,
    finalize_generation: F,
) -> Result<(), StorageError>
where
    F: FnOnce(&Connection) -> Result<(), StorageError>,
{
    let transaction = connection.unchecked_transaction()?;
    bm25_routing::renew_rebuild(&transaction, lease)?;
    verify_rebuild_generation(&transaction)?;
    schema::activate_bm25_rebuild_table(&transaction)?;
    let graph_version = if table_exists(&transaction, "graph_state")? {
        transaction.query_row(
            "SELECT graph_version FROM graph_state WHERE id = 1",
            [],
            |row| row.get::<_, u64>(0),
        )?
    } else {
        0
    };
    bm25_routing::finish_rebuild(
        &transaction,
        lease,
        graph_version,
        companion_plan.semantic.then_some(LOCAL_TOKENIZER_VERSION),
        companion_plan.vector.then_some(LOCAL_TOKENIZER_VERSION),
    )?;
    finalize_generation(&transaction)?;
    transaction.commit()?;
    schema::drop_retired_bm25_table(connection)?;
    Ok(())
}

fn verify_rebuild_generation(connection: &Connection) -> Result<(), StorageError> {
    let expected_count = retrievable_source_document_count(connection)?;
    let counts = (
        table_row_count(connection, schema::GRAPH_BM25_REBUILD_TABLE)?,
        table_row_count(connection, "graph_bm25_route_documents")?,
        connection.query_row(
            "SELECT COALESCE(SUM(document_count), 0) FROM graph_bm25_route_groups",
            [],
            |row| row.get::<_, usize>(0),
        )?,
        connection.query_row(
            "SELECT document_count FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| row.get::<_, usize>(0),
        )?,
        table_row_count(connection, "graph_semantic_documents")?,
        table_row_count(connection, "graph_vector_documents")?,
    );
    let route_identity_mismatch = connection.query_row(
        "SELECT EXISTS(
             SELECT 1
             FROM graph_bm25_route_documents route
             LEFT JOIN graph_bm25_rebuild rebuilt
               ON rebuilt.rowid = route.fts_rowid
              AND rebuilt.document_id = route.document_id
             WHERE rebuilt.rowid IS NULL
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if counts
        != (
            expected_count,
            expected_count,
            expected_count,
            expected_count,
            expected_count,
            expected_count,
        )
        || route_identity_mismatch
        || table_has_mismatched_tokenizer_version(connection, "graph_semantic_documents")?
        || table_has_mismatched_tokenizer_version(connection, "graph_vector_documents")?
        || derived_document_identity_mismatch(connection, "graph_semantic_documents")?
        || derived_document_identity_mismatch(connection, "graph_vector_documents")?
    {
        return Err(StorageError::InvalidInput(format!(
            "BM25 rebuild generation is incomplete: expected {expected_count}, observed \
             bm25={}, route_documents={}, grouped={}, state={}, semantic={}, vector={}",
            counts.0, counts.1, counts.2, counts.3, counts.4, counts.5
        )));
    }
    Ok(())
}

fn acquire_rebuild_lease(
    connection: &Connection,
) -> Result<Option<bm25_routing::RebuildLease>, StorageError> {
    match bm25_routing::begin_rebuild(connection) {
        Ok(lease) => return Ok(Some(lease)),
        Err(StorageError::Busy(_)) => {}
        Err(error) => return Err(error),
    }
    for delay_ms in REBUILD_LEASE_WAIT_DELAYS_MS {
        thread::sleep(Duration::from_millis(delay_ms));
        let state = connection
            .query_row(
                "SELECT state FROM graph_bm25_route_state WHERE id = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if state.as_deref() == Some("fresh") && !derived_documents_missing(connection)? {
            return Ok(None);
        }
        match bm25_routing::begin_rebuild(connection) {
            Ok(lease) => return Ok(Some(lease)),
            Err(StorageError::Busy(_)) => {}
            Err(error) => return Err(error),
        }
    }
    Err(StorageError::Busy(
        "timed out waiting for the active BM25 route rebuild lease".to_owned(),
    ))
}

pub(super) fn derived_documents_missing(connection: &Connection) -> Result<bool, StorageError> {
    let expected_count = retrievable_source_document_count(connection)?;
    let bm25_count = table_row_count(connection, "graph_bm25")?;
    let semantic_count = table_row_count(connection, "graph_semantic_documents")?;
    let vector_count = table_row_count(connection, "graph_vector_documents")?;
    let route_document_count = table_row_count(connection, "graph_bm25_route_documents")?;
    let grouped_document_count = connection.query_row(
        "SELECT COALESCE(SUM(document_count), 0) FROM graph_bm25_route_groups",
        [],
        |row| row.get::<_, usize>(0),
    )?;
    let expected_graph_version = if table_exists(connection, "graph_state")? {
        connection.query_row(
            "SELECT graph_version FROM graph_state WHERE id = 1",
            [],
            |row| row.get::<_, u64>(0),
        )?
    } else {
        0
    };
    let route_algorithm_is_current = connection
        .query_row(
            "SELECT state = 'fresh' AND algorithm_version = ?1 AND document_count = ?2
                AND indexed_graph_version = ?3
                AND semantic_generation = ?4 AND vector_generation = ?4
         FROM graph_bm25_route_state WHERE id = 1",
            params![
                bm25_routing::ROUTING_ALGORITHM_VERSION,
                expected_count,
                expected_graph_version,
                LOCAL_TOKENIZER_VERSION
            ],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false);
    Ok(bm25_count != expected_count
        || semantic_count != expected_count
        || vector_count != expected_count
        || route_document_count != expected_count
        || grouped_document_count != expected_count
        || !route_algorithm_is_current)
}

pub(super) fn cleanup_retired_bm25_generation_if_current(
    connection: &Connection,
) -> Result<(), StorageError> {
    let transaction = connection.unchecked_transaction()?;
    let fresh_generation = transaction.execute(
        "UPDATE graph_bm25_route_state
         SET document_count = document_count
         WHERE id = 1 AND state = 'fresh'",
        [],
    )? == 1;
    if fresh_generation && !derived_documents_missing(&transaction)? {
        schema::drop_retired_bm25_table(&transaction)?;
    }
    transaction.commit()?;
    Ok(())
}

fn delete_stale_table_rows_in_bounded_batches(
    connection: &Connection,
    lease: &bm25_routing::RebuildLease,
    table: &'static str,
    phase: &'static str,
    next_phase: &'static str,
    initial_cursor: Option<&str>,
    enabled: bool,
) -> Result<(), StorageError> {
    if !enabled {
        let transaction = connection.unchecked_transaction()?;
        bm25_routing::renew_rebuild(&transaction, lease)?;
        bm25_routing::checkpoint_rebuild(&transaction, lease, next_phase, None)?;
        transaction.commit()?;
        return Ok(());
    }
    let page_sql = format!(
        "SELECT COUNT(*), COALESCE(MAX(rowid), 0)
         FROM (
             SELECT rowid FROM {table}
             WHERE rowid > ?1
             ORDER BY rowid
             LIMIT ?2
         )"
    );
    let delete_sql = format!(
        "DELETE FROM {table}
         WHERE rowid > ?1 AND rowid <= ?2
           AND NOT EXISTS (
               SELECT 1 FROM graph_bm25_route_documents AS route
               WHERE route.document_id = {table}.document_id
           )"
    );
    let mut cursor = initial_cursor
        .map(|cursor| {
            cursor.parse::<i64>().map_err(|_| {
                StorageError::InvalidInput(format!(
                    "invalid BM25 rebuild rowid cursor '{cursor}' for phase '{phase}'"
                ))
            })
        })
        .transpose()?
        .unwrap_or(0);
    loop {
        let transaction = connection.unchecked_transaction()?;
        bm25_routing::renew_rebuild(&transaction, lease)?;
        let (page_count, next_cursor) = transaction.query_row(
            &page_sql,
            params![cursor, REBUILD_DELETE_BATCH_SIZE],
            |row| Ok((row.get::<_, usize>(0)?, row.get::<_, i64>(1)?)),
        )?;
        if page_count == 0 {
            bm25_routing::checkpoint_rebuild(&transaction, lease, next_phase, None)?;
            transaction.commit()?;
            return Ok(());
        }
        transaction.execute(&delete_sql, params![cursor, next_cursor])?;
        if page_count < REBUILD_DELETE_BATCH_SIZE {
            bm25_routing::checkpoint_rebuild(&transaction, lease, next_phase, None)?;
        } else {
            let next_cursor = next_cursor.to_string();
            bm25_routing::checkpoint_rebuild(&transaction, lease, phase, Some(&next_cursor))?;
        }
        transaction.commit()?;
        if page_count < REBUILD_DELETE_BATCH_SIZE {
            return Ok(());
        }
        cursor = next_cursor;
    }
}

fn delete_table_in_bounded_batches(
    connection: &Connection,
    lease: &bm25_routing::RebuildLease,
    table: &'static str,
    phase: &'static str,
    next_phase: &'static str,
) -> Result<(), StorageError> {
    let sql = format!(
        "DELETE FROM {table}
         WHERE rowid IN (
             SELECT rowid FROM {table} ORDER BY rowid LIMIT ?1
         )"
    );
    loop {
        let transaction = connection.unchecked_transaction()?;
        bm25_routing::renew_rebuild(&transaction, lease)?;
        let deleted = transaction.execute(&sql, params![REBUILD_DELETE_BATCH_SIZE])?;
        bm25_routing::checkpoint_rebuild(
            &transaction,
            lease,
            if deleted < REBUILD_DELETE_BATCH_SIZE {
                next_phase
            } else {
                phase
            },
            None,
        )?;
        transaction.commit()?;
        if deleted < REBUILD_DELETE_BATCH_SIZE {
            return Ok(());
        }
    }
}

fn rebuild_evidence_documents(
    connection: &Connection,
    lease: &bm25_routing::RebuildLease,
    companion_plan: CompanionRebuildPlan,
    initial_cursor: Option<&str>,
) -> Result<(), StorageError> {
    let mut cursor = initial_cursor.map(|evidence_id| EvidenceRebuildKey {
        evidence_id: evidence_id.to_owned(),
    });
    loop {
        let page = evidence_rebuild_page(connection, cursor.as_ref())?;
        let Some(next_cursor) = page.keys.last().cloned() else {
            let transaction = connection.unchecked_transaction()?;
            bm25_routing::renew_rebuild(&transaction, lease)?;
            bm25_routing::checkpoint_rebuild(&transaction, lease, PHASE_CODE_SYMBOLS, None)?;
            transaction.commit()?;
            return Ok(());
        };
        let transaction = connection.unchecked_transaction()?;
        bm25_routing::renew_rebuild(&transaction, lease)?;
        for key in &page.keys {
            rebuild_evidence_document(&transaction, key, companion_plan)?;
        }
        bm25_routing::checkpoint_rebuild(
            &transaction,
            lease,
            if page.page_is_complete {
                PHASE_CODE_SYMBOLS
            } else {
                PHASE_EVIDENCE
            },
            (!page.page_is_complete).then_some(next_cursor.evidence_id.as_str()),
        )?;
        transaction.commit()?;
        if page.page_is_complete {
            return Ok(());
        }
        cursor = Some(next_cursor);
    }
}

fn rebuild_evidence_document(
    connection: &Connection,
    key: &EvidenceRebuildKey,
    companion_plan: CompanionRebuildPlan,
) -> Result<(), StorageError> {
    let (
        source_scope,
        source_path,
        content,
        status,
        modality,
        source_hash,
        parent_evidence_id,
        embedding_model,
        embedding_dimension,
        graph_version,
    ) = connection.query_row(
        "SELECT source_scope, source_path, content, status, modality, source_hash,
                parent_evidence_id, embedding_model, embedding_dimension,
                created_graph_version
         FROM evidence
         WHERE id = ?1",
        params![key.evidence_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<u16>>(8)?,
                row.get::<_, u64>(9)?,
            ))
        },
    )?;
    let entities = entities_for_evidence(connection, &key.evidence_id)?;
    let entity_labels = entities
        .iter()
        .map(|entity| entity.label.clone())
        .collect::<Vec<_>>();
    let source_hash = source_hash.unwrap_or_else(|| {
        super::super::super::indexing::source_hash(&source_scope, source_path.as_deref(), &content)
    });
    let extraction = EvidenceExtractionMetadata {
        modality: parse_evidence_modality(&modality)?,
        source_hash: Some(source_hash.clone()),
        parent_evidence_id,
        embedding_model,
        embedding_dimension,
        ..EvidenceExtractionMetadata::text_span()
    };
    replace_evidence_document(
        connection,
        EvidenceDocumentInput {
            evidence_id: &key.evidence_id,
            source_scope: &source_scope,
            source_path: source_path.as_deref(),
            entity_labels: &entity_labels,
            content: &content,
            status: parse_fact_status(&status)?,
            extraction: &extraction,
            source_hash: &source_hash,
            write: RetrievalWriteContext {
                graph_version,
                bm25_target: Bm25WriteTarget::Rebuild,
                refresh_labels: true,
                refresh_semantic: companion_plan.semantic,
                refresh_vector: companion_plan.vector,
            },
        },
    )
}

fn parse_evidence_modality(value: &str) -> Result<EvidenceModality, StorageError> {
    match value {
        "text_span" => Ok(EvidenceModality::TextSpan),
        "image_asset" => Ok(EvidenceModality::ImageAsset),
        "ocr_text" => Ok(EvidenceModality::OcrText),
        "caption" => Ok(EvidenceModality::Caption),
        "image_embedding" => Ok(EvidenceModality::ImageEmbedding),
        "table" => Ok(EvidenceModality::Table),
        "layout_region" => Ok(EvidenceModality::LayoutRegion),
        _ => Err(StorageError::InvalidInput(format!(
            "unknown evidence modality '{value}'"
        ))),
    }
}

fn rebuild_code_symbol_documents(
    connection: &Connection,
    lease: &bm25_routing::RebuildLease,
    companion_plan: CompanionRebuildPlan,
    initial_cursor: Option<&str>,
) -> Result<(), StorageError> {
    if !table_exists(connection, "code_symbols")? {
        checkpoint_empty_phase(connection, lease, PHASE_CODE_CHUNKS)?;
        return Ok(());
    }
    let mut cursor = decode_code_rebuild_cursor(initial_cursor, PHASE_CODE_SYMBOLS)?;
    loop {
        let page = code_symbol_rebuild_page(connection, cursor.as_ref())?;
        let Some(next_cursor) = page.keys.last().cloned() else {
            checkpoint_empty_phase(connection, lease, PHASE_CODE_CHUNKS)?;
            return Ok(());
        };
        let transaction = connection.unchecked_transaction()?;
        bm25_routing::renew_rebuild(&transaction, lease)?;
        for key in &page.keys {
            rebuild_code_symbol_document(&transaction, key, companion_plan)?;
        }
        let encoded_cursor = (!page.page_is_complete)
            .then(|| encode_code_rebuild_cursor(&next_cursor))
            .transpose()?;
        bm25_routing::checkpoint_rebuild(
            &transaction,
            lease,
            if page.page_is_complete {
                PHASE_CODE_CHUNKS
            } else {
                PHASE_CODE_SYMBOLS
            },
            encoded_cursor.as_deref(),
        )?;
        transaction.commit()?;
        if page.page_is_complete {
            return Ok(());
        }
        cursor = Some(next_cursor);
    }
}

fn rebuild_code_symbol_document(
    connection: &Connection,
    key: &CodeRebuildKey,
    companion_plan: CompanionRebuildPlan,
) -> Result<(), StorageError> {
    let (name, kind, graph_version) = connection.query_row(
        "SELECT name, kind, created_graph_version
         FROM code_symbols
         WHERE source_scope = ?1 AND path = ?2 AND symbol_id = ?3",
        params![key.source_scope, key.path, key.document_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        },
    )?;
    insert_code_symbol_document(
        connection,
        &key.source_scope,
        &key.path,
        &key.document_id,
        &name,
        &kind,
        RetrievalWriteContext {
            graph_version,
            bm25_target: Bm25WriteTarget::Rebuild,
            refresh_labels: true,
            refresh_semantic: companion_plan.semantic,
            refresh_vector: companion_plan.vector,
        },
    )
}

fn checkpoint_empty_phase(
    connection: &Connection,
    lease: &bm25_routing::RebuildLease,
    next_phase: &'static str,
) -> Result<(), StorageError> {
    let transaction = connection.unchecked_transaction()?;
    bm25_routing::renew_rebuild(&transaction, lease)?;
    bm25_routing::checkpoint_rebuild(&transaction, lease, next_phase, None)?;
    transaction.commit()?;
    Ok(())
}

fn rebuild_code_chunk_documents(
    connection: &Connection,
    lease: &bm25_routing::RebuildLease,
    companion_plan: CompanionRebuildPlan,
    initial_cursor: Option<&str>,
) -> Result<(), StorageError> {
    if !table_exists(connection, "code_chunks")? {
        checkpoint_empty_phase(connection, lease, PHASE_STALE_LABELS)?;
        return Ok(());
    }
    let mut cursor = decode_code_rebuild_cursor(initial_cursor, PHASE_CODE_CHUNKS)?;
    loop {
        let page = code_chunk_rebuild_page(connection, cursor.as_ref())?;
        let Some(next_cursor) = page.keys.last().cloned() else {
            checkpoint_empty_phase(connection, lease, PHASE_STALE_LABELS)?;
            return Ok(());
        };
        let transaction = connection.unchecked_transaction()?;
        bm25_routing::renew_rebuild(&transaction, lease)?;
        for key in &page.keys {
            rebuild_code_chunk_document(&transaction, key, companion_plan)?;
        }
        let encoded_cursor = (!page.page_is_complete)
            .then(|| encode_code_rebuild_cursor(&next_cursor))
            .transpose()?;
        bm25_routing::checkpoint_rebuild(
            &transaction,
            lease,
            if page.page_is_complete {
                PHASE_STALE_LABELS
            } else {
                PHASE_CODE_CHUNKS
            },
            encoded_cursor.as_deref(),
        )?;
        transaction.commit()?;
        if page.page_is_complete {
            return Ok(());
        }
        cursor = Some(next_cursor);
    }
}

fn rebuild_code_chunk_document(
    connection: &Connection,
    key: &CodeRebuildKey,
    companion_plan: CompanionRebuildPlan,
) -> Result<(), StorageError> {
    let (content, graph_version) = connection.query_row(
        "SELECT content, created_graph_version
         FROM code_chunks
         WHERE source_scope = ?1 AND path = ?2 AND chunk_id = ?3",
        params![key.source_scope, key.path, key.document_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
    )?;
    let linked_symbol_ids =
        linked_symbol_ids_for_chunk(connection, &key.source_scope, &key.path, &key.document_id)?;
    insert_code_chunk_document(
        connection,
        &key.source_scope,
        &key.path,
        &key.document_id,
        &linked_symbol_ids,
        &content,
        RetrievalWriteContext {
            graph_version,
            bm25_target: Bm25WriteTarget::Rebuild,
            refresh_labels: true,
            refresh_semantic: companion_plan.semantic,
            refresh_vector: companion_plan.vector,
        },
    )
}

fn linked_symbol_ids_for_chunk(
    connection: &Connection,
    source_scope: &str,
    path: &str,
    chunk_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT symbol_id
        FROM code_chunk_symbols
        WHERE source_scope = ?1 AND path = ?2 AND chunk_id = ?3
        ORDER BY symbol_id ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope, path, chunk_id], |row| {
        row.get::<_, String>(0)
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, StorageError> {
    let exists = connection.query_row(
        "SELECT EXISTS (
            SELECT 1 FROM sqlite_master
            WHERE type = 'table' AND name = ?1
        )",
        params![table],
        |row| row.get::<_, bool>(0),
    )?;

    Ok(exists)
}

fn optional_table_row_count(
    connection: &Connection,
    table: &'static str,
) -> Result<usize, StorageError> {
    if table_exists(connection, table)? {
        table_row_count(connection, table)
    } else {
        Ok(0)
    }
}

fn table_row_count(connection: &Connection, table: &'static str) -> Result<usize, StorageError> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    connection
        .query_row(&sql, [], |row| row.get::<_, usize>(0))
        .map_err(StorageError::from)
}

fn table_has_mismatched_tokenizer_version(
    connection: &Connection,
    table: &'static str,
) -> Result<bool, StorageError> {
    let sql = format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE tokenizer_version <> ?1)");
    connection
        .query_row(&sql, params![LOCAL_TOKENIZER_VERSION], |row| {
            row.get::<_, bool>(0)
        })
        .map_err(StorageError::from)
}

fn retrievable_source_document_count(connection: &Connection) -> Result<usize, StorageError> {
    let evidence_count = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM evidence
            WHERE status IN ('accepted', 'proposed')
            ",
            [],
            |row| row.get::<_, usize>(0),
        )
        .map_err(StorageError::from)?;

    Ok(evidence_count
        + optional_table_row_count(connection, "code_symbols")?
        + optional_table_row_count(connection, "code_chunks")?)
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod migration_tests;
