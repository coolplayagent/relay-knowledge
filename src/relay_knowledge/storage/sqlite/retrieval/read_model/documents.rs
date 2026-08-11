use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{EvidenceExtractionMetadata, EvidenceModality, FactStatus},
    storage::StorageError,
};

use super::super::{
    aliases,
    bm25_routing::{self, Bm25RoutingText},
    context::retrievable_status,
    label_trigrams,
    local_model::{hashed_vector, stable_hash64, token_signature},
    read_model::schema::GRAPH_BM25_REBUILD_TABLE,
};

const LABEL_SEPARATOR: char = '\u{1f}';
const LOCAL_SEMANTIC_MODEL: &str = "relay-local-token-semantic-v1";
const LOCAL_VECTOR_MODEL: &str = "relay-local-hash-ann-v1";
pub(in crate::storage::sqlite) const LOCAL_TOKENIZER_VERSION: &str = "relay-normalized-terms-v3";
const LOCAL_VECTOR_DIMENSION: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::storage::sqlite) enum Bm25WriteTarget {
    Live,
    Rebuild,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::storage::sqlite) struct RetrievalWriteContext {
    pub(in crate::storage::sqlite) graph_version: u64,
    pub(in crate::storage::sqlite) bm25_target: Bm25WriteTarget,
    pub(in crate::storage::sqlite) refresh_labels: bool,
    pub(in crate::storage::sqlite) refresh_semantic: bool,
    pub(in crate::storage::sqlite) refresh_vector: bool,
}

impl Bm25WriteTarget {
    fn table_name(self) -> &'static str {
        match self {
            Self::Live => "graph_bm25",
            Self::Rebuild => GRAPH_BM25_REBUILD_TABLE,
        }
    }
}

pub(in crate::storage::sqlite) struct EvidenceDocumentInput<'a> {
    pub(in crate::storage::sqlite) evidence_id: &'a str,
    pub(in crate::storage::sqlite) source_scope: &'a str,
    pub(in crate::storage::sqlite) source_path: Option<&'a str>,
    pub(in crate::storage::sqlite) entity_labels: &'a [String],
    pub(in crate::storage::sqlite) content: &'a str,
    pub(in crate::storage::sqlite) status: FactStatus,
    pub(in crate::storage::sqlite) extraction: &'a EvidenceExtractionMetadata,
    pub(in crate::storage::sqlite) source_hash: &'a str,
    pub(in crate::storage::sqlite) write: RetrievalWriteContext,
}

