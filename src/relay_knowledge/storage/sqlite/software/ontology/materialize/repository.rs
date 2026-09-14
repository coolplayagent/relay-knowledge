use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::{
    domain::{
        SoftwareAssertionMode, SoftwareEntityKind, SoftwarePredicate, SoftwareSourceKind,
        SoftwareStatementResolution,
    },
    storage::StorageError,
};

use super::{OntologyBuilder, OntologyEntityCandidate, resolution_state, source_kind_for};

pub(super) fn collect_files(
    connection: &Connection,
    builder: &mut OntologyBuilder,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT software_file_id, path, language_id, file_role, parse_status
        FROM software_files
        WHERE source_scope = ?1
        ORDER BY path ASC
        ",
    )?;
    let rows = statement.query_map(params![builder.source_scope], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (projection_id, path, language_id, file_role, parse_status) = row?;
        builder.add_file(
            &projection_id,
            &path,
            &language_id,
            &file_role,
            &parse_status,
        )?;
    }
    Ok(())
}

pub(super) fn collect_components(
    connection: &Connection,
    builder: &mut OntologyBuilder,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT component_id, ecosystem, name, requirement, resolved_version,
               dependency_group, source_kind, relationship_state, language_id,
               evidence_path, evidence_line_start, evidence_line_end,
               confidence_basis_points
        FROM software_components
        WHERE source_scope = ?1
        ORDER BY ecosystem ASC, name ASC, evidence_path ASC, evidence_line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![builder.source_scope], |row| {
        Ok(ComponentRow {
            projection_id: row.get(0)?,
            ecosystem: row.get(1)?,
            name: row.get(2)?,
            requirement: row.get(3)?,
            resolved_version: row.get(4)?,
            dependency_group: row.get(5)?,
            source_kind: row.get(6)?,
            relationship_state: row.get(7)?,
            language_id: row.get(8)?,
            path: row.get(9)?,
            line_start: row.get(10)?,
            line_end: row.get(11)?,
            confidence: row.get(12)?,
        })
    })?;
    for row in rows {
        let row = row?;
        let evidence = builder.evidence(&row.path, row.line_start, row.line_end)?;
        let source_kind = source_kind_for(&row.source_kind, &row.path);
        let mut attributes = BTreeMap::new();
        attributes.insert("ecosystem".to_owned(), row.ecosystem.clone());
        attributes.insert("language_id".to_owned(), row.language_id);
        attributes.insert("dependency_group".to_owned(), row.dependency_group);
        attributes.insert(
            "relationship_state".to_owned(),
            row.relationship_state.clone(),
        );
        if let Some(requirement) = row.requirement {
            attributes.insert("requirement".to_owned(), requirement);
        }
        if let Some(version) = row.resolved_version {
            attributes.insert("resolved_version".to_owned(), version);
        }
        let entity_key = builder.add_entity(OntologyEntityCandidate {
            projection_id: Some(&row.projection_id),
            kind: SoftwareEntityKind::PackageComponent,
            name: row.name,
            namespace: Some(row.ecosystem),
            source_kind,
            evidence: evidence.clone(),
            attributes,
        })?;
        let subject = builder
            .file_key(&row.path)
            .unwrap_or_else(|| builder.snapshot_key.clone());
        let assertion_mode = if row.relationship_state == "locked" {
            SoftwareAssertionMode::Verified
        } else {
            SoftwareAssertionMode::Declared
        };
        builder.add_statement(
            subject,
            SoftwarePredicate::DependsOn,
            Some(entity_key),
            None,
            source_kind,
            evidence,
            assertion_mode,
            SoftwareStatementResolution::Resolved,
            row.confidence,
        )?;
    }
    Ok(())
}

