use rusqlite::{Connection, Row, params};

use crate::{
    domain::{
        CodeChunkRecord, CodeExtractionMetadata, CodeFileRecord, CodeGraphBatch,
        CodeGraphCommitReceipt, CodeParseStatus, CodeParseStatusCounts, CodeRange,
        CodeReferenceFields, CodeReferenceKind, CodeReferenceRecord, CodeResolutionState,
        CodeSymbolKind, CodeSymbolRecord, DomainError, GraphVersion, SourceScope,
    },
    storage::{
        CodeChunkSearchRequest, CodeReferenceSearchRequest, CodeSymbolSearchRequest, StorageError,
    },
};

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    preserve_incompatible_code_graph_tables(connection)?;
    connection.execute_batch(
        "
        DROP INDEX IF EXISTS code_symbols_lookup;
        DROP INDEX IF EXISTS code_references_lookup;
        DROP INDEX IF EXISTS code_chunks_lookup;

        CREATE TABLE IF NOT EXISTS code_files (
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            language_id TEXT NOT NULL,
            parse_status TEXT NOT NULL,
            diagnostic TEXT,
            created_graph_version INTEGER NOT NULL,
            PRIMARY KEY (source_scope, path)
        );

        CREATE TABLE IF NOT EXISTS code_symbols (
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            symbol_id TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            grammar_version TEXT NOT NULL,
            query_name TEXT NOT NULL,
            query_version TEXT NOT NULL,
            node_kind TEXT NOT NULL,
            capture_kind TEXT NOT NULL,
            created_graph_version INTEGER NOT NULL,
            PRIMARY KEY (source_scope, path, symbol_id),
            FOREIGN KEY (source_scope, path)
                REFERENCES code_files(source_scope, path)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_references (
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            reference_id TEXT NOT NULL,
            symbol_text TEXT NOT NULL,
            kind TEXT NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            resolution_state TEXT NOT NULL,
            target_symbol_id TEXT,
            grammar_version TEXT NOT NULL,
            query_name TEXT NOT NULL,
            query_version TEXT NOT NULL,
            node_kind TEXT NOT NULL,
            capture_kind TEXT NOT NULL,
            created_graph_version INTEGER NOT NULL,
            PRIMARY KEY (source_scope, path, reference_id),
            FOREIGN KEY (source_scope, path)
                REFERENCES code_files(source_scope, path)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_chunks (
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            chunk_id TEXT NOT NULL,
            content TEXT NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            grammar_version TEXT,
            query_name TEXT,
            query_version TEXT,
            node_kind TEXT,
            capture_kind TEXT,
            created_graph_version INTEGER NOT NULL,
            PRIMARY KEY (source_scope, path, chunk_id),
            FOREIGN KEY (source_scope, path)
                REFERENCES code_files(source_scope, path)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_chunk_symbols (
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            chunk_id TEXT NOT NULL,
            symbol_id TEXT NOT NULL,
            PRIMARY KEY (source_scope, path, chunk_id, symbol_id),
            FOREIGN KEY (source_scope, path, chunk_id)
                REFERENCES code_chunks(source_scope, path, chunk_id)
                ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS code_symbols_lookup
            ON code_symbols(source_scope, name, path);
        CREATE INDEX IF NOT EXISTS code_references_lookup
            ON code_references(source_scope, symbol_text, target_symbol_id);
        CREATE INDEX IF NOT EXISTS code_chunks_lookup
            ON code_chunks(source_scope, path);
        ",
    )?;

    Ok(())
}

fn preserve_incompatible_code_graph_tables(connection: &Connection) -> Result<(), StorageError> {
    if !code_graph_tables_are_incompatible(connection)? {
        return Ok(());
    }

    for table in [
        "code_chunk_symbols",
        "code_chunks",
        "code_references",
        "code_symbols",
        "code_files",
    ] {
        if table_exists(connection, table)? {
            let legacy = next_legacy_table_name(connection, table)?;
            connection.execute(&format!("ALTER TABLE {table} RENAME TO {legacy}"), [])?;
        }
    }

    Ok(())
}

fn code_graph_tables_are_incompatible(connection: &Connection) -> Result<bool, StorageError> {
    for (table, required_columns) in [
        (
            "code_files",
            &[
                "source_scope",
                "path",
                "content_hash",
                "language_id",
                "parse_status",
                "created_graph_version",
            ][..],
        ),
        (
            "code_symbols",
            &[
                "source_scope",
                "path",
                "symbol_id",
                "name",
                "kind",
                "created_graph_version",
            ][..],
        ),
        (
            "code_references",
            &[
                "source_scope",
                "path",
                "reference_id",
                "symbol_text",
                "resolution_state",
                "created_graph_version",
            ][..],
        ),
        (
            "code_chunks",
            &[
                "source_scope",
                "path",
                "chunk_id",
                "content",
                "created_graph_version",
            ][..],
        ),
        (
            "code_chunk_symbols",
            &["source_scope", "path", "chunk_id", "symbol_id"][..],
        ),
    ] {
        if table_exists(connection, table)?
            && !table_has_columns(connection, table, required_columns)?
        {
            return Ok(true);
        }
    }

    Ok(false)
}

fn table_has_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> Result<bool, StorageError> {
    let columns = table_columns(connection, table)?;

    Ok(required_columns
        .iter()
        .all(|required| columns.iter().any(|column| column == required)))
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, StorageError> {
    let exists = connection.query_row(
        "SELECT EXISTS (
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table' AND name = ?1
        )",
        params![table],
        |row| row.get::<_, bool>(0),
    )?;

    Ok(exists)
}

fn next_legacy_table_name(connection: &Connection, table: &str) -> Result<String, StorageError> {
    for index in 0..1000 {
        let candidate = format!("{table}_legacy_{index}");
        if !table_exists(connection, &candidate)? {
            return Ok(candidate);
        }
    }

    Err(StorageError::InvalidInput(format!(
        "could not find a free legacy table name for {table}"
    )))
}

pub(super) fn commit_batch(
    connection: &mut Connection,
    batch: CodeGraphBatch,
) -> Result<CodeGraphCommitReceipt, StorageError> {
    let transaction = connection.transaction()?;
    let current = super::current_graph_version_in_transaction(&transaction)?;
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

pub(super) fn search_symbols(
    connection: &mut Connection,
    request: CodeSymbolSearchRequest,
) -> Result<Vec<CodeSymbolRecord>, StorageError> {
    validate_limit("code symbol search limit", request.limit)?;
    let scope = normalize_filter("source_scope", request.source_scope)?;
    let path = normalize_filter("code_path", request.path)?;
    let name = normalize_filter("symbol_name", request.name)?;
    let mut statement = connection.prepare(
        "
        SELECT source_scope, path, symbol_id, name, kind, start_byte, end_byte,
               start_line, end_line, grammar_version, query_name, query_version,
               node_kind, capture_kind
        FROM code_symbols
        WHERE (?1 IS NULL OR source_scope = ?1)
          AND (?2 IS NULL OR path = ?2)
          AND (?3 IS NULL OR lower(name) LIKE '%' || lower(?3) || '%')
          AND created_graph_version <= ?4
        ORDER BY created_graph_version DESC, source_scope ASC, path ASC,
                 start_line ASC, symbol_id ASC
        LIMIT ?5
        ",
    )?;
    let rows = statement.query_map(
        params![
            scope.as_deref(),
            path.as_deref(),
            name.as_deref(),
            request.graph_version.get(),
            request.limit
        ],
        row_to_symbol,
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?
        .into_iter()
        .map(|raw| raw.into_record())
        .collect()
}

pub(super) fn search_references(
    connection: &mut Connection,
    request: CodeReferenceSearchRequest,
) -> Result<Vec<CodeReferenceRecord>, StorageError> {
    validate_limit("code reference search limit", request.limit)?;
    let scope = normalize_filter("source_scope", request.source_scope)?;
    let path = normalize_filter("code_path", request.path)?;
    let symbol_text = normalize_filter("symbol_text", request.symbol_text)?;
    let target_symbol_id = normalize_filter("target_symbol_id", request.target_symbol_id)?;
    let mut statement = connection.prepare(
        "
        SELECT source_scope, path, reference_id, symbol_text, kind, start_byte,
               end_byte, start_line, end_line, resolution_state, target_symbol_id,
               grammar_version, query_name, query_version, node_kind, capture_kind
        FROM code_references
        WHERE (?1 IS NULL OR source_scope = ?1)
          AND (?2 IS NULL OR path = ?2)
          AND (?3 IS NULL OR lower(symbol_text) LIKE '%' || lower(?3) || '%')
          AND (?4 IS NULL OR target_symbol_id = ?4)
          AND created_graph_version <= ?5
        ORDER BY created_graph_version DESC, source_scope ASC, path ASC,
                 start_line ASC, reference_id ASC
        LIMIT ?6
        ",
    )?;
    let rows = statement.query_map(
        params![
            scope.as_deref(),
            path.as_deref(),
            symbol_text.as_deref(),
            target_symbol_id.as_deref(),
            request.graph_version.get(),
            request.limit
        ],
        row_to_reference,
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?
        .into_iter()
        .map(|raw| raw.into_record())
        .collect()
}

pub(super) fn search_chunks(
    connection: &mut Connection,
    request: CodeChunkSearchRequest,
) -> Result<Vec<CodeChunkRecord>, StorageError> {
    validate_limit("code chunk search limit", request.limit)?;
    let scope = normalize_filter("source_scope", request.source_scope)?;
    let path = normalize_filter("code_path", request.path)?;
    let query = normalize_filter("code_query", request.query)?;
    let mut statement = connection.prepare(
        "
        SELECT source_scope, path, chunk_id, content, start_byte, end_byte,
               start_line, end_line, grammar_version, query_name, query_version,
               node_kind, capture_kind
        FROM code_chunks
        WHERE (?1 IS NULL OR source_scope = ?1)
          AND (?2 IS NULL OR path = ?2)
          AND (?3 IS NULL OR lower(content) LIKE '%' || lower(?3) || '%')
          AND created_graph_version <= ?4
        ORDER BY created_graph_version DESC, source_scope ASC, path ASC,
                 start_line ASC, chunk_id ASC
        LIMIT ?5
        ",
    )?;
    let rows = statement.query_map(
        params![
            scope.as_deref(),
            path.as_deref(),
            query.as_deref(),
            request.graph_version.get(),
            request.limit
        ],
        row_to_chunk,
    )?;
    let raw_chunks = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    raw_chunks
        .into_iter()
        .map(|raw| {
            let linked_symbol_ids =
                linked_symbols(connection, &raw.source_scope, &raw.path, &raw.chunk_id)?;
            raw.into_record(linked_symbol_ids)
        })
        .collect()
}

pub(super) fn parse_status_counts(
    connection: &Connection,
) -> Result<CodeParseStatusCounts, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT parse_status, COUNT(*)
        FROM code_files
        GROUP BY parse_status
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
    })?;
    let mut counts = CodeParseStatusCounts::default();
    for row in rows {
        let (status, count) = row.map_err(StorageError::from)?;
        match parse_status(&status)? {
            CodeParseStatus::Parsed => counts.parsed = count,
            CodeParseStatus::Partial => counts.partial = count,
            CodeParseStatus::TextOnly => counts.text_only = count,
            CodeParseStatus::Failed => counts.failed = count,
        }
    }

    Ok(counts)
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
    super::retrieval::delete_code_documents(connection, file.source_scope.as_str(), &file.path)?;
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
    super::retrieval::insert_code_symbol_document(
        connection,
        &source_scope,
        &path,
        &symbol_id,
        &name,
        &kind,
        graph_version.get(),
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
    super::retrieval::insert_code_chunk_document(
        connection,
        &source_scope,
        &path,
        &chunk_id,
        &linked_symbol_ids,
        &content,
        graph_version.get(),
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

struct RawSymbol {
    source_scope: String,
    path: String,
    symbol_id: String,
    name: String,
    kind: String,
    range: RawRange,
    extraction: CodeExtractionMetadata,
}

impl RawSymbol {
    fn into_record(self) -> Result<CodeSymbolRecord, StorageError> {
        CodeSymbolRecord::new(
            self.symbol_id,
            parse_scope(self.source_scope)?,
            self.path,
            self.name,
            parse_symbol_kind(&self.kind)?,
            self.range.into_range()?,
            self.extraction,
        )
        .map_err(domain_error)
    }
}

struct RawReference {
    source_scope: String,
    path: String,
    reference_id: String,
    symbol_text: String,
    kind: String,
    range: RawRange,
    resolution_state: String,
    target_symbol_id: Option<String>,
    extraction: CodeExtractionMetadata,
}

impl RawReference {
    fn into_record(self) -> Result<CodeReferenceRecord, StorageError> {
        CodeReferenceRecord::new(CodeReferenceFields {
            reference_id: self.reference_id,
            source_scope: parse_scope(self.source_scope)?,
            path: self.path,
            symbol_text: self.symbol_text,
            kind: parse_reference_kind(&self.kind)?,
            range: self.range.into_range()?,
            resolution_state: parse_resolution_state(&self.resolution_state)?,
            target_symbol_id: self.target_symbol_id,
            extraction: self.extraction,
        })
        .map_err(domain_error)
    }
}

struct RawChunk {
    source_scope: String,
    path: String,
    chunk_id: String,
    content: String,
    range: RawRange,
    extraction: Option<CodeExtractionMetadata>,
}

impl RawChunk {
    fn into_record(self, linked_symbol_ids: Vec<String>) -> Result<CodeChunkRecord, StorageError> {
        CodeChunkRecord::new(
            self.chunk_id,
            parse_scope(self.source_scope)?,
            self.path,
            self.content,
            self.range.into_range()?,
            linked_symbol_ids,
            self.extraction,
        )
        .map_err(domain_error)
    }
}

fn row_to_symbol(row: &Row<'_>) -> rusqlite::Result<RawSymbol> {
    Ok(RawSymbol {
        source_scope: row.get(0)?,
        path: row.get(1)?,
        symbol_id: row.get(2)?,
        name: row.get(3)?,
        kind: row.get(4)?,
        range: row_range(row, 5)?,
        extraction: extraction(
            row.get(9)?,
            row.get(10)?,
            row.get(11)?,
            row.get(12)?,
            row.get(13)?,
        ),
    })
}

fn row_to_reference(row: &Row<'_>) -> rusqlite::Result<RawReference> {
    Ok(RawReference {
        source_scope: row.get(0)?,
        path: row.get(1)?,
        reference_id: row.get(2)?,
        symbol_text: row.get(3)?,
        kind: row.get(4)?,
        range: row_range(row, 5)?,
        resolution_state: row.get(9)?,
        target_symbol_id: row.get(10)?,
        extraction: extraction(
            row.get(11)?,
            row.get(12)?,
            row.get(13)?,
            row.get(14)?,
            row.get(15)?,
        ),
    })
}

fn row_to_chunk(row: &Row<'_>) -> rusqlite::Result<RawChunk> {
    Ok(RawChunk {
        source_scope: row.get(0)?,
        path: row.get(1)?,
        chunk_id: row.get(2)?,
        content: row.get(3)?,
        range: row_range(row, 4)?,
        extraction: optional_extraction(
            row.get(8)?,
            row.get(9)?,
            row.get(10)?,
            row.get(11)?,
            row.get(12)?,
        ),
    })
}

#[derive(Debug, Clone, Copy)]
struct RawRange {
    start_byte: u32,
    end_byte: u32,
    start_line: u32,
    end_line: u32,
}

impl RawRange {
    fn into_range(self) -> Result<CodeRange, StorageError> {
        CodeRange::new(
            self.start_byte,
            self.end_byte,
            self.start_line,
            self.end_line,
        )
        .map_err(domain_error)
    }
}

fn row_range(row: &Row<'_>, start_index: usize) -> rusqlite::Result<RawRange> {
    Ok(RawRange {
        start_byte: row.get(start_index)?,
        end_byte: row.get(start_index + 1)?,
        start_line: row.get(start_index + 2)?,
        end_line: row.get(start_index + 3)?,
    })
}

fn linked_symbols(
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
    let rows = statement.query_map(params![source_scope, path, chunk_id], |row| row.get(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn extraction(
    grammar_version: String,
    query_name: String,
    query_version: String,
    node_kind: String,
    capture_kind: String,
) -> CodeExtractionMetadata {
    CodeExtractionMetadata {
        grammar_version,
        query_name,
        query_version,
        node_kind,
        capture_kind,
    }
}

fn optional_extraction(
    grammar_version: Option<String>,
    query_name: Option<String>,
    query_version: Option<String>,
    node_kind: Option<String>,
    capture_kind: Option<String>,
) -> Option<CodeExtractionMetadata> {
    Some(CodeExtractionMetadata {
        grammar_version: grammar_version?,
        query_name: query_name?,
        query_version: query_version?,
        node_kind: node_kind?,
        capture_kind: capture_kind?,
    })
}

fn normalize_filter(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, StorageError> {
    value
        .map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(StorageError::InvalidInput(format!(
                    "{field} filter must not be empty"
                )));
            }
            if trimmed.contains('\0') {
                return Err(StorageError::InvalidInput(format!(
                    "{field} filter must not contain NUL bytes"
                )));
            }

            Ok(trimmed.to_owned())
        })
        .transpose()
}

fn validate_limit(label: &'static str, limit: usize) -> Result<(), StorageError> {
    if limit == 0 {
        return Err(StorageError::InvalidInput(format!(
            "{label} must be greater than zero"
        )));
    }

    Ok(())
}

fn parse_scope(value: String) -> Result<SourceScope, StorageError> {
    SourceScope::parse(value).map_err(domain_error)
}

fn parse_status(value: &str) -> Result<CodeParseStatus, StorageError> {
    match value {
        "parsed" => Ok(CodeParseStatus::Parsed),
        "partial" => Ok(CodeParseStatus::Partial),
        "text_only" => Ok(CodeParseStatus::TextOnly),
        "failed" => Ok(CodeParseStatus::Failed),
        _ => Err(invalid_code_metadata(format!(
            "unknown code parse status '{value}'"
        ))),
    }
}

fn parse_symbol_kind(value: &str) -> Result<CodeSymbolKind, StorageError> {
    match value {
        "function" => Ok(CodeSymbolKind::Function),
        "method" => Ok(CodeSymbolKind::Method),
        "class" => Ok(CodeSymbolKind::Class),
        "interface" => Ok(CodeSymbolKind::Interface),
        "module" => Ok(CodeSymbolKind::Module),
        "type" => Ok(CodeSymbolKind::Type),
        "constant" => Ok(CodeSymbolKind::Constant),
        "field" => Ok(CodeSymbolKind::Field),
        "variable" => Ok(CodeSymbolKind::Variable),
        _ => Err(invalid_code_metadata(format!(
            "unknown code symbol kind '{value}'"
        ))),
    }
}

fn parse_reference_kind(value: &str) -> Result<CodeReferenceKind, StorageError> {
    match value {
        "call" => Ok(CodeReferenceKind::Call),
        "type" => Ok(CodeReferenceKind::Type),
        "import" => Ok(CodeReferenceKind::Import),
        "implementation" => Ok(CodeReferenceKind::Implementation),
        _ => Err(invalid_code_metadata(format!(
            "unknown code reference kind '{value}'"
        ))),
    }
}

fn parse_resolution_state(value: &str) -> Result<CodeResolutionState, StorageError> {
    match value {
        "unresolved" => Ok(CodeResolutionState::Unresolved),
        "ambiguous" => Ok(CodeResolutionState::Ambiguous),
        "resolved" => Ok(CodeResolutionState::Resolved),
        _ => Err(invalid_code_metadata(format!(
            "unknown code resolution state '{value}'"
        ))),
    }
}

fn invalid_code_metadata(message: String) -> StorageError {
    StorageError::InvalidInput(format!("{message} in code graph metadata"))
}

fn domain_error(error: DomainError) -> StorageError {
    StorageError::InvalidInput(error.to_string())
}

#[cfg(test)]
#[path = "code_graph_tests.rs"]
mod code_graph_tests;
