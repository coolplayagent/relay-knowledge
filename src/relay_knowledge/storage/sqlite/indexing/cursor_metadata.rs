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
    let supplied_model_name = normalized_model_name(request.model_name)?;
    validate_model_dimension_pair(supplied_model_name.as_deref(), request.model_dimension)?;
    let supplied_model = match (supplied_model_name, request.model_dimension) {
        (Some(name), Some(dimension)) => Some(IndexedModelMetadata { name, dimension }),
        (None, None) => None,
        _ => unreachable!("model name and dimension pair was validated"),
    };
    let model = match indexed_model_metadata(connection, &request)? {
        Some(metadata) => Some(metadata),
        None => supplied_model.or(existing_cursor_model_metadata(connection, &request)?),
    };
    let (model_name, model_dimension) = model
        .map(|metadata| (Some(metadata.name), Some(metadata.dimension)))
        .unwrap_or((None, None));
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
        model_dimension,
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

struct IndexedModelMetadata {
    name: String,
    dimension: u32,
}

fn indexed_model_metadata(
    connection: &Connection,
    request: &CursorBackendMetadataRequest<'_>,
) -> Result<Option<IndexedModelMetadata>, StorageError> {
    let table = match request.kind {
        IndexKind::Bm25 => return Ok(None),
        IndexKind::Semantic => "graph_semantic_documents",
        IndexKind::Vector => "graph_vector_documents",
    };
    let include_all_scopes = if request.scope == super::DEFAULT_SCOPE {
        1_i64
    } else {
        0_i64
    };
    let sql = format!(
        "
        SELECT model, dimension
        FROM {table}
        WHERE created_graph_version > ?1
          AND created_graph_version <= ?2
          AND (?3 = 1 OR source_scope = ?4)
        GROUP BY model, dimension
        ORDER BY COUNT(*) DESC, MAX(created_graph_version) DESC, model ASC, dimension ASC
        LIMIT 1
        "
    );

    connection
        .query_row(
            &sql,
            params![
                request.cursor_before.get(),
                request.graph_version.get(),
                include_all_scopes,
                request.scope
            ],
            |row| {
                let name = row.get::<_, String>(0)?;
                let raw_dimension = row.get::<_, i64>(1)?;

                Ok((name, raw_dimension))
            },
        )
        .optional()?
        .map(|(name, raw_dimension)| {
            let name = normalized_model_name(Some(&name))?
                .expect("indexed model query returned a model name");
            let dimension = u64::try_from(raw_dimension)
                .map_err(|_| {
                    super::invalid_index_metadata(
                        "indexed model dimension must be non-negative".to_owned(),
                    )
                })
                .and_then(checked_model_dimension)?;

            Ok(IndexedModelMetadata { name, dimension })
        })
        .transpose()
}

fn existing_cursor_model_metadata(
    connection: &Connection,
    request: &CursorBackendMetadataRequest<'_>,
) -> Result<Option<IndexedModelMetadata>, StorageError> {
    connection
        .query_row(
            "
            SELECT model_name, model_dimension
            FROM index_cursors
            WHERE kind = ?1 AND source_scope = ?2 AND modality = ?3
            ",
            params![
                request.kind.as_str(),
                request.scope,
                request.modality.as_str()
            ],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<u64>>(1)?,
                ))
            },
        )
        .optional()?
        .map(|(raw_name, raw_dimension)| {
            let name = normalized_model_name(raw_name.as_deref())?;
            let dimension = raw_dimension.map(checked_model_dimension).transpose()?;
            validate_model_dimension_pair(name.as_deref(), dimension)?;

            Ok(match (name, dimension) {
                (Some(name), Some(dimension)) => Some(IndexedModelMetadata { name, dimension }),
                (None, None) => None,
                _ => unreachable!("existing cursor model pair was validated"),
            })
        })
        .transpose()
        .map(Option::flatten)
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
