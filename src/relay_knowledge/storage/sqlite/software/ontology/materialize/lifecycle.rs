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

pub(super) fn collect_build_targets(
    connection: &Connection,
    builder: &mut OntologyBuilder,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT target_id, ecosystem, language_id, name, kind, command, output_hint,
               source_kind, evidence_path, evidence_line_start, evidence_line_end,
               confidence_basis_points
        FROM software_build_targets
        WHERE source_scope = ?1
        ORDER BY evidence_path ASC, evidence_line_start ASC, target_id ASC
        ",
    )?;
    let rows = statement.query_map(params![builder.source_scope], |row| {
        Ok(BuildRow {
            projection_id: row.get(0)?,
            ecosystem: row.get(1)?,
            language_id: row.get(2)?,
            name: row.get(3)?,
            kind: row.get(4)?,
            command: row.get(5)?,
            output_hint: row.get(6)?,
            source_kind: row.get(7)?,
            path: row.get(8)?,
            line_start: row.get(9)?,
            line_end: row.get(10)?,
            confidence: row.get(11)?,
        })
    })?;
    for row in rows {
        let row = row?;
        let evidence = builder.evidence(&row.path, row.line_start, row.line_end)?;
        let source_kind = source_kind_for(&row.source_kind, &row.path);
        let entity_kind = if row.kind == "job" {
            SoftwareEntityKind::BuildJob
        } else {
            SoftwareEntityKind::BuildDefinition
        };
        let mut attributes = BTreeMap::new();
        attributes.insert("ecosystem".to_owned(), row.ecosystem.clone());
        attributes.insert("language_id".to_owned(), row.language_id);
        attributes.insert("build_kind".to_owned(), row.kind.clone());
        if let Some(command) = row.command {
            attributes.insert("command".to_owned(), command);
        }
        if let Some(output_hint) = row.output_hint.as_ref() {
            attributes.insert("output_hint".to_owned(), output_hint.clone());
        }
        let build_key = builder.add_entity(OntologyEntityCandidate {
            projection_id: Some(&row.projection_id),
            kind: entity_kind,
            name: row.name,
            namespace: Some(row.ecosystem.clone()),
            source_kind,
            evidence: evidence.clone(),
            attributes,
        })?;
        let parent = if entity_kind == SoftwareEntityKind::BuildJob {
            let mut pipeline_attributes = BTreeMap::new();
            pipeline_attributes.insert("language_id".to_owned(), "yaml".to_owned());
            let pipeline_key = builder.add_entity(OntologyEntityCandidate {
                projection_id: None,
                kind: SoftwareEntityKind::Pipeline,
                name: row.path.clone(),
                namespace: Some(row.ecosystem.clone()),
                source_kind: SoftwareSourceKind::Ci,
                evidence: evidence.clone(),
                attributes: pipeline_attributes,
            })?;
            builder.add_statement(
                builder.snapshot_key.clone(),
                SoftwarePredicate::Contains,
                Some(pipeline_key.clone()),
                None,
                SoftwareSourceKind::Ci,
                evidence.clone(),
                SoftwareAssertionMode::Declared,
                SoftwareStatementResolution::Resolved,
                row.confidence,
            )?;
            pipeline_key
        } else {
            builder.snapshot_key.clone()
        };
        builder.add_statement(
            parent,
            SoftwarePredicate::Contains,
            Some(build_key.clone()),
            None,
            source_kind,
            evidence.clone(),
            SoftwareAssertionMode::Declared,
            SoftwareStatementResolution::Resolved,
            row.confidence,
        )?;
        if let Some(output_hint) = row.output_hint {
            let mut artifact_attributes = BTreeMap::new();
            artifact_attributes.insert("ecosystem".to_owned(), row.ecosystem.clone());
            let artifact_key = builder.add_entity(OntologyEntityCandidate {
                projection_id: None,
                kind: SoftwareEntityKind::ReleaseArtifact,
                name: output_hint,
                namespace: Some(row.ecosystem),
                source_kind,
                evidence: evidence.clone(),
                attributes: artifact_attributes,
            })?;
            builder.add_statement(
                build_key,
                SoftwarePredicate::Builds,
                Some(artifact_key),
                None,
                source_kind,
                evidence,
                SoftwareAssertionMode::Declared,
                SoftwareStatementResolution::Resolved,
                row.confidence,
            )?;
        }
    }
    Ok(())
}