pub(super) fn collect_sdk_usages(
    connection: &Connection,
    builder: &mut OntologyBuilder,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT usage_id, language_id, module, target_hint, resolution_state,
               evidence_path, evidence_line_start, evidence_line_end,
               confidence_basis_points
        FROM software_sdk_usages
        WHERE source_scope = ?1
        ORDER BY language_id ASC, module ASC, evidence_path ASC, evidence_line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![builder.source_scope], |row| {
        Ok(SdkRow {
            projection_id: row.get(0)?,
            language_id: row.get(1)?,
            module: row.get(2)?,
            target_hint: row.get(3)?,
            resolution_state: row.get(4)?,
            path: row.get(5)?,
            line_start: row.get(6)?,
            line_end: row.get(7)?,
            confidence: row.get(8)?,
        })
    })?;
    for row in rows {
        let row = row?;
        let evidence = builder.evidence(&row.path, row.line_start, row.line_end)?;
        let mut attributes = BTreeMap::new();
        attributes.insert("language_id".to_owned(), row.language_id.clone());
        attributes.insert("module".to_owned(), row.module.clone());
        attributes.insert("resolution_state".to_owned(), row.resolution_state.clone());
        if let Some(target_hint) = row.target_hint.as_ref() {
            attributes.insert("target_hint".to_owned(), target_hint.clone());
        }
        let name = row.target_hint.unwrap_or(row.module);
        let entity_key = builder.add_entity(OntologyEntityCandidate {
            projection_id: Some(&row.projection_id),
            kind: SoftwareEntityKind::Sdk,
            name,
            namespace: Some(row.language_id),
            source_kind: SoftwareSourceKind::Code,
            evidence: evidence.clone(),
            attributes,
        })?;
        let subject = builder
            .file_key(&row.path)
            .unwrap_or_else(|| builder.snapshot_key.clone());
        builder.add_statement(
            subject,
            SoftwarePredicate::ConsumesApi,
            Some(entity_key),
            None,
            SoftwareSourceKind::Code,
            evidence,
            SoftwareAssertionMode::Extracted,
            resolution_state(&row.resolution_state),
            row.confidence,
        )?;
    }
    Ok(())
}

pub(super) fn collect_topics(
    connection: &Connection,
    builder: &mut OntologyBuilder,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT topic_id, name, topic_kind, source_path, line_start, line_end
        FROM software_topics
        WHERE source_scope = ?1
        ORDER BY source_path ASC, line_start ASC, topic_id ASC
        ",
    )?;
    let rows = statement.query_map(params![builder.source_scope], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, u32>(4)?,
            row.get::<_, u32>(5)?,
        ))
    })?;
    for row in rows {
        let (projection_id, name, topic_kind, path, line_start, line_end) = row?;
        let evidence = builder.evidence(&path, line_start, line_end)?;
        let mut attributes = BTreeMap::new();
        attributes.insert("topic_kind".to_owned(), topic_kind);
        attributes.insert("language_id".to_owned(), "markdown".to_owned());
        let document_key = builder.add_entity(OntologyEntityCandidate {
            projection_id: Some(&projection_id),
            kind: SoftwareEntityKind::DocumentationUnit,
            name,
            namespace: Some(path.clone()),
            source_kind: SoftwareSourceKind::Documentation,
            evidence: evidence.clone(),
            attributes,
        })?;
        builder.add_statement(
            document_key.clone(),
            SoftwarePredicate::Documents,
            Some(builder.snapshot_key.clone()),
            None,
            SoftwareSourceKind::Documentation,
            evidence.clone(),
            SoftwareAssertionMode::Extracted,
            SoftwareStatementResolution::Resolved,
            10_000,
        )?;
        if let Some(file_key) = builder.file_key(&path) {
            builder.add_statement(
                document_key,
                SoftwarePredicate::DerivedFrom,
                Some(file_key),
                None,
                SoftwareSourceKind::Documentation,
                evidence,
                SoftwareAssertionMode::Extracted,
                SoftwareStatementResolution::Resolved,
                10_000,
            )?;
        }
    }
    Ok(())
}

struct ComponentRow {
    projection_id: String,
    ecosystem: String,
    name: String,
    requirement: Option<String>,
    resolved_version: Option<String>,
    dependency_group: String,
    source_kind: String,
    relationship_state: String,
    language_id: String,
    path: String,
    line_start: u32,
    line_end: u32,
    confidence: u16,
}

struct SdkRow {
    projection_id: String,
    language_id: String,
    module: String,
    target_hint: Option<String>,
    resolution_state: String,
    path: String,
    line_start: u32,
    line_end: u32,
    confidence: u16,
}
