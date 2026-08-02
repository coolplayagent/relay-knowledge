use rusqlite::{Connection, params};

use crate::{
    domain::{EvidenceExtractionMetadata, EvidenceModality, FactStatus},
    storage::StorageError,
};

use super::super::{
    aliases,
    context::retrievable_status,
    label_trigrams,
    local_model::{hashed_vector, stable_hash64, token_signature},
};

const LABEL_SEPARATOR: char = '\u{1f}';
const LOCAL_SEMANTIC_MODEL: &str = "relay-local-token-semantic-v1";
const LOCAL_VECTOR_MODEL: &str = "relay-local-hash-ann-v1";
pub(in crate::storage::sqlite) const LOCAL_TOKENIZER_VERSION: &str = "relay-normalized-terms-v2";
const LOCAL_VECTOR_DIMENSION: usize = 16;

pub(in crate::storage::sqlite) struct EvidenceDocumentInput<'a> {
    pub(in crate::storage::sqlite) evidence_id: &'a str,
    pub(in crate::storage::sqlite) source_scope: &'a str,
    pub(in crate::storage::sqlite) source_path: Option<&'a str>,
    pub(in crate::storage::sqlite) entity_labels: &'a [String],
    pub(in crate::storage::sqlite) content: &'a str,
    pub(in crate::storage::sqlite) status: FactStatus,
    pub(in crate::storage::sqlite) extraction: &'a EvidenceExtractionMetadata,
    pub(in crate::storage::sqlite) source_hash: &'a str,
    pub(in crate::storage::sqlite) graph_version: u64,
}

pub(in crate::storage::sqlite) fn replace_evidence_document(
    connection: &Connection,
    input: EvidenceDocumentInput<'_>,
) -> Result<(), StorageError> {
    let document_id = evidence_document_id(input.evidence_id);
    label_trigrams::delete_document(connection, &document_id)?;
    connection.execute(
        "DELETE FROM graph_bm25 WHERE document_id = ?1",
        params![document_id],
    )?;
    connection.execute(
        "DELETE FROM graph_semantic_documents WHERE document_id = ?1",
        params![document_id],
    )?;
    connection.execute(
        "DELETE FROM graph_vector_documents WHERE document_id = ?1",
        params![document_id],
    )?;
    if !retrievable_status(input.status) {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version,
            source_scope, source_path, entity_labels, entity_aliases, content
        )
        VALUES (?1, 'evidence', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ",
        params![
            document_id,
            input.evidence_id,
            input.extraction.parent_evidence_id.as_deref(),
            input.extraction.modality.as_str(),
            input.graph_version,
            input.source_scope,
            input.source_path,
            join_labels(input.entity_labels),
            aliases_from_strings(input.entity_labels),
            input.content,
        ],
    )?;
    label_trigrams::replace_document(
        connection,
        label_trigrams::LabelGramDocument {
            document_id: &document_id,
            document_kind: "evidence",
            source_scope: input.source_scope,
            graph_version: input.graph_version,
            labels: input.entity_labels,
        },
    )?;
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
            graph_version: input.graph_version,
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
            graph_version: input.graph_version,
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

    Ok(())
}

pub(in crate::storage::sqlite) fn delete_code_documents(
    connection: &Connection,
    source_scope: &str,
    path: &str,
) -> Result<(), StorageError> {
    label_trigrams::delete_code_documents_for_path(connection, source_scope, path)?;
    connection.execute(
        "
        DELETE FROM graph_bm25
        WHERE document_kind IN ('code_symbol', 'code_chunk')
          AND source_scope = ?1
          AND source_path = ?2
        ",
        params![source_scope, path],
    )?;
    connection.execute(
        "
        DELETE FROM graph_semantic_documents
        WHERE document_kind IN ('code_symbol', 'code_chunk')
          AND source_scope = ?1
          AND source_path = ?2
        ",
        params![source_scope, path],
    )?;
    connection.execute(
        "
        DELETE FROM graph_vector_documents
        WHERE document_kind IN ('code_symbol', 'code_chunk')
          AND source_scope = ?1
          AND source_path = ?2
        ",
        params![source_scope, path],
    )?;

    Ok(())
}

pub(in crate::storage::sqlite) fn insert_code_symbol_document(
    connection: &Connection,
    source_scope: &str,
    path: &str,
    symbol_id: &str,
    name: &str,
    kind: &str,
    graph_version: u64,
) -> Result<(), StorageError> {
    let document_id = code_document_id("symbol", source_scope, path, symbol_id);
    let content = format!("{name} {kind} {path} {symbol_id}");
    let labels = [name.to_owned()];
    let entity_aliases = aliases::lexical_aliases(&[name, kind, path, symbol_id]);
    let source_hash = format!("{:016x}", stable_hash64(content.as_bytes()));
    connection.execute(
        "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version,
            source_scope, source_path, entity_labels, entity_aliases, content
        )
        VALUES (?1, 'code_symbol', ?2, NULL, 'text_span', ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            document_id,
            symbol_id,
            graph_version,
            source_scope,
            path,
            join_labels(&labels),
            entity_aliases,
            content
        ],
    )?;
    label_trigrams::replace_document(
        connection,
        label_trigrams::LabelGramDocument {
            document_id: &document_id,
            document_kind: "code_symbol",
            source_scope,
            graph_version,
            labels: &labels,
        },
    )?;
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
            graph_version,
            model: LOCAL_SEMANTIC_MODEL,
            dimension: LOCAL_VECTOR_DIMENSION,
        },
    )?;
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
            graph_version,
            model: LOCAL_VECTOR_MODEL,
            dimension: LOCAL_VECTOR_DIMENSION,
        },
    )?;

    Ok(())
}

pub(in crate::storage::sqlite) fn insert_code_chunk_document(
    connection: &Connection,
    source_scope: &str,
    path: &str,
    chunk_id: &str,
    linked_symbol_ids: &[String],
    content: &str,
    graph_version: u64,
) -> Result<(), StorageError> {
    let document_id = code_document_id("chunk", source_scope, path, chunk_id);
    let linked_symbols = linked_symbol_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let entity_aliases = aliases::lexical_aliases(&linked_symbols);
    let source_hash = format!("{:016x}", stable_hash64(content.as_bytes()));
    connection.execute(
        "
        INSERT INTO graph_bm25 (
            document_id, document_kind, evidence_id, parent_evidence_id, modality,
            created_graph_version,
            source_scope, source_path, entity_labels, entity_aliases, content
        )
        VALUES (?1, 'code_chunk', ?2, NULL, 'text_span', ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            document_id,
            chunk_id,
            graph_version,
            source_scope,
            path,
            join_labels(linked_symbol_ids),
            entity_aliases,
            content
        ],
    )?;
    label_trigrams::replace_document(
        connection,
        label_trigrams::LabelGramDocument {
            document_id: &document_id,
            document_kind: "code_chunk",
            source_scope,
            graph_version,
            labels: linked_symbol_ids,
        },
    )?;
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
            graph_version,
            model: LOCAL_SEMANTIC_MODEL,
            dimension: LOCAL_VECTOR_DIMENSION,
        },
    )?;
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
            graph_version,
            model: LOCAL_VECTOR_MODEL,
            dimension: LOCAL_VECTOR_DIMENSION,
        },
    )?;

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
    format!("code:{kind}:{source_scope}:{path}:{id}")
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
