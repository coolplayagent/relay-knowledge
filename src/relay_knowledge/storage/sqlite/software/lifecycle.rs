use std::{collections::BTreeMap, path::Path};

use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SoftwareBuildTarget, SoftwareBuildTargetInput,
        SoftwareDesignElement, SoftwareDesignElementInput, SoftwareGlobalRequest,
        SoftwareIacResource, SoftwareIacResourceInput,
    },
    storage::{StorageError, sqlite::maven},
};

const HIGH_CONFIDENCE: u16 = 9_000;
const MEDIUM_CONFIDENCE: u16 = 7_500;

#[path = "lifecycle_build.rs"]
mod build;
#[path = "lifecycle_design.rs"]
mod design;
#[path = "lifecycle_iac.rs"]
mod iac;

pub(super) struct LifecycleProjection {
    pub(super) build_targets: Vec<SoftwareBuildTarget>,
    pub(super) iac_resources: Vec<SoftwareIacResource>,
    pub(super) design_elements: Vec<SoftwareDesignElement>,
}

pub(super) struct IndexedDocument {
    repository_id: String,
    source_scope: String,
    pub(super) path: String,
    pub(super) language_id: String,
    pub(super) lines: Vec<IndexedLine>,
}

pub(super) struct IndexedLine {
    pub(super) number: u32,
    pub(super) text: String,
}

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS software_build_targets (
            target_id TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            ecosystem TEXT NOT NULL,
            language_id TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            command TEXT,
            output_hint TEXT,
            source_kind TEXT NOT NULL,
            evidence_path TEXT NOT NULL,
            evidence_line_start INTEGER NOT NULL,
            evidence_line_end INTEGER NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_build_targets_scope
            ON software_build_targets(source_scope, language_id, ecosystem, name);

        CREATE TABLE IF NOT EXISTS software_iac_resources (
            resource_id TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            language_id TEXT NOT NULL,
            provider TEXT NOT NULL,
            resource_kind TEXT NOT NULL,
            name TEXT NOT NULL,
            scope_hint TEXT,
            target_hint TEXT,
            resolution_state TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            evidence_path TEXT NOT NULL,
            evidence_line_start INTEGER NOT NULL,
            evidence_line_end INTEGER NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_iac_resources_scope
            ON software_iac_resources(source_scope, language_id, provider, resource_kind, name);

        CREATE TABLE IF NOT EXISTS software_design_elements (
            element_id TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            language_id TEXT NOT NULL,
            element_kind TEXT NOT NULL,
            name TEXT NOT NULL,
            parent TEXT,
            summary TEXT,
            source_kind TEXT NOT NULL,
            evidence_path TEXT NOT NULL,
            evidence_line_start INTEGER NOT NULL,
            evidence_line_end INTEGER NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_design_elements_scope
            ON software_design_elements(source_scope, language_id, element_kind, name);
        ",
    )?;

    Ok(())
}

pub(super) fn delete_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    if maven::preserves_existing_facts(connection, source_scope)? {
        connection.execute(
            "DELETE FROM software_build_targets WHERE source_scope = ?1 AND ecosystem != 'maven'",
            params![source_scope],
        )?;
    } else {
        connection.execute(
            "DELETE FROM software_build_targets WHERE source_scope = ?1",
            params![source_scope],
        )?;
    }
    connection.execute(
        "DELETE FROM software_iac_resources WHERE source_scope = ?1",
        params![source_scope],
    )?;
    connection.execute(
        "DELETE FROM software_design_elements WHERE source_scope = ?1",
        params![source_scope],
    )?;

    Ok(())
}