pub(super) fn collect_iac_resources(
    connection: &Connection,
    builder: &mut OntologyBuilder,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT resource_id, language_id, provider, resource_kind, name, scope_hint,
               target_hint, resolution_state, source_kind, evidence_path,
               evidence_line_start, evidence_line_end, confidence_basis_points
        FROM software_iac_resources
        WHERE source_scope = ?1
        ORDER BY evidence_path ASC, evidence_line_start ASC, resource_id ASC
        ",
    )?;
    let rows = statement.query_map(params![builder.source_scope], |row| {
        Ok(IacRow {
            projection_id: row.get(0)?,
            language_id: row.get(1)?,
            provider: row.get(2)?,
            resource_kind: row.get(3)?,
            name: row.get(4)?,
            scope_hint: row.get(5)?,
            target_hint: row.get(6)?,
            resolution_state: row.get(7)?,
            source_kind: row.get(8)?,
            path: row.get(9)?,
            line_start: row.get(10)?,
            line_end: row.get(11)?,
            confidence: row.get(12)?,
        })
    })?;
    for row in rows {
        collect_iac_row(builder, row?)?;
    }
    Ok(())
}

fn collect_iac_row(builder: &mut OntologyBuilder, row: IacRow) -> Result<(), StorageError> {
    let evidence = builder.evidence(&row.path, row.line_start, row.line_end)?;
    let source_kind = source_kind_for(&row.source_kind, &row.path);
    let resolution = resolution_state(&row.resolution_state);
    if matches!(row.provider.as_str(), "systemd" | "launchd") {
        let deployment_key = deployment_unit(builder, &row, source_kind, &evidence)?;
        let mut service_attributes = common_iac_attributes(&row);
        if let Some(target_hint) = row.target_hint.as_ref() {
            service_attributes.insert("target_hint".to_owned(), target_hint.clone());
        }
        let service_key = builder.add_entity(OntologyEntityCandidate {
            projection_id: Some(&row.projection_id),
            kind: SoftwareEntityKind::RuntimeService,
            name: row.name,
            namespace: Some(row.provider),
            source_kind,
            evidence: evidence.clone(),
            attributes: service_attributes,
        })?;
        builder.add_statement(
            deployment_key,
            SoftwarePredicate::RunsAs,
            Some(service_key),
            None,
            source_kind,
            evidence,
            SoftwareAssertionMode::Declared,
            resolution,
            row.confidence,
        )?;
        return Ok(());
    }

    let deployment_key = deployment_unit(builder, &row, source_kind, &evidence)?;
    let resource_key = builder.add_entity(OntologyEntityCandidate {
        projection_id: Some(&row.projection_id),
        kind: SoftwareEntityKind::Resource,
        name: row.name.clone(),
        namespace: Some(row.provider.clone()),
        source_kind,
        evidence: evidence.clone(),
        attributes: common_iac_attributes(&row),
    })?;
    builder.add_statement(
        deployment_key.clone(),
        SoftwarePredicate::Contains,
        Some(resource_key),
        None,
        source_kind,
        evidence.clone(),
        SoftwareAssertionMode::Declared,
        resolution,
        row.confidence,
    )?;
    if matches!(row.resource_kind.as_str(), "image" | "container_image")
        && let Some(target_hint) = row.target_hint
    {
        let artifact_key = builder.add_entity(OntologyEntityCandidate {
            projection_id: None,
            kind: SoftwareEntityKind::ReleaseArtifact,
            name: target_hint,
            namespace: Some("container".to_owned()),
            source_kind,
            evidence: evidence.clone(),
            attributes: BTreeMap::new(),
        })?;
        builder.add_statement(
            deployment_key,
            SoftwarePredicate::Deploys,
            Some(artifact_key),
            None,
            source_kind,
            evidence,
            SoftwareAssertionMode::Declared,
            resolution,
            row.confidence,
        )?;
    }
    Ok(())
}

fn deployment_unit(
    builder: &mut OntologyBuilder,
    row: &IacRow,
    source_kind: SoftwareSourceKind,
    evidence: &crate::domain::SoftwareEvidenceRef,
) -> Result<String, StorageError> {
    if let Some(key) = builder.deployment_by_path.get(&row.path) {
        return Ok(key.clone());
    }
    let mut attributes = BTreeMap::new();
    attributes.insert("provider".to_owned(), row.provider.clone());
    attributes.insert("language_id".to_owned(), row.language_id.clone());
    let key = builder.add_entity(OntologyEntityCandidate {
        projection_id: None,
        kind: SoftwareEntityKind::DeploymentUnit,
        name: row.path.clone(),
        namespace: Some(row.provider.clone()),
        source_kind,
        evidence: evidence.clone(),
        attributes,
    })?;
    builder
        .deployment_by_path
        .insert(row.path.clone(), key.clone());
    builder.add_statement(
        builder.snapshot_key.clone(),
        SoftwarePredicate::Contains,
        Some(key.clone()),
        None,
        source_kind,
        evidence.clone(),
        SoftwareAssertionMode::Declared,
        SoftwareStatementResolution::Resolved,
        row.confidence,
    )?;
    Ok(key)
}

