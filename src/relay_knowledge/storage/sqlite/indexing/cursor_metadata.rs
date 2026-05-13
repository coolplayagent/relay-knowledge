use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{GraphVersion, IndexKind, IndexModality},
    storage::StorageError,
};

pub(super) struct CursorBackendMetadata {
    pub(super) source_hash: String,
    pub(super) backend_cursor: String,
    pub(super) model_name: Option<String>,
    pub(super) model_dimension: Option<u32>,
}

pub(super) struct CursorBackendMetadataRequest<'a> {
    pub(super) kind: IndexKind,
    pub(super) scope: &'a str,
    pub(super) modality: IndexModality,
    pub(super) cursor_before: GraphVersion,
    pub(super) graph_version: GraphVersion,
    pub(super) model_name: Option<&'a str>,
    pub(super) model_dimension: Option<u32>,
}

pub(super) fn cursor_backend_metadata(
    connection: &Connection,
    request: CursorBackendMetadataRequest<'_>,
) -> Result<CursorBackendMetadata, StorageError> {
    let model_name = normalized_model_name(request.model_name)?;
    validate_model_dimension_pair(model_name.as_deref(), request.model_dimension)?;
    let source_hash = cursor_source_hash(
        connection,
        request.scope,
        request.modality,
        request.cursor_before,
        request.graph_version,
    )?;
    let backend_cursor = cursor_backend_cursor(
        request.kind,
        request.scope,
        request.modality,
        request.graph_version,
        &source_hash,
    );

    Ok(CursorBackendMetadata {
        source_hash,
        backend_cursor,
        model_name,
        model_dimension: request.model_dimension,
    })
}

pub(super) fn cursor_indexed_graph_version(
    connection: &Connection,
    kind: IndexKind,
    scope: &str,
    modality: IndexModality,
) -> Result<Option<GraphVersion>, StorageError> {
    connection
        .query_row(
            "
            SELECT indexed_graph_version
            FROM index_cursors
            WHERE kind = ?1 AND source_scope = ?2 AND modality = ?3
            ",
            params![kind.as_str(), scope, modality.as_str()],
            |row| row.get::<_, u64>(0),
        )
        .optional()
        .map(|value| value.map(GraphVersion::new))
        .map_err(StorageError::from)
}

pub(super) fn checked_model_dimension(value: u64) -> Result<u32, StorageError> {
    let value = u32::try_from(value).map_err(|_| {
        super::invalid_index_metadata("index cursor model dimension is too large".to_owned())
    })?;
    if value == 0 {
        return Err(super::invalid_index_metadata(
            "index cursor model dimension must be greater than zero".to_owned(),
        ));
    }

    Ok(value)
}

fn cursor_source_hash(
    connection: &Connection,
    scope: &str,
    modality: IndexModality,
    cursor_before: GraphVersion,
    graph_version: GraphVersion,
) -> Result<String, StorageError> {
    let mut source_parts = BTreeSet::new();
    let mut statement = connection.prepare(
        "
        SELECT graph_version, affected_scopes_json, source_hashes_json
        FROM graph_mutations
        WHERE graph_version > ?1 AND graph_version <= ?2
        ORDER BY graph_version ASC
        ",
    )?;
    let mut rows = statement.query(params![cursor_before.get(), graph_version.get()])?;
    while let Some(row) = rows.next()? {
        let mutation_version = row.get::<_, u64>(0)?;
        let affected_scopes = super::parse_json_array(row.get::<_, String>(1)?)?;
        if scope != super::DEFAULT_SCOPE && !affected_scopes.iter().any(|value| value == scope) {
            continue;
        }

        let source_hashes = super::parse_json_array(row.get::<_, String>(2)?)?;
        if source_hashes.is_empty() {
            source_parts.insert(format!("mutation:{mutation_version}"));
        } else {
            source_parts.extend(source_hashes);
        }
    }

    let mut input = Vec::new();
    super::append_hash_part(&mut input, scope);
    super::append_hash_part(&mut input, modality.as_str());
    super::append_hash_part(&mut input, &graph_version.get().to_string());
    for part in source_parts {
        super::append_hash_part(&mut input, &part);
    }

    Ok(format!("{:016x}", super::stable_hash64(&input)))
}

fn cursor_backend_cursor(
    kind: IndexKind,
    scope: &str,
    modality: IndexModality,
    graph_version: GraphVersion,
    source_hash: &str,
) -> String {
    let mut input = Vec::new();
    super::append_hash_part(&mut input, kind.as_str());
    super::append_hash_part(&mut input, scope);
    super::append_hash_part(&mut input, modality.as_str());
    super::append_hash_part(&mut input, &graph_version.get().to_string());
    super::append_hash_part(&mut input, source_hash);

    format!(
        "{}:{}:{:016x}",
        kind.as_str(),
        modality.as_str(),
        super::stable_hash64(&input)
    )
}

fn normalized_model_name(value: Option<&str>) -> Result<Option<String>, StorageError> {
    value
        .map(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                Err(StorageError::InvalidInput(
                    "index cursor model name must not be empty".to_owned(),
                ))
            } else {
                Ok(trimmed.to_owned())
            }
        })
        .transpose()
}

fn validate_model_dimension_pair(
    model_name: Option<&str>,
    model_dimension: Option<u32>,
) -> Result<(), StorageError> {
    match (model_name, model_dimension) {
        (None, None) => Ok(()),
        (Some(_), Some(1..)) => Ok(()),
        (Some(_), Some(0)) => Err(StorageError::InvalidInput(
            "index cursor model dimension must be greater than zero".to_owned(),
        )),
        _ => Err(StorageError::InvalidInput(
            "index cursor model name and dimension must be supplied together".to_owned(),
        )),
    }
}