pub(in crate::storage::sqlite) fn replace_evidence_document(
    connection: &Connection,
    input: EvidenceDocumentInput<'_>,
) -> Result<(), StorageError> {
    let document_id = evidence_document_id(input.evidence_id);
    if !retrievable_status(input.status) {
        delete_bm25_document(connection, input.write.bm25_target, &document_id)?;
        bm25_routing::delete_document(connection, &document_id, input.write.graph_version)?;
        if input.write.refresh_labels {
            label_trigrams::delete_document(connection, &document_id)?;
        }
        if input.write.refresh_semantic {
            connection.execute(
                "DELETE FROM graph_semantic_documents WHERE document_id = ?1",
                params![document_id],
            )?;
        }
        if input.write.refresh_vector {
            connection.execute(
                "DELETE FROM graph_vector_documents WHERE document_id = ?1",
                params![document_id],
            )?;
        }
        return Ok(());
    }
    let entity_labels = join_labels(input.entity_labels);
    let entity_aliases = aliases_from_strings(input.entity_labels);
    insert_bm25_document(
        connection,
        Bm25DocumentInput {
            document_id: &document_id,
            document_kind: "evidence",
            evidence_id: input.evidence_id,
            parent_evidence_id: input.extraction.parent_evidence_id.as_deref(),
            modality: input.extraction.modality.as_str(),
            graph_version: input.write.graph_version,
            source_scope: input.source_scope,
            source_path: input.source_path,
            entity_labels: &entity_labels,
            entity_aliases: &entity_aliases,
            content: input.content,
        },
        input.write.bm25_target,
    )?;
    let label_gram_state = if input.write.refresh_labels {
        label_trigrams::replace_document(
            connection,
            label_trigrams::LabelGramDocument {
                document_id: &document_id,
                document_kind: "evidence",
                source_scope: input.source_scope,
                graph_version: input.write.graph_version,
                labels: input.entity_labels,
            },
        )?
        .route_state()
    } else {
        "not_refreshed"
    };
    bm25_routing::mark_label_gram_state(
        connection,
        &document_id,
        input.write.graph_version,
        label_gram_state,
    )?;
    if input.write.refresh_semantic {
        replace_semantic_document(
            connection,
            SemanticDocumentInput {
                document_id: &document_id,
                document_kind: "evidence",
                evidence_id: input.evidence_id,
                parent_evidence_id: input.extraction.parent_evidence_id.as_deref(),
                modality: input.extraction.modality,
                source_scope: input.source_scope,
                source_path: input.source_path,
                entity_labels: input.entity_labels,
                content: input.content,
                source_hash: input.source_hash,
                graph_version: input.write.graph_version,
                model: input
                    .extraction
                    .embedding_model
                    .as_deref()
                    .unwrap_or(LOCAL_SEMANTIC_MODEL),
                dimension: input
                    .extraction
                    .embedding_dimension
                    .map(usize::from)
                    .unwrap_or(LOCAL_VECTOR_DIMENSION),
            },
        )?;
    }
    if input.write.refresh_vector {
        replace_vector_document(
            connection,
            VectorDocumentInput {
                document_id: &document_id,
                document_kind: "evidence",
                evidence_id: input.evidence_id,
                parent_evidence_id: input.extraction.parent_evidence_id.as_deref(),
                modality: input.extraction.modality,
                source_scope: input.source_scope,
                source_path: input.source_path,
                entity_labels: input.entity_labels,
                content: input.content,
                source_hash: input.source_hash,
                graph_version: input.write.graph_version,
                model: input
                    .extraction
                    .embedding_model
                    .as_deref()
                    .unwrap_or(LOCAL_VECTOR_MODEL),
                dimension: input
                    .extraction
                    .embedding_dimension
                    .map(usize::from)
                    .unwrap_or(LOCAL_VECTOR_DIMENSION),
            },
        )?;
    }

    Ok(())
}

pub(in crate::storage::sqlite) fn delete_code_documents(
    connection: &Connection,
    source_scope: &str,
    path: &str,
    graph_version: u64,
) -> Result<(), StorageError> {
    loop {
        let batch = bm25_routing::code_document_batch(connection, source_scope, path)?;
        if batch.is_empty() {
            break;
        }
        let document_ids = batch
            .iter()
            .map(|identity| identity.document_id.clone())
            .collect::<Vec<_>>();
        let identities = batch
            .iter()
            .map(|identity| (identity.fts_rowid, identity.document_id.as_str()))
            .collect::<Vec<_>>();
        let document_ids_json = serde_json::to_string(&document_ids)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
        let identities_json = serde_json::to_string(&identities)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
        let deleted_fts = connection.execute(
            "WITH expected(fts_rowid, document_id) AS (
                 SELECT CAST(json_extract(value, '$[0]') AS INTEGER),
                        CAST(json_extract(value, '$[1]') AS TEXT)
                 FROM json_each(?1)
             )
             DELETE FROM graph_bm25
             WHERE rowid IN (SELECT fts_rowid FROM expected)
               AND EXISTS (
                   SELECT 1 FROM expected
                   WHERE expected.fts_rowid = graph_bm25.rowid
                     AND expected.document_id = graph_bm25.document_id
               )",
            params![identities_json],
        )?;
        if deleted_fts != batch.len() {
            return Err(StorageError::InvalidInput(format!(
                "BM25 path batch mapped {} documents but deleted {deleted_fts} FTS rows",
                batch.len()
            )));
        }
        label_trigrams::delete_documents(connection, &document_ids)?;
        for table in ["graph_semantic_documents", "graph_vector_documents"] {
            connection.execute(
                &format!(
                    "DELETE FROM {table}
                     WHERE document_id IN (
                         SELECT CAST(value AS TEXT) FROM json_each(?1)
                     )"
                ),
                params![document_ids_json],
            )?;
        }
        let deleted_routes =
            bm25_routing::delete_code_document_batch(connection, source_scope, path, batch.len())?;
        if deleted_routes != batch.len() {
            return Err(StorageError::InvalidInput(format!(
                "BM25 path batch mapped {} documents but deleted {deleted_routes} routes",
                batch.len()
            )));
        }
    }
    bm25_routing::mark_graph_version(connection, graph_version)?;

    Ok(())
}