pub(super) fn refresh_projection(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<LifecycleProjection, StorageError> {
    let documents = indexed_documents(connection, source_scope)?;
    let mut build_targets = existing_maven_build_targets(connection, source_scope)?;
    let mut iac_resources = Vec::new();
    let mut design_elements = Vec::new();
    for document in &documents {
        build::collect(document, graph_version, &mut build_targets)?;
        iac::collect(document, graph_version, &mut iac_resources)?;
        design::collect(document, graph_version, &mut design_elements)?;
    }
    for input in maven::build_target_inputs(connection, source_scope, graph_version)? {
        push_build_target(&mut build_targets, input)?;
    }
    for target in &build_targets {
        insert_build_target(connection, target)?;
    }
    for resource in &iac_resources {
        insert_iac_resource(connection, resource)?;
    }
    for element in &design_elements {
        insert_design_element(connection, element)?;
    }

    Ok(LifecycleProjection {
        build_targets,
        iac_resources,
        design_elements,
    })
}

fn indexed_documents(
    connection: &Connection,
    source_scope: &str,
) -> Result<Vec<IndexedDocument>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT repository_id, source_scope, path, language_id, content, line_start
        FROM code_repository_chunks
        WHERE source_scope = ?1
        ORDER BY path ASC, line_start ASC, chunk_id ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, u32>(5)?,
        ))
    })?;
    let mut documents = BTreeMap::<String, IndexedDocument>::new();
    for row in rows {
        let (repository_id, source_scope, path, language_id, content, line_start) = row?;
        let document = documents
            .entry(path.clone())
            .or_insert_with(|| IndexedDocument {
                repository_id,
                source_scope,
                path,
                language_id,
                lines: Vec::new(),
            });
        for (offset, text) in content.lines().enumerate() {
            document.lines.push(IndexedLine {
                number: line_start.saturating_add(offset as u32),
                text: text.to_owned(),
            });
        }
    }

    Ok(documents.into_values().collect())
}

