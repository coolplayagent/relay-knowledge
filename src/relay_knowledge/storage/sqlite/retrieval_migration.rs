use rusqlite::{Connection, params};

use crate::{
    domain::{EvidenceExtractionMetadata, EvidenceModality},
    storage::StorageError,
};

use super::{
    EvidenceDocumentInput,
    context::{entities_for_evidence, parse_fact_status},
    insert_code_chunk_document, insert_code_symbol_document, replace_evidence_document,
};

pub(super) fn drop_incompatible_bm25_table(connection: &Connection) -> Result<bool, StorageError> {
    let exists = connection.query_row(
        "SELECT EXISTS (
            SELECT 1 FROM sqlite_master
            WHERE type = 'table' AND name = 'graph_bm25'
        )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Ok(false);
    }

    let mut statement = connection.prepare("PRAGMA table_info(graph_bm25)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let columns = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let required = [
        "created_graph_version",
        "parent_evidence_id",
        "modality",
        "entity_aliases",
    ];
    if required
        .iter()
        .any(|required_column| !columns.iter().any(|column| column == required_column))
    {
        connection.execute("DROP TABLE graph_bm25", [])?;
        return Ok(true);
    }

    Ok(false)
}

pub(super) fn rebuild_bm25_documents(connection: &Connection) -> Result<(), StorageError> {
    clear_retrieval_documents(connection)?;
    rebuild_evidence_documents(connection)?;
    rebuild_code_symbol_documents(connection)?;
    rebuild_code_chunk_documents(connection)?;

    Ok(())
}

pub(super) fn derived_documents_missing(connection: &Connection) -> Result<bool, StorageError> {
    let expected_count = retrievable_source_document_count(connection)?;
    if expected_count == 0 {
        return Ok(false);
    }

    let bm25_count = table_row_count(connection, "graph_bm25")?;
    let semantic_count = table_row_count(connection, "graph_semantic_documents")?;
    let vector_count = table_row_count(connection, "graph_vector_documents")?;

    Ok(bm25_count != expected_count
        || semantic_count != expected_count
        || vector_count != expected_count)
}

fn clear_retrieval_documents(connection: &Connection) -> Result<(), StorageError> {
    connection.execute("DELETE FROM graph_bm25", [])?;
    connection.execute("DELETE FROM graph_semantic_documents", [])?;
    connection.execute("DELETE FROM graph_vector_documents", [])?;

    Ok(())
}

fn rebuild_evidence_documents(connection: &Connection) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT id, source_scope, source_path, content, status, modality, source_hash,
               parent_evidence_id, embedding_model, embedding_dimension, created_graph_version
        FROM evidence
        WHERE status IN ('accepted', 'proposed')
        ORDER BY created_graph_version ASC, id ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<u16>>(9)?,
            row.get::<_, u64>(10)?,
        ))
    })?;
    let documents = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    for (
        evidence_id,
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
    ) in documents
    {
        let entities = entities_for_evidence(connection, &evidence_id)?;
        let entity_labels = entities
            .iter()
            .map(|entity| entity.label.clone())
            .collect::<Vec<_>>();
        let source_hash = source_hash.unwrap_or_else(|| {
            super::super::indexing::source_hash(&source_scope, source_path.as_deref(), &content)
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
                evidence_id: &evidence_id,
                source_scope: &source_scope,
                source_path: source_path.as_deref(),
                entity_labels: &entity_labels,
                content: &content,
                status: parse_fact_status(&status)?,
                extraction: &extraction,
                source_hash: &source_hash,
                graph_version,
            },
        )?;
    }

    Ok(())
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

fn rebuild_code_symbol_documents(connection: &Connection) -> Result<(), StorageError> {
    if !table_exists(connection, "code_symbols")? {
        return Ok(());
    }
    let mut statement = connection.prepare(
        "
        SELECT source_scope, path, symbol_id, name, kind, created_graph_version
        FROM code_symbols
        ORDER BY created_graph_version ASC, source_scope ASC, path ASC, symbol_id ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, u64>(5)?,
        ))
    })?;
    let documents = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    for (source_scope, path, symbol_id, name, kind, graph_version) in documents {
        insert_code_symbol_document(
            connection,
            &source_scope,
            &path,
            &symbol_id,
            &name,
            &kind,
            graph_version,
        )?;
    }

    Ok(())
}

fn rebuild_code_chunk_documents(connection: &Connection) -> Result<(), StorageError> {
    if !table_exists(connection, "code_chunks")? {
        return Ok(());
    }
    let mut statement = connection.prepare(
        "
        SELECT source_scope, path, chunk_id, content, created_graph_version
        FROM code_chunks
        ORDER BY created_graph_version ASC, source_scope ASC, path ASC, chunk_id ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, u64>(4)?,
        ))
    })?;
    let documents = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    for (source_scope, path, chunk_id, content, graph_version) in documents {
        let linked_symbol_ids =
            linked_symbol_ids_for_chunk(connection, &source_scope, &path, &chunk_id)?;
        insert_code_chunk_document(
            connection,
            &source_scope,
            &path,
            &chunk_id,
            &linked_symbol_ids,
            &content,
            graph_version,
        )?;
    }

    Ok(())
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