pub(in crate::storage::sqlite) fn insert_code_symbol_document(
    connection: &Connection,
    source_scope: &str,
    path: &str,
    symbol_id: &str,
    name: &str,
    kind: &str,
    write: RetrievalWriteContext,
) -> Result<(), StorageError> {
    let document_id = code_document_id("symbol", source_scope, path, symbol_id);
    let content = format!("{name} {kind} {path} {symbol_id}");
    let labels = [name.to_owned()];
    let entity_aliases = aliases::lexical_aliases(&[name, kind, path, symbol_id]);
    let source_hash = format!("{:016x}", stable_hash64(content.as_bytes()));
    let entity_labels = join_labels(&labels);
    insert_bm25_document(
        connection,
        Bm25DocumentInput {
            document_id: &document_id,
            document_kind: "code_symbol",
            evidence_id: symbol_id,
            parent_evidence_id: None,
            modality: "text_span",
            graph_version: write.graph_version,
            source_scope,
            source_path: Some(path),
            entity_labels: &entity_labels,
            entity_aliases: &entity_aliases,
            content: &content,
        },
        write.bm25_target,
    )?;
    let label_gram_state = if write.refresh_labels {
        label_trigrams::replace_document(
            connection,
            label_trigrams::LabelGramDocument {
                document_id: &document_id,
                document_kind: "code_symbol",
                source_scope,
                graph_version: write.graph_version,
                labels: &labels,
            },
        )?
        .route_state()
    } else {
        "not_refreshed"
    };
    bm25_routing::mark_label_gram_state(
        connection,
        &document_id,
        write.graph_version,
        label_gram_state,
    )?;
    if write.refresh_semantic {
        replace_semantic_document(
            connection,
            SemanticDocumentInput {
                document_id: &document_id,
                document_kind: "code_symbol",
                evidence_id: symbol_id,
                parent_evidence_id: None,
                modality: EvidenceModality::TextSpan,
                source_scope,
                source_path: Some(path),
                entity_labels: &labels,
                content: &content,
                source_hash: &source_hash,
                graph_version: write.graph_version,
                model: LOCAL_SEMANTIC_MODEL,
                dimension: LOCAL_VECTOR_DIMENSION,
            },
        )?;
    }
    if write.refresh_vector {
        replace_vector_document(
            connection,
            VectorDocumentInput {
                document_id: &document_id,
                document_kind: "code_symbol",
                evidence_id: symbol_id,
                parent_evidence_id: None,
                modality: EvidenceModality::TextSpan,
                source_scope,
                source_path: Some(path),
                entity_labels: &labels,
                content: &content,
                source_hash: &source_hash,
                graph_version: write.graph_version,
                model: LOCAL_VECTOR_MODEL,
                dimension: LOCAL_VECTOR_DIMENSION,
            },
        )?;
    }

    Ok(())
}