fn common_iac_attributes(row: &IacRow) -> BTreeMap<String, String> {
    let mut attributes = BTreeMap::new();
    attributes.insert("provider".to_owned(), row.provider.clone());
    attributes.insert("resource_kind".to_owned(), row.resource_kind.clone());
    attributes.insert("language_id".to_owned(), row.language_id.clone());
    attributes.insert("resolution_state".to_owned(), row.resolution_state.clone());
    if let Some(scope_hint) = row.scope_hint.as_ref() {
        attributes.insert("scope_hint".to_owned(), scope_hint.clone());
    }
    if let Some(target_hint) = row.target_hint.as_ref() {
        attributes.insert("target_hint".to_owned(), target_hint.clone());
    }
    attributes
}

pub(super) fn collect_design_elements(
    connection: &Connection,
    builder: &mut OntologyBuilder,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT element_id, language_id, element_kind, name, parent, summary,
               source_kind, evidence_path, evidence_line_start, evidence_line_end,
               confidence_basis_points
        FROM software_design_elements
        WHERE source_scope = ?1
        ORDER BY evidence_path ASC, evidence_line_start ASC, element_id ASC
        ",
    )?;
    let rows = statement.query_map(params![builder.source_scope], |row| {
        Ok(DesignRow {
            projection_id: row.get(0)?,
            language_id: row.get(1)?,
            element_kind: row.get(2)?,
            name: row.get(3)?,
            parent: row.get(4)?,
            summary: row.get(5)?,
            source_kind: row.get(6)?,
            path: row.get(7)?,
            line_start: row.get(8)?,
            line_end: row.get(9)?,
            confidence: row.get(10)?,
        })
    })?;
    for row in rows {
        let row = row?;
        let evidence = builder.evidence(&row.path, row.line_start, row.line_end)?;
        let source_kind = source_kind_for(&row.source_kind, &row.path);
        let entity_kind = design_entity_kind(&row);
        let mut attributes = BTreeMap::new();
        attributes.insert("language_id".to_owned(), row.language_id);
        attributes.insert("design_kind".to_owned(), row.element_kind);
        if let Some(parent) = row.parent {
            attributes.insert("parent".to_owned(), parent);
        }
        if let Some(summary) = row.summary {
            attributes.insert("summary".to_owned(), summary);
        }
        let namespace = if entity_kind == SoftwareEntityKind::DocumentationUnit {
            Some(row.path.clone())
        } else {
            Some(row.source_kind.clone())
        };
        let entity_key = builder.add_entity(OntologyEntityCandidate {
            projection_id: Some(&row.projection_id),
            kind: entity_kind,
            name: row.name,
            namespace,
            source_kind,
            evidence: evidence.clone(),
            attributes,
        })?;
        let predicate = if entity_kind == SoftwareEntityKind::DocumentationUnit {
            SoftwarePredicate::Documents
        } else {
            SoftwarePredicate::Contains
        };
        let (subject, object) = if predicate == SoftwarePredicate::Documents {
            (entity_key, builder.snapshot_key.clone())
        } else {
            (builder.snapshot_key.clone(), entity_key)
        };
        builder.add_statement(
            subject,
            predicate,
            Some(object),
            None,
            source_kind,
            evidence,
            SoftwareAssertionMode::Declared,
            SoftwareStatementResolution::Resolved,
            row.confidence,
        )?;
    }
    Ok(())
}

fn design_entity_kind(row: &DesignRow) -> SoftwareEntityKind {
    if row.source_kind == "markdown-metadata" {
        return match row.element_kind.as_str() {
            "software_system" => SoftwareEntityKind::SoftwareSystem,
            "api" | "interface" => SoftwareEntityKind::Api,
            "component" | "module" => SoftwareEntityKind::Component,
            "resource" => SoftwareEntityKind::Resource,
            _ => SoftwareEntityKind::DocumentationUnit,
        };
    }
    if row.source_kind == "markdown" {
        SoftwareEntityKind::DocumentationUnit
    } else {
        SoftwareEntityKind::Component
    }
}

struct BuildRow {
    projection_id: String,
    ecosystem: String,
    language_id: String,
    name: String,
    kind: String,
    command: Option<String>,
    output_hint: Option<String>,
    source_kind: String,
    path: String,
    line_start: u32,
    line_end: u32,
    confidence: u16,
}

struct IacRow {
    projection_id: String,
    language_id: String,
    provider: String,
    resource_kind: String,
    name: String,
    scope_hint: Option<String>,
    target_hint: Option<String>,
    resolution_state: String,
    source_kind: String,
    path: String,
    line_start: u32,
    line_end: u32,
    confidence: u16,
}

struct DesignRow {
    projection_id: String,
    language_id: String,
    element_kind: String,
    name: String,
    parent: Option<String>,
    summary: Option<String>,
    source_kind: String,
    path: String,
    line_start: u32,
    line_end: u32,
    confidence: u16,
}
