use std::{collections::BTreeMap, io};

use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, SoftwareAssertionMode, SoftwareEntity, SoftwareEntityKind, SoftwareFactState,
        SoftwareGlobalKind, SoftwareGlobalRequest, SoftwarePredicate, SoftwareShapeDiagnostic,
        SoftwareShapeSeverity, SoftwareSourceKind, SoftwareStatement, SoftwareStatementResolution,
    },
    storage::StorageError,
};

pub(in super::super) fn entities_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareEntity>, StorageError> {
    let kind_filter = entity_kind_filter(request.kind);
    let path_filter = super::super::path_filter_sql_for_column(
        "primary_evidence_path",
        &request.repository.path_filters,
    );
    let language_filter = super::super::language_filter_sql_for_column(
        "language_id",
        &request.repository.language_filters,
    );
    let evidence_order = entity_evidence_order(request.kind);
    let query = format!(
        "
        SELECT occurrence_id, entity_key, repository_id, source_scope, entity_kind,
               name, namespace, source_kind, evidence_refs_json, attributes_json,
               created_graph_version
        FROM software_entities
        WHERE source_scope = ?1
          {kind_filter}
          {path_filter}
          {language_filter}
        ORDER BY {evidence_order}
        LIMIT ?
        "
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), entity_from_row)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(in super::super) fn entities_by_keys_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    entity_keys: &[String],
    limit: usize,
) -> Result<Vec<SoftwareEntity>, StorageError> {
    if entity_keys.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", entity_keys.len())
        .collect::<Vec<_>>()
        .join(", ");
    let path_filter = super::super::path_filter_sql_for_column(
        "primary_evidence_path",
        &request.repository.path_filters,
    );
    let language_filter = super::super::language_filter_sql_for_column(
        "language_id",
        &request.repository.language_filters,
    );
    let query = format!(
        "
        WITH ranked_entities AS (
            SELECT occurrence_id, entity_key, repository_id, source_scope, entity_kind,
                   name, namespace, source_kind, evidence_refs_json, attributes_json,
                   created_graph_version,
                   ROW_NUMBER() OVER (
                       PARTITION BY entity_key ORDER BY occurrence_id ASC
                   ) AS occurrence_rank
            FROM software_entities
            WHERE source_scope = ?1 AND entity_key IN ({placeholders})
              {path_filter}
              {language_filter}
        )
        SELECT occurrence_id, entity_key, repository_id, source_scope, entity_kind,
               name, namespace, source_kind, evidence_refs_json, attributes_json,
               created_graph_version
        FROM ranked_entities
        WHERE occurrence_rank = 1
        ORDER BY entity_key ASC, occurrence_id ASC
        LIMIT ?
        "
    );
    let mut values = std::iter::once(Value::Text(source_scope.to_owned()))
        .chain(entity_keys.iter().cloned().map(Value::Text))
        .collect::<Vec<_>>();
    super::super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), entity_from_row)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(in super::super) fn statements_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareStatement>, StorageError> {
    let state_filter = if request.kind == SoftwareGlobalKind::Conflicts {
        "AND (fact_state IN ('conflicting', 'superseded', 'rejected')
              OR resolution_state IN ('unresolved', 'ambiguous', 'external', 'conflicting'))"
    } else {
        ""
    };
    let path_filter = super::super::path_filter_sql_for_column(
        "primary_evidence_path",
        &request.repository.path_filters,
    );
    let language_filter = statement_language_filter(&request.repository.language_filters);
    let query = format!(
        "
        SELECT statement_id, subject_id, predicate, object_id, object_value,
               source_scope, source_kind, evidence_refs_json, assertion_mode,
               resolution_state, valid_from, valid_to, observed_at, extractor_id,
               extractor_version, confidence_basis_points, fact_state
        FROM software_statements
        WHERE source_scope = ?1
          {state_filter}
          {path_filter}
          {language_filter}
        ORDER BY fact_state ASC, predicate ASC, primary_evidence_path ASC,
                 source_kind ASC, statement_id ASC
        LIMIT ?
        "
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::super::push_path_filter_values(&mut values, &request.repository.path_filters);
    for language in &request.repository.language_filters {
        values.push(Value::Text(language.clone()));
    }
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), statement_from_row)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(in super::super) fn diagnostics_for_scope(
    connection: &Connection,
    source_scope: &str,
    limit: usize,
) -> Result<Vec<SoftwareShapeDiagnostic>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT diagnostic_id, shape_id, code, severity, statement_id, entity_key,
               field, message
        FROM software_ontology_diagnostics
        WHERE source_scope = ?1
        ORDER BY severity ASC, code ASC, diagnostic_id ASC
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(params![source_scope, limit as i64], |row| {
        Ok(SoftwareShapeDiagnostic {
            diagnostic_id: row.get(0)?,
            shape_id: row.get(1)?,
            code: row.get(2)?,
            severity: parse_enum(row.get::<_, String>(3)?, SoftwareShapeSeverity::parse)?,
            statement_id: row.get(4)?,
            entity_key: row.get(5)?,
            field: row.get(6)?,
            message: row.get(7)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn entity_kind_filter(kind: SoftwareGlobalKind) -> &'static str {
    match kind {
        SoftwareGlobalKind::Systems => "AND entity_kind = 'software_system'",
        SoftwareGlobalKind::Apis => "AND entity_kind = 'api'",
        SoftwareGlobalKind::Resources => "AND entity_kind = 'resource'",
        SoftwareGlobalKind::Tests => "AND entity_kind = 'test_case'",
        SoftwareGlobalKind::Deployments => {
            "AND entity_kind IN ('deployment_unit', 'runtime_service')"
        }
        SoftwareGlobalKind::Releases => "AND entity_kind = 'release_artifact'",
        _ => "",
    }
}

fn entity_evidence_order(kind: SoftwareGlobalKind) -> &'static str {
    match kind {
        SoftwareGlobalKind::Apis => {
            "CASE source_kind
                 WHEN 'api_schema' THEN 0
                 WHEN 'code' THEN 1
                 WHEN 'documentation' THEN 2
                 ELSE 3
             END ASC,
             name ASC, occurrence_id ASC"
        }
        SoftwareGlobalKind::Resources => {
            "CASE namespace
                 WHEN 'kubernetes' THEN 0
                 WHEN 'terraform' THEN 1
                 WHEN 'compose' THEN 2
                 WHEN 'systemd' THEN 3
                 WHEN 'launchd' THEN 4
                 WHEN 'helm' THEN 5
                 ELSE 6
             END ASC,
             name ASC, occurrence_id ASC"
        }
        SoftwareGlobalKind::Deployments => {
            "CASE source_kind
                 WHEN 'service_definition' THEN 0
                 WHEN 'iac' THEN 1
                 WHEN 'runtime' THEN 2
                 ELSE 3
             END ASC,
             CASE entity_kind
                 WHEN 'deployment_unit' THEN 0
                 WHEN 'runtime_service' THEN 1
                 ELSE 2
             END ASC,
             name ASC, occurrence_id ASC"
        }
        _ => "entity_kind ASC, name ASC, occurrence_id ASC",
    }
}

fn statement_language_filter(filters: &[String]) -> String {
    if filters.is_empty() {
        return String::new();
    }
    let clauses = filters
        .iter()
        .map(|_| {
            "EXISTS (
                SELECT 1 FROM software_entities subject
                WHERE subject.source_scope = software_statements.source_scope
                  AND subject.entity_key = software_statements.subject_id
                  AND subject.language_id = ?
            )"
        })
        .collect::<Vec<_>>();
    format!("AND ({})", clauses.join(" OR "))
}

fn entity_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareEntity> {
    Ok(SoftwareEntity {
        occurrence_id: row.get(0)?,
        entity_key: row.get(1)?,
        repository_id: row.get(2)?,
        source_scope: row.get(3)?,
        entity_kind: parse_enum(row.get::<_, String>(4)?, SoftwareEntityKind::parse)?,
        name: row.get(5)?,
        namespace: row.get(6)?,
        source_kind: parse_enum(row.get::<_, String>(7)?, SoftwareSourceKind::parse)?,
        evidence_refs: parse_json(row.get::<_, String>(8)?)?,
        attributes: parse_json::<BTreeMap<String, String>>(row.get::<_, String>(9)?)?,
        created_graph_version: GraphVersion::new(row.get::<_, u64>(10)?),
    })
}

fn statement_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareStatement> {
    Ok(SoftwareStatement {
        statement_id: row.get(0)?,
        subject_id: row.get(1)?,
        predicate: parse_enum(row.get::<_, String>(2)?, SoftwarePredicate::parse)?,
        object_id: row.get(3)?,
        object_value: row.get(4)?,
        source_scope: row.get(5)?,
        source_kind: parse_enum(row.get::<_, String>(6)?, SoftwareSourceKind::parse)?,
        evidence_refs: parse_json(row.get::<_, String>(7)?)?,
        assertion_mode: parse_enum(row.get::<_, String>(8)?, SoftwareAssertionMode::parse)?,
        resolution_state: parse_enum(row.get::<_, String>(9)?, SoftwareStatementResolution::parse)?,
        valid_from: row.get(10)?,
        valid_to: row.get(11)?,
        observed_at: row.get(12)?,
        extractor_id: row.get(13)?,
        extractor_version: row.get(14)?,
        confidence_basis_points: row.get(15)?,
        fact_state: parse_enum(row.get::<_, String>(16)?, SoftwareFactState::parse)?,
    })
}

fn parse_enum<T>(value: String, parse: impl FnOnce(&str) -> Option<T>) -> rusqlite::Result<T> {
    parse(&value)
        .ok_or_else(|| conversion_error(format!("unknown software ontology value '{value}'")))
}

fn parse_json<T: serde::de::DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_str(&value)
        .map_err(|error| conversion_error(format!("invalid software ontology JSON: {error}")))
}

fn conversion_error(message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(io::Error::new(io::ErrorKind::InvalidData, message)),
    )
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