pub(in crate::storage::sqlite) fn insert_code_chunk_document(
    connection: &Connection,
    source_scope: &str,
    path: &str,
    chunk_id: &str,
    linked_symbol_ids: &[String],
    content: &str,
    write: RetrievalWriteContext,
) -> Result<(), StorageError> {
    let document_id = code_document_id("chunk", source_scope, path, chunk_id);
    let linked_symbols = linked_symbol_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let entity_aliases = aliases::lexical_aliases(&linked_symbols);
    let source_hash = format!("{:016x}", stable_hash64(content.as_bytes()));
    let entity_labels = join_labels(linked_symbol_ids);
    insert_bm25_document(
        connection,
        Bm25DocumentInput {
            document_id: &document_id,
            document_kind: "code_chunk",
            evidence_id: chunk_id,
            parent_evidence_id: None,
            modality: "text_span",
            graph_version: write.graph_version,
            source_scope,
            source_path: Some(path),
            entity_labels: &entity_labels,
            entity_aliases: &entity_aliases,
            content,
        },
        write.bm25_target,
    )?;
    let label_gram_state = if write.refresh_labels {
        label_trigrams::replace_document(
            connection,
            label_trigrams::LabelGramDocument {
                document_id: &document_id,
                document_kind: "code_chunk",
                source_scope,
                graph_version: write.graph_version,
                labels: linked_symbol_ids,
            },
        )?
        .route_state()
    } else {
        "not_refreshed"
    };
    bm25_routing::mark_label_gram_state(
        connection,
        &document_id,
        write.graph_version,
        label_gram_state,
    )?;
    if write.refresh_semantic {
        replace_semantic_document(
            connection,
            SemanticDocumentInput {
                document_id: &document_id,
                document_kind: "code_chunk",
                evidence_id: chunk_id,
                parent_evidence_id: None,
                modality: EvidenceModality::TextSpan,
                source_scope,
                source_path: Some(path),
                entity_labels: linked_symbol_ids,
                content,
                source_hash: &source_hash,
                graph_version: write.graph_version,
                model: LOCAL_SEMANTIC_MODEL,
                dimension: LOCAL_VECTOR_DIMENSION,
            },
        )?;
    }
    if write.refresh_vector {
        replace_vector_document(
            connection,
            VectorDocumentInput {
                document_id: &document_id,
                document_kind: "code_chunk",
                evidence_id: chunk_id,
                parent_evidence_id: None,
                modality: EvidenceModality::TextSpan,
                source_scope,
                source_path: Some(path),
                entity_labels: linked_symbol_ids,
                content,
                source_hash: &source_hash,
                graph_version: write.graph_version,
                model: LOCAL_VECTOR_MODEL,
                dimension: LOCAL_VECTOR_DIMENSION,
            },
        )?;
    }

    Ok(())
}

struct Bm25DocumentInput<'a> {
    document_id: &'a str,
    document_kind: &'a str,
    evidence_id: &'a str,
    parent_evidence_id: Option<&'a str>,
    modality: &'a str,
    graph_version: u64,
    source_scope: &'a str,
    source_path: Option<&'a str>,
    entity_labels: &'a str,
    entity_aliases: &'a str,
    content: &'a str,
}

