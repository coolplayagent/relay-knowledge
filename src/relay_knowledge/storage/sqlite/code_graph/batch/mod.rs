//! Transactional replacement of persisted code-graph facts.

use rusqlite::{Connection, TransactionBehavior, params};

use crate::{
    domain::{
        CodeChunkRecord, CodeFileRecord, CodeGraphBatch, CodeGraphCommitReceipt,
        CodeReferenceRecord, CodeSymbolRecord, GraphVersion,
    },
    storage::{
        StorageError,
        sqlite::{graph::current_graph_version_in_transaction, retrieval},
    },
};

pub(in crate::storage::sqlite) fn commit_batch(
    connection: &mut Connection,
    batch: CodeGraphBatch,
) -> Result<CodeGraphCommitReceipt, StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    retrieval::ensure_bm25_rebuild_inactive(&transaction)?;
    let current = current_graph_version_in_transaction(&transaction)?;
    let next = GraphVersion::new(current.get() + 1);
    let file_count = batch.files.len();
    let mut symbol_count = 0;
    let mut reference_count = 0;
    let mut chunk_count = 0;

    for file in batch.files {
        symbol_count += file.symbols.len();
        reference_count += file.references.len();
        chunk_count += file.chunks.len();
        replace_file_facts(&transaction, file, next)?;
    }

    transaction.execute(
        "INSERT INTO graph_mutations (
             graph_version, evidence_count, entity_count, relation_count, claim_count, event_count
         )
         VALUES (?1, 0, 0, 0, 0, 0)",
        params![next.get()],
    )?;
    transaction.execute(
        "UPDATE graph_state SET graph_version = ?1 WHERE id = 1",
        params![next.get()],
    )?;
    transaction.execute("UPDATE index_status SET state = 'stale'", [])?;
    transaction.commit()?;

    Ok(CodeGraphCommitReceipt {
        graph_version: next,
        file_count,
        symbol_count,
        reference_count,
        chunk_count,
    })
}

fn replace_file_facts(
    connection: &Connection,
    file: CodeFileRecord,
    graph_version: GraphVersion,
) -> Result<(), StorageError> {
    connection.execute(
        "DELETE FROM code_files WHERE source_scope = ?1 AND path = ?2",
        params![file.source_scope.as_str(), file.path],
    )?;
    retrieval::delete_code_documents(
        connection,
        file.source_scope.as_str(),
        &file.path,
        graph_version.get(),
    )?;
    connection.execute(
        "INSERT INTO code_files
         (source_scope, path, content_hash, language_id, parse_status, diagnostic,
          created_graph_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            file.source_scope.as_str(),
            file.path,
            file.content_hash,
            file.language_id,
            file.parse_status.as_str(),
            file.diagnostic.as_deref(),
            graph_version.get()
        ],
    )?;

    for symbol in file.symbols {
        insert_symbol(connection, symbol, graph_version)?;
    }
    for reference in file.references {
        insert_reference(connection, reference, graph_version)?;
    }
    for chunk in file.chunks {
        insert_chunk(connection, chunk, graph_version)?;
    }

    Ok(())
}

fn insert_symbol(
    connection: &Connection,
    symbol: CodeSymbolRecord,
    graph_version: GraphVersion,
) -> Result<(), StorageError> {
    let source_scope = symbol.source_scope.as_str().to_owned();
    let path = symbol.path.clone();
    let symbol_id = symbol.symbol_id.clone();
    let name = symbol.name.clone();
    let kind = symbol.kind.as_str().to_owned();
    connection.execute(
        "INSERT INTO code_symbols
         (source_scope, path, symbol_id, name, kind, start_byte, end_byte,
          start_line, end_line, grammar_version, query_name, query_version,
          node_kind, capture_kind, created_graph_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            symbol.source_scope.as_str(),
            symbol.path,
            symbol.symbol_id,
            symbol.name,
            symbol.kind.as_str(),
            symbol.range.start_byte,
            symbol.range.end_byte,
            symbol.range.start_line,
            symbol.range.end_line,
            symbol.extraction.grammar_version,
            symbol.extraction.query_name,
            symbol.extraction.query_version,
            symbol.extraction.node_kind,
            symbol.extraction.capture_kind,
            graph_version.get()
        ],
    )?;
    retrieval::insert_code_symbol_document(
        connection,
        &source_scope,
        &path,
        &symbol_id,
        &name,
        &kind,
        retrieval::RetrievalWriteContext {
            graph_version: graph_version.get(),
            bm25_target: retrieval::Bm25WriteTarget::Live,
            refresh_labels: true,
            refresh_semantic: true,
            refresh_vector: true,
        },
    )?;

    Ok(())
}

fn insert_reference(
    connection: &Connection,
    reference: CodeReferenceRecord,
    graph_version: GraphVersion,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO code_references
         (source_scope, path, reference_id, symbol_text, kind, start_byte, end_byte,
          start_line, end_line, resolution_state, target_symbol_id, grammar_version,
          query_name, query_version, node_kind, capture_kind, created_graph_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            reference.source_scope.as_str(),
            reference.path,
            reference.reference_id,
            reference.symbol_text,
            reference.kind.as_str(),
            reference.range.start_byte,
            reference.range.end_byte,
            reference.range.start_line,
            reference.range.end_line,
            reference.resolution_state.as_str(),
            reference.target_symbol_id.as_deref(),
            reference.extraction.grammar_version,
            reference.extraction.query_name,
            reference.extraction.query_version,
            reference.extraction.node_kind,
            reference.extraction.capture_kind,
            graph_version.get()
        ],
    )?;

    Ok(())
}

fn insert_chunk(
    connection: &Connection,
    chunk: CodeChunkRecord,
    graph_version: GraphVersion,
) -> Result<(), StorageError> {
    let extraction = chunk.extraction.as_ref();
    let source_scope = chunk.source_scope.as_str().to_owned();
    let path = chunk.path.clone();
    let chunk_id = chunk.chunk_id.clone();
    let linked_symbol_ids = chunk.linked_symbol_ids.clone();
    let content = chunk.content.clone();
    connection.execute(
        "INSERT INTO code_chunks
         (source_scope, path, chunk_id, content, start_byte, end_byte, start_line,
          end_line, grammar_version, query_name, query_version, node_kind,
          capture_kind, created_graph_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            chunk.source_scope.as_str(),
            chunk.path,
            chunk.chunk_id,
            chunk.content,
            chunk.range.start_byte,
            chunk.range.end_byte,
            chunk.range.start_line,
            chunk.range.end_line,
            extraction.map(|value| value.grammar_version.as_str()),
            extraction.map(|value| value.query_name.as_str()),
            extraction.map(|value| value.query_version.as_str()),
            extraction.map(|value| value.node_kind.as_str()),
            extraction.map(|value| value.capture_kind.as_str()),
            graph_version.get()
        ],
    )?;
    retrieval::insert_code_chunk_document(
        connection,
        &source_scope,
        &path,
        &chunk_id,
        &linked_symbol_ids,
        &content,
        retrieval::RetrievalWriteContext {
            graph_version: graph_version.get(),
            bm25_target: retrieval::Bm25WriteTarget::Live,
            refresh_labels: true,
            refresh_semantic: true,
            refresh_vector: true,
        },
    )?;
    for symbol_id in chunk.linked_symbol_ids {
        connection.execute(
            "INSERT INTO code_chunk_symbols (source_scope, path, chunk_id, symbol_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                chunk.source_scope.as_str(),
                chunk.path,
                chunk.chunk_id,
                symbol_id
            ],
        )?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod batch_tests;
