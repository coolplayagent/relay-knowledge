use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SoftwareGlobalRequest, SoftwareRelationship,
        SoftwareRelationshipInput,
    },
    storage::StorageError,
};

// Compatibility edges contain no independent facts. Keep the scope predicate in
// every UNION branch so SQLite never materializes another repository's edges.
const RELATIONSHIP_FACTS_SQL: &str = "
    SELECT source_scope, 'documents' AS relationship_kind, topic_id AS target_id,
           'topic' AS target_kind, name AS target_hint, 'resolved' AS resolution_state,
           10000 AS confidence_basis_points, 'extracted' AS confidence_tier,
           source_path AS evidence_path, line_start AS evidence_line_start,
           line_end AS evidence_line_end, NULL AS component_language, 1 AS edge_rank
    FROM software_topics WHERE source_scope = ?1
    UNION ALL
    SELECT source_scope, 'depends_on', component_id, 'component', name,
           relationship_state, confidence_basis_points, 'extracted', evidence_path,
           evidence_line_start, evidence_line_end, language_id, 1
    FROM software_components WHERE source_scope = ?1
    UNION ALL
    SELECT source_scope, 'uses_sdk', usage_id, 'sdk_usage', COALESCE(target_hint, module),
           resolution_state, confidence_basis_points, 'ambiguous', evidence_path,
           evidence_line_start, evidence_line_end, NULL, 1
    FROM software_sdk_usages WHERE source_scope = ?1
    UNION ALL
    SELECT source_scope,
           CASE edge_kind WHEN 'defines_config' THEN 'configures'
                          WHEN 'reads_config' THEN 'configures'
                          WHEN 'guards_code' THEN 'configures' ELSE 'references' END,
           feature_flag_id, 'configuration', source_key, 'inferred',
           confidence_basis_points, confidence_tier, path, line_start, line_end, NULL, edge_rank
    FROM (
        SELECT *, ROW_NUMBER() OVER (
            PARTITION BY source_scope, feature_flag_id, path, line_start,
                         CASE WHEN edge_kind IN ('defines_config', 'reads_config', 'guards_code')
                              THEN 'configures' ELSE 'references' END
            ORDER BY confidence_basis_points DESC, line_end DESC, usage_id ASC
        ) AS edge_rank
        FROM code_repository_feature_flags WHERE source_scope = ?1
    )
";

const RELATIONSHIP_COLUMNS_SQL: &str = "
    files.repository_id, relationships.source_scope,
    relationships.relationship_kind, files.software_file_id,
    'file', relationships.target_id,
    relationships.target_kind, relationships.target_hint,
    relationships.resolution_state, relationships.confidence_basis_points,
    relationships.confidence_tier, relationships.evidence_path,
    relationships.evidence_line_start, relationships.evidence_line_end
";

/// Validates each joined fact with bounded row memory and counts selected edges without writes.
/// The durable Relationships phase still publishes this count before ontology/freshness.
pub(in crate::storage::sqlite::software) fn relationship_count_for_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<usize, StorageError> {
    let query = format!(
        "WITH relationships AS ({RELATIONSHIP_FACTS_SQL})
         SELECT {RELATIONSHIP_COLUMNS_SQL}, 0, relationships.edge_rank FROM relationships
         JOIN software_files files ON files.source_scope = relationships.source_scope
                                  AND files.path = relationships.evidence_path"
    );
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok((relationship_from_row(row)?, row.get::<_, u64>(15)? == 1))
    })?;
    let mut count = 0_usize;
    for row in rows {
        let (input, selected) = row?;
        // Validate even a losing duplicate, as the former write path did. Use
        // the domain constructor so Unicode whitespace and future invariants
        // cannot diverge between publication and queries.
        SoftwareRelationship::new(input)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
        count = count.checked_add(usize::from(selected)).ok_or_else(|| {
            StorageError::CapacityExceeded("software relationship count overflow".to_owned())
        })?;
    }
    Ok(count)
}

pub(in crate::storage::sqlite::software) fn relationships_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareRelationship>, StorageError> {
    let path_filter = super::super::path_filter_sql_for_column(
        "relationships.evidence_path",
        &request.repository.path_filters,
    );
    let language_filter = relationship_language_filter_sql(&request.repository.language_filters);
    let query = format!(
        "
        WITH relationships AS ({RELATIONSHIP_FACTS_SQL})
        SELECT {RELATIONSHIP_COLUMNS_SQL}, status.projected_graph_version
        FROM relationships
        JOIN software_files files
          ON files.source_scope = relationships.source_scope
         AND files.path = relationships.evidence_path
        JOIN software_global_status status ON status.source_scope = relationships.source_scope
        WHERE relationships.source_scope = ?1
          AND relationships.edge_rank = 1
        {path_filter}
        {language_filter}
        ORDER BY
            CASE relationships.relationship_kind
                WHEN 'depends_on' THEN 0
                WHEN 'uses_sdk' THEN 1
                WHEN 'documents' THEN 2
                WHEN 'configures' THEN 3
                ELSE 4
            END ASC,
            CASE relationships.resolution_state
                WHEN 'declared' THEN 0
                WHEN 'resolved' THEN 1
                WHEN 'extracted' THEN 2
                WHEN 'inferred' THEN 3
                WHEN 'locked' THEN 4
                ELSE 5
            END ASC,
            CASE files.file_role
                WHEN 'dependency_manifest' THEN 0
                WHEN 'build_manifest' THEN 1
                WHEN 'deployment' THEN 2
                WHEN 'source' THEN 3
                WHEN 'configuration' THEN 4
                WHEN 'documentation' THEN 5
                ELSE 6
            END ASC,
            relationships.confidence_basis_points DESC,
            relationships.evidence_path ASC,
            relationships.evidence_line_start ASC,
            relationships.target_id ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::super::push_path_filter_values(&mut values, &request.repository.path_filters);
    push_relationship_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), relationship_from_row)?;

    rows.map(|row| {
        row.map_err(StorageError::from).and_then(|input| {
            SoftwareRelationship::new(input)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))
        })
    })
    .collect()
}

fn relationship_language_filter_sql(filters: &[String]) -> String {
    let clauses = filters
        .iter()
        .map(|_| {
            "(files.language_id = ? OR \
             (relationships.relationship_kind = 'depends_on' AND relationships.component_language = ?))"
        })
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND ({})", clauses.join(" OR "))
    }
}

fn push_relationship_language_filter_values(values: &mut Vec<Value>, filters: &[String]) {
    for filter in filters {
        values.push(Value::Text(filter.clone()));
        values.push(Value::Text(filter.clone()));
    }
}

fn relationship_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareRelationshipInput> {
    Ok(SoftwareRelationshipInput {
        repository_id: row.get(0)?,
        source_scope: row.get(1)?,
        relationship_kind: row.get(2)?,
        source_id: row.get(3)?,
        source_kind: row.get(4)?,
        target_id: row.get(5)?,
        target_kind: row.get(6)?,
        target_hint: row.get(7)?,
        resolution_state: row.get(8)?,
        confidence_basis_points: row.get(9)?,
        confidence_tier: row.get(10)?,
        evidence_path: row.get(11)?,
        evidence_line_range: RepositoryCodeRange {
            start: row.get(12)?,
            end: row.get(13)?,
        },
        created_graph_version: GraphVersion::new(row.get::<_, u64>(14)?),
    })
}

#[cfg(test)]
#[path = "relationships_tests.rs"]
mod tests;