fn insert_bm25_document(
    connection: &Connection,
    input: Bm25DocumentInput<'_>,
    target: Bm25WriteTarget,
) -> Result<(), StorageError> {
    let route = bm25_routing::prepare_document(Bm25RoutingText {
        source_scope: input.source_scope,
        source_path: input.source_path,
        entity_labels: input.entity_labels,
        entity_aliases: input.entity_aliases,
        content: input.content,
        graph_version: input.graph_version,
    });
    delete_bm25_document(connection, target, input.document_id)?;
    let insert_sql = format!(
        "
        INSERT INTO {} (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version, routing_key, source_scope, source_path,
            entity_labels, entity_aliases, content
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
        target.table_name()
    );
    connection.execute(
        &insert_sql,
        params![
            input.document_id,
            input.document_kind,
            input.evidence_id,
            input.parent_evidence_id,
            input.modality,
            input.graph_version,
            route.routing_key.as_str(),
            input.source_scope,
            input.source_path,
            input.entity_labels,
            input.entity_aliases,
            input.content,
        ],
    )?;
    let fts_rowid = connection.last_insert_rowid();
    bm25_routing::replace_document(
        connection,
        input.document_id,
        fts_rowid,
        input.document_kind,
        input.source_path,
        "pending",
        &route,
    )
}

fn delete_bm25_document(
    connection: &Connection,
    target: Bm25WriteTarget,
    document_id: &str,
) -> Result<(), StorageError> {
    let fts_rowid = connection
        .query_row(
            "SELECT fts_rowid
             FROM graph_bm25_route_documents
             WHERE document_id = ?1",
            params![document_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(fts_rowid) = fts_rowid else {
        return Ok(());
    };
    let delete_sql = format!(
        "DELETE FROM {} WHERE rowid = ?1 AND document_id = ?2",
        target.table_name()
    );
    let deleted = connection.execute(&delete_sql, params![fts_rowid, document_id])?;
    if deleted != 1 {
        return Err(StorageError::InvalidInput(format!(
            "BM25 document {document_id} maps to missing FTS rowid {fts_rowid}"
        )));
    }
    Ok(())
}

struct SemanticDocumentInput<'a> {
    document_id: &'a str,
    document_kind: &'a str,
    evidence_id: &'a str,
    parent_evidence_id: Option<&'a str>,
    modality: EvidenceModality,
    source_scope: &'a str,
    source_path: Option<&'a str>,
    entity_labels: &'a [String],
    content: &'a str,
    source_hash: &'a str,
    graph_version: u64,
    model: &'a str,
    dimension: usize,
}

fn replace_semantic_document(
    connection: &Connection,
    input: SemanticDocumentInput<'_>,
) -> Result<(), StorageError> {
    let signature = token_signature(input.content, input.entity_labels, input.source_path);
    connection.execute(
        "DELETE FROM graph_semantic_documents WHERE document_id = ?1",
        params![input.document_id],
    )?;
    connection.execute(
        "
        INSERT INTO graph_semantic_documents (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version, source_scope, source_path, entity_labels_json,
            content, token_signature_json, model, dimension, source_hash, tokenizer_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ",
        params![
            input.document_id,
            input.document_kind,
            input.evidence_id,
            input.parent_evidence_id,
            input.modality.as_str(),
            input.graph_version,
            input.source_scope,
            input.source_path,
            join_labels(input.entity_labels),
            input.content,
            json_string_array(&signature)?,
            input.model,
            input.dimension as i64,
            input.source_hash,
            LOCAL_TOKENIZER_VERSION,
        ],
    )?;

    Ok(())
}

struct VectorDocumentInput<'a> {
    document_id: &'a str,
    document_kind: &'a str,
    evidence_id: &'a str,
    parent_evidence_id: Option<&'a str>,
    modality: EvidenceModality,
    source_scope: &'a str,
    source_path: Option<&'a str>,
    entity_labels: &'a [String],
    content: &'a str,
    source_hash: &'a str,
    graph_version: u64,
    model: &'a str,
    dimension: usize,
}

fn replace_vector_document(
    connection: &Connection,
    input: VectorDocumentInput<'_>,
) -> Result<(), StorageError> {
    let vector = hashed_vector(
        input.content,
        input.entity_labels,
        input.source_path,
        input.dimension,
    );
    connection.execute(
        "DELETE FROM graph_vector_documents WHERE document_id = ?1",
        params![input.document_id],
    )?;
    connection.execute(
        "
        INSERT INTO graph_vector_documents (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version, source_scope, source_path, entity_labels_json,
            content, vector_json, model, dimension, source_hash, tokenizer_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ",
        params![
            input.document_id,
            input.document_kind,
            input.evidence_id,
            input.parent_evidence_id,
            input.modality.as_str(),
            input.graph_version,
            input.source_scope,
            input.source_path,
            join_labels(input.entity_labels),
            input.content,
            json_f64_array(&vector)?,
            input.model,
            input.dimension as i64,
            input.source_hash,
            LOCAL_TOKENIZER_VERSION,
        ],
    )?;

    Ok(())
}

fn evidence_document_id(evidence_id: &str) -> String {
    format!("evidence:{evidence_id}")
}

fn code_document_id(kind: &str, source_scope: &str, path: &str, id: &str) -> String {
    format!(
        "code:{kind}:{}:{source_scope}:{}:{path}:{}:{id}",
        source_scope.len(),
        path.len(),
        id.len()
    )
}

fn join_labels(labels: &[String]) -> String {
    serde_json::to_string(labels).unwrap_or_default()
}

fn json_string_array(values: &[String]) -> Result<String, StorageError> {
    serde_json::to_string(values).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn json_f64_array(values: &[f64]) -> Result<String, StorageError> {
    serde_json::to_string(values).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

pub(in crate::storage::sqlite::retrieval) fn parse_string_array(
    value: &str,
) -> Result<Vec<String>, StorageError> {
    serde_json::from_str(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

pub(in crate::storage::sqlite::retrieval) fn parse_f64_array(
    value: &str,
) -> Result<Vec<f64>, StorageError> {
    serde_json::from_str(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn aliases_from_strings(values: &[String]) -> String {
    let values = values.iter().map(String::as_str).collect::<Vec<_>>();
    aliases::lexical_aliases(&values)
}

pub(in crate::storage::sqlite::retrieval) fn split_labels(labels: String) -> Vec<String> {
    serde_json::from_str(&labels).unwrap_or_else(|_| {
        labels
            .split(LABEL_SEPARATOR)
            .filter(|label| !label.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

#[cfg(test)]
#[path = "documents_tests.rs"]
mod documents_tests;