fn existing_maven_build_targets(
    connection: &Connection,
    source_scope: &str,
) -> Result<Vec<SoftwareBuildTarget>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT target_id, repository_id, source_scope, ecosystem, language_id, name,
               kind, command, output_hint, source_kind, evidence_path, evidence_line_start,
               evidence_line_end, confidence_basis_points, created_graph_version
        FROM software_build_targets
        WHERE source_scope = ?1
          AND ecosystem = 'maven'
        ORDER BY kind ASC, name ASC, evidence_path ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], build_target_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn build_targets_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareBuildTarget>, StorageError> {
    let path_filter =
        super::path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
    let language_filter =
        super::language_filter_sql_for_column("language_id", &request.repository.language_filters);
    let query = format!(
        "
        SELECT target_id, repository_id, source_scope, ecosystem, language_id, name,
               kind, command, output_hint, source_kind, evidence_path, evidence_line_start,
               evidence_line_end, confidence_basis_points, created_graph_version
        FROM software_build_targets
        WHERE source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY
            CASE kind
                WHEN 'script' THEN 0
                WHEN 'job' THEN 1
                WHEN 'executable' THEN 2
                WHEN 'library' THEN 3
                WHEN 'module' THEN 4
                WHEN 'package' THEN 5
                WHEN 'project' THEN 6
                WHEN 'feature' THEN 7
                ELSE 8
            END ASC,
            CASE name
                WHEN 'build' THEN 0
                WHEN 'verify' THEN 1
                WHEN 'test' THEN 2
                WHEN 'check' THEN 3
                ELSE 4
            END ASC,
            ecosystem ASC,
            name ASC,
            evidence_path ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), build_target_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn iac_resources_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareIacResource>, StorageError> {
    let path_filter =
        super::path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
    let language_filter =
        super::language_filter_sql_for_column("language_id", &request.repository.language_filters);
    let query = format!(
        "
        SELECT resource_id, repository_id, source_scope, language_id, provider,
               resource_kind, name, scope_hint, target_hint, resolution_state,
               source_kind, evidence_path, evidence_line_start, evidence_line_end,
               confidence_basis_points, created_graph_version
        FROM software_iac_resources
        WHERE source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY
            CASE provider
                WHEN 'kubernetes' THEN 0
                WHEN 'terraform' THEN 1
                WHEN 'compose' THEN 2
                WHEN 'systemd' THEN 3
                WHEN 'launchd' THEN 4
                WHEN 'helm' THEN 5
                WHEN 'github-actions' THEN 6
                WHEN 'gitlab-ci' THEN 7
                WHEN 'container' THEN 8
                ELSE 9
            END ASC,
            CASE lower(resource_kind)
                WHEN 'deployment' THEN 0
                WHEN 'statefulset' THEN 1
                WHEN 'daemonset' THEN 2
                WHEN 'service' THEN 3
                WHEN 'resource' THEN 4
                WHEN 'module' THEN 5
                WHEN 'base_image' THEN 6
                ELSE 7
            END ASC,
            confidence_basis_points DESC,
            name ASC,
            evidence_path ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), iac_resource_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn design_elements_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareDesignElement>, StorageError> {
    let path_filter =
        super::path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
    let language_filter =
        super::language_filter_sql_for_column("language_id", &request.repository.language_filters);
    let query = format!(
        "
        SELECT element_id, repository_id, source_scope, language_id, element_kind,
               name, parent, summary, source_kind, evidence_path, evidence_line_start,
               evidence_line_end, confidence_basis_points, created_graph_version
        FROM software_design_elements
        WHERE source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY element_kind ASC, name ASC, evidence_path ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), design_element_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn insert_build_target(
    connection: &Connection,
    target: &SoftwareBuildTarget,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO software_build_targets (
            target_id, repository_id, source_scope, ecosystem, language_id, name, kind,
            command, output_hint, source_kind, evidence_path, evidence_line_start,
            evidence_line_end, confidence_basis_points, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ",
        params![
            target.target_id,
            target.repository_id,
            target.source_scope,
            target.ecosystem,
            target.language_id,
            target.name,
            target.kind,
            target.command,
            target.output_hint,
            target.source_kind,
            target.evidence_path,
            target.evidence_line_range.start,
            target.evidence_line_range.end,
            target.confidence_basis_points,
            target.created_graph_version.get(),
        ],
    )?;
    Ok(())
}

fn insert_iac_resource(
    connection: &Connection,
    resource: &SoftwareIacResource,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO software_iac_resources (
            resource_id, repository_id, source_scope, language_id, provider, resource_kind,
            name, scope_hint, target_hint, resolution_state, source_kind, evidence_path,
            evidence_line_start, evidence_line_end, confidence_basis_points,
            created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ",
        params![
            resource.resource_id,
            resource.repository_id,
            resource.source_scope,
            resource.language_id,
            resource.provider,
            resource.resource_kind,
            resource.name,
            resource.scope_hint,
            resource.target_hint,
            resource.resolution_state,
            resource.source_kind,
            resource.evidence_path,
            resource.evidence_line_range.start,
            resource.evidence_line_range.end,
            resource.confidence_basis_points,
            resource.created_graph_version.get(),
        ],
    )?;
    Ok(())
}

fn insert_design_element(
    connection: &Connection,
    element: &SoftwareDesignElement,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO software_design_elements (
            element_id, repository_id, source_scope, language_id, element_kind, name,
            parent, summary, source_kind, evidence_path, evidence_line_start,
            evidence_line_end, confidence_basis_points, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
        ",
        params![
            element.element_id,
            element.repository_id,
            element.source_scope,
            element.language_id,
            element.element_kind,
            element.name,
            element.parent,
            element.summary,
            element.source_kind,
            element.evidence_path,
            element.evidence_line_range.start,
            element.evidence_line_range.end,
            element.confidence_basis_points,
            element.created_graph_version.get(),
        ],
    )?;
    Ok(())
}

fn build_target_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareBuildTarget> {
    Ok(SoftwareBuildTarget {
        target_id: row.get(0)?,
        repository_id: row.get(1)?,
        source_scope: row.get(2)?,
        ecosystem: row.get(3)?,
        language_id: row.get(4)?,
        name: row.get(5)?,
        kind: row.get(6)?,
        command: row.get(7)?,
        output_hint: row.get(8)?,
        source_kind: row.get(9)?,
        evidence_path: row.get(10)?,
        evidence_line_range: RepositoryCodeRange {
            start: row.get(11)?,
            end: row.get(12)?,
        },
        confidence_basis_points: row.get(13)?,
        created_graph_version: GraphVersion::new(row.get::<_, u64>(14)?),
    })
}

fn iac_resource_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareIacResource> {
    Ok(SoftwareIacResource {
        resource_id: row.get(0)?,
        repository_id: row.get(1)?,
        source_scope: row.get(2)?,
        language_id: row.get(3)?,
        provider: row.get(4)?,
        resource_kind: row.get(5)?,
        name: row.get(6)?,
        scope_hint: row.get(7)?,
        target_hint: row.get(8)?,
        resolution_state: row.get(9)?,
        source_kind: row.get(10)?,
        evidence_path: row.get(11)?,
        evidence_line_range: RepositoryCodeRange {
            start: row.get(12)?,
            end: row.get(13)?,
        },
        confidence_basis_points: row.get(14)?,
        created_graph_version: GraphVersion::new(row.get::<_, u64>(15)?),
    })
}

fn design_element_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareDesignElement> {
    Ok(SoftwareDesignElement {
        element_id: row.get(0)?,
        repository_id: row.get(1)?,
        source_scope: row.get(2)?,
        language_id: row.get(3)?,
        element_kind: row.get(4)?,
        name: row.get(5)?,
        parent: row.get(6)?,
        summary: row.get(7)?,
        source_kind: row.get(8)?,
        evidence_path: row.get(9)?,
        evidence_line_range: RepositoryCodeRange {
            start: row.get(10)?,
            end: row.get(11)?,
        },
        confidence_basis_points: row.get(12)?,
        created_graph_version: GraphVersion::new(row.get::<_, u64>(13)?),
    })
}

pub(super) fn push_build_target(
    targets: &mut Vec<SoftwareBuildTarget>,
    input: SoftwareBuildTargetInput,
) -> Result<(), StorageError> {
    let target = SoftwareBuildTarget::new(input)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    if !targets
        .iter()
        .any(|existing| existing.target_id == target.target_id)
    {
        targets.push(target);
    }
    Ok(())
}

pub(super) fn push_iac_resource(
    resources: &mut Vec<SoftwareIacResource>,
    input: SoftwareIacResourceInput,
) -> Result<(), StorageError> {
    let resource = SoftwareIacResource::new(input)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    if !resources
        .iter()
        .any(|existing| existing.resource_id == resource.resource_id)
    {
        resources.push(resource);
    }
    Ok(())
}

pub(super) fn push_design_element(
    elements: &mut Vec<SoftwareDesignElement>,
    input: SoftwareDesignElementInput,
) -> Result<(), StorageError> {
    let element = SoftwareDesignElement::new(input)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    if !elements
        .iter()
        .any(|existing| existing.element_id == element.element_id)
    {
        elements.push(element);
    }
    Ok(())
}

pub(super) fn build_input(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    ecosystem: &str,
    kind: &str,
    name: &str,
    source_kind: &str,
    line: &IndexedLine,
) -> SoftwareBuildTargetInput {
    SoftwareBuildTargetInput {
        repository_id: document.repository_id.clone(),
        source_scope: document.source_scope.clone(),
        ecosystem: ecosystem.to_owned(),
        language_id: document.language_id.clone(),
        name: clean_scalar(name),
        kind: kind.to_owned(),
        command: None,
        output_hint: None,
        source_kind: source_kind.to_owned(),
        evidence_path: document.path.clone(),
        evidence_line_range: line_range(line.number),
        confidence_basis_points: HIGH_CONFIDENCE,
        created_graph_version: graph_version,
    }
}

pub(super) fn iac_input(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    provider: &str,
    resource_kind: &str,
    name: &str,
    source_kind: &str,
    line: &IndexedLine,
) -> SoftwareIacResourceInput {
    SoftwareIacResourceInput {
        repository_id: document.repository_id.clone(),
        source_scope: document.source_scope.clone(),
        language_id: document.language_id.clone(),
        provider: provider.to_owned(),
        resource_kind: resource_kind.to_owned(),
        name: clean_scalar(name),
        scope_hint: None,
        target_hint: None,
        resolution_state: "extracted".to_owned(),
        source_kind: source_kind.to_owned(),
        evidence_path: document.path.clone(),
        evidence_line_range: line_range(line.number),
        confidence_basis_points: HIGH_CONFIDENCE,
        created_graph_version: graph_version,
    }
}

pub(super) fn design_input(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    element_kind: &str,
    name: &str,
    source_kind: &str,
    line: &IndexedLine,
) -> SoftwareDesignElementInput {
    SoftwareDesignElementInput {
        repository_id: document.repository_id.clone(),
        source_scope: document.source_scope.clone(),
        language_id: document.language_id.clone(),
        element_kind: element_kind.to_owned(),
        name: clean_scalar(name),
        parent: None,
        summary: None,
        source_kind: source_kind.to_owned(),
        evidence_path: document.path.clone(),
        evidence_line_range: line_range(line.number),
        confidence_basis_points: MEDIUM_CONFIDENCE,
        created_graph_version: graph_version,
    }
}

pub(super) fn line_range(line: u32) -> RepositoryCodeRange {
    RepositoryCodeRange {
        start: line,
        end: line,
    }
}

pub(super) fn file_name(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
}

pub(super) fn file_stem(path: &str) -> Option<String> {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
}

pub(super) fn key_value(line: &str, separator: char) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(separator)?;
    Some((key.trim(), value.trim()))
}

pub(super) fn toml_section(line: &str) -> Option<&str> {
    line.strip_prefix("[[")
        .and_then(|value| value.strip_suffix("]]"))
        .or_else(|| {
            line.strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
        })
        .map(str::trim)
}

pub(super) fn toml_value(line: &str, key: &str) -> Option<String> {
    let (candidate, value) = key_value(line, '=')?;
    (candidate == key).then(|| clean_scalar(value))
}

pub(super) fn yaml_value(line: &str, key: &str) -> Option<String> {
    let (candidate, value) = key_value(line, ':')?;
    (candidate == key && !value.is_empty()).then(|| clean_scalar(value))
}

pub(super) fn json_string_value(line: &str, key: &str) -> Option<String> {
    let (candidate, value) = json_string_pair(line)?;
    (candidate == key).then_some(value)
}

pub(super) fn json_string_pair(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim().trim_end_matches(',');
    let trimmed = trimmed.strip_prefix('"')?;
    let (key, rest) = trimmed.split_once('"')?;
    let value = rest.trim_start().strip_prefix(':')?.trim();
    Some((key.to_owned(), clean_scalar(value)))
}

pub(super) fn clean_scalar(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(',')
        .trim_end_matches(')')
        .trim_start_matches('(')
        .trim_matches('"')
        .trim_matches('\'')
        .to_owned()
}

pub(super) fn strip_comment(line: &str, marker: char) -> &str {
    line.split_once(marker).map_or(line, |(value, _)| value)
}

pub(super) fn first_call_arg(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?.trim();
    let rest = rest.trim_start_matches('(').trim();
    let token = rest
        .split([',', ')', ' ', '\t'])
        .find(|value| !value.trim().is_empty())?;
    Some(clean_scalar(token))
}

pub(super) fn gradle_plugin(line: &str) -> Option<String> {
    line.strip_prefix("id ")
        .or_else(|| line.strip_prefix("id("))
        .map(clean_scalar)
}

pub(super) fn terraform_block(line: &str, prefix: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix(prefix)?.trim();
    let mut quoted = rest.split('"').skip(1).step_by(2);
    let first = quoted.next()?.to_owned();
    let second = quoted.next().unwrap_or(&first).to_owned();
    Some((first, second))
}

pub(super) fn xml_string(line: &str) -> Option<String> {
    line.split_once("<string>")
        .and_then(|(_, rest)| rest.split_once("</string>"))
        .map(|(value, _)| value.trim().to_owned())
}

pub(super) fn indentation(line: &str) -> usize {
    line.chars().take_while(|value| *value == ' ').count()
}

pub(super) fn markdown_heading(line: &str) -> Option<String> {
    let hashes = line.chars().take_while(|value| *value == '#').count();
    if !(1..=4).contains(&hashes) {
        return None;
    }
    let title = line[hashes..].trim();
    (!title.is_empty()).then(|| title.to_owned())
}

fn design_heading_kind<'a>(title: &str, path: &str) -> Option<&'a str> {
    let lower = title.to_ascii_lowercase();
    if lower.contains("architecture") || lower.contains("design") {
        Some("architecture")
    } else if lower.contains("module") {
        Some("module")
    } else if lower.contains("component") {
        Some("component")
    } else if lower.contains("interface") || lower.contains("api") {
        Some("interface")
    } else if lower.contains("capability") || lower.contains("feature") {
        Some("capability")
    } else if path.to_ascii_lowercase().contains("readme") {
        Some("software_system")
    } else {
        None
    }
}

pub(super) fn next_markdown_summary(lines: &[IndexedLine]) -> Option<String> {
    lines
        .iter()
        .map(|line| line.text.trim())
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.chars().take(240).collect())
}
