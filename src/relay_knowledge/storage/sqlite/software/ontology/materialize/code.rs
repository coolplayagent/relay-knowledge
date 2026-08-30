use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::{
    domain::{
        SoftwareAssertionMode, SoftwareEntityKind, SoftwarePredicate, SoftwareSourceKind,
        SoftwareStatementResolution,
    },
    storage::StorageError,
};

use super::{OntologyBuilder, OntologyEntityCandidate, source_kind_for};

pub(super) fn collect_api_and_test_symbols(
    connection: &Connection,
    builder: &mut OntologyBuilder,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT symbol_snapshot_id, path, language_id, name, kind, line_start, line_end
        FROM code_repository_symbols
        WHERE source_scope = ?1
          AND (
              kind IN ('interface', 'trait', 'protocol')
              OR (
                  kind IN ('function', 'method')
                  AND (
                      path LIKE 'tests/%' OR path LIKE '%/tests/%'
                      OR path LIKE '%_test.%' OR path LIKE '%_tests.%'
                      OR path LIKE 'test/%' OR path LIKE '%/test/%'
                  )
                  AND (
                      lower(name) LIKE '%test%' OR lower(name) LIKE '%spec%'
                      OR lower(name) LIKE '%smoke%'
                  )
              )
          )
        ORDER BY path ASC, line_start ASC, symbol_snapshot_id ASC
        ",
    )?;
    let rows = statement.query_map(params![builder.source_scope], |row| {
        Ok(SymbolRow {
            projection_id: row.get(0)?,
            path: row.get(1)?,
            language_id: row.get(2)?,
            name: row.get(3)?,
            symbol_kind: row.get(4)?,
            line_start: row.get(5)?,
            line_end: row.get(6)?,
        })
    })?;
    for row in rows {
        let row = row?;
        let entity_kind = if matches!(row.symbol_kind.as_str(), "interface" | "trait" | "protocol")
        {
            SoftwareEntityKind::Api
        } else {
            SoftwareEntityKind::TestCase
        };
        let source_kind = if entity_kind == SoftwareEntityKind::TestCase {
            SoftwareSourceKind::Test
        } else {
            SoftwareSourceKind::Code
        };
        let evidence = builder.evidence(&row.path, row.line_start, row.line_end)?;
        let mut attributes = BTreeMap::new();
        attributes.insert("language_id".to_owned(), row.language_id);
        attributes.insert("symbol_kind".to_owned(), row.symbol_kind);
        let entity_key = builder.add_entity(OntologyEntityCandidate {
            projection_id: Some(&row.projection_id),
            kind: entity_kind,
            name: row.name,
            namespace: Some(row.path),
            source_kind,
            evidence: evidence.clone(),
            attributes,
        })?;
        if entity_kind == SoftwareEntityKind::TestCase {
            builder.add_statement(
                entity_key,
                SoftwarePredicate::Tests,
                Some(builder.snapshot_key.clone()),
                None,
                source_kind,
                evidence,
                SoftwareAssertionMode::Extracted,
                SoftwareStatementResolution::Resolved,
                9_000,
            )?;
        } else {
            builder.add_statement(
                builder.snapshot_key.clone(),
                SoftwarePredicate::Contains,
                Some(entity_key),
                None,
                source_kind,
                evidence,
                SoftwareAssertionMode::Extracted,
                SoftwareStatementResolution::Resolved,
                9_000,
            )?;
        }
    }
    Ok(())
}

pub(super) fn collect_configurations(
    connection: &Connection,
    builder: &mut OntologyBuilder,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT feature_flag_id, path, language_id, name, source_kind, source_key,
               edge_kind, confidence_basis_points, line_start, line_end
        FROM code_repository_feature_flags
        WHERE source_scope = ?1
        ORDER BY path ASC, line_start ASC, feature_flag_id ASC
        ",
    )?;
    let rows = statement.query_map(params![builder.source_scope], |row| {
        Ok(ConfigurationRow {
            projection_id: row.get(0)?,
            path: row.get(1)?,
            language_id: row.get(2)?,
            name: row.get(3)?,
            source_kind: row.get(4)?,
            source_key: row.get(5)?,
            edge_kind: row.get(6)?,
            confidence: row.get(7)?,
            line_start: row.get(8)?,
            line_end: row.get(9)?,
        })
    })?;
    for row in rows {
        let row = row?;
        let Some(file_key) = builder.file_key(&row.path) else {
            continue;
        };
        let evidence = builder.evidence(&row.path, row.line_start, row.line_end)?;
        let source_kind = source_kind_for(&row.source_kind, &row.path);
        let mut attributes = BTreeMap::new();
        attributes.insert("language_id".to_owned(), row.language_id);
        attributes.insert("source_key".to_owned(), row.source_key.clone());
        attributes.insert("edge_kind".to_owned(), row.edge_kind);
        attributes.insert("display_name".to_owned(), row.name);
        let configuration_key = builder.add_entity(OntologyEntityCandidate {
            projection_id: Some(&row.projection_id),
            kind: SoftwareEntityKind::Configuration,
            name: row.source_key,
            namespace: Some(row.path),
            source_kind,
            evidence: evidence.clone(),
            attributes,
        })?;
        builder.add_statement(
            configuration_key,
            SoftwarePredicate::Configures,
            Some(file_key),
            None,
            source_kind,
            evidence,
            SoftwareAssertionMode::Inferred,
            SoftwareStatementResolution::Resolved,
            row.confidence,
        )?;
    }
    Ok(())
}

struct SymbolRow {
    projection_id: String,
    path: String,
    language_id: String,
    name: String,
    symbol_kind: String,
    line_start: u32,
    line_end: u32,
}

struct ConfigurationRow {
    projection_id: String,
    path: String,
    language_id: String,
    name: String,
    source_kind: String,
    source_key: String,
    edge_kind: String,
    confidence: u16,
    line_start: u32,
    line_end: u32,
}
