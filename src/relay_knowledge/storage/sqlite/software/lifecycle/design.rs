//! Design element extraction from indexed repository documents.

use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SoftwareDesignElement, SoftwareDesignElementInput,
        SoftwareGlobalRequest,
    },
    storage::StorageError,
};

use super::{
    BoundedFacts,
    document::{IndexedDocument, IndexedLine},
    syntax::{
        clean_scalar, design_heading_kind, file_name, json_string_value, key_value,
        markdown_heading, next_markdown_summary, toml_value,
    },
};

const MEDIUM_CONFIDENCE: u16 = 7_500;
const MAX_DESIGN_ELEMENTS_PER_SCOPE: usize = 65_536;
type DesignElements = BoundedFacts<SoftwareDesignElement>;

pub(super) fn new_elements() -> DesignElements {
    DesignElements::new(MAX_DESIGN_ELEMENTS_PER_SCOPE, "design elements")
}

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
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
    connection.execute(
        "DELETE FROM software_design_elements WHERE source_scope = ?1",
        params![source_scope],
    )?;

    Ok(())
}

pub(super) fn persist(
    connection: &Connection,
    elements: &[SoftwareDesignElement],
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        INSERT OR REPLACE INTO software_design_elements (
            element_id, repository_id, source_scope, language_id, element_kind, name,
            parent, summary, source_kind, evidence_path, evidence_line_start,
            evidence_line_end, confidence_basis_points, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
        ",
    )?;
    for element in elements {
        statement.execute(params![
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
        ])?;
    }

    Ok(())
}

pub(in super::super) fn design_elements_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareDesignElement>, StorageError> {
    let path_filter =
        super::super::path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
    let language_filter = super::super::language_filter_sql_for_column(
        "language_id",
        &request.repository.language_filters,
    );
    let query = format!(
        "
        SELECT element_id, repository_id, source_scope, language_id, element_kind,
               name, parent, summary, source_kind, evidence_path, evidence_line_start,
               evidence_line_end, confidence_basis_points, created_graph_version
        FROM software_design_elements
        WHERE source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY
            CASE element_kind
                WHEN 'architecture' THEN 0
                WHEN 'capability' THEN 1
                WHEN 'module' THEN 2
                WHEN 'api' THEN 3
                WHEN 'software_system' THEN 4
                ELSE 5
            END ASC,
            confidence_basis_points DESC,
            name ASC,
            evidence_path ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), design_element_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
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

fn push_design_element(
    elements: &mut DesignElements,
    input: SoftwareDesignElementInput,
) -> Result<(), StorageError> {
    let element = SoftwareDesignElement::new(input)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    elements.insert(element.element_id.clone(), element)
}

fn design_input(
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
        evidence_line_range: RepositoryCodeRange {
            start: line.number,
            end: line.number,
        },
        confidence_basis_points: MEDIUM_CONFIDENCE,
        created_graph_version: graph_version,
    }
}

pub(super) fn collect(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    elements: &mut DesignElements,
) -> Result<(), StorageError> {
    let lower_path = document.path.to_ascii_lowercase();
    if lower_path.ends_with(".md") || lower_path.ends_with(".mdx") {
        collect_markdown(document, graph_version, elements)?;
    }
    match file_name(&document.path).as_deref() {
        Some("Cargo.toml") => collect_manifest(document, graph_version, "rust", elements)?,
        Some("package.json") => collect_manifest(document, graph_version, "npm", elements)?,
        Some("pyproject.toml") => collect_manifest(document, graph_version, "python", elements)?,
        Some("go.mod") => collect_manifest(document, graph_version, "go", elements)?,
        _ => {}
    }
    Ok(())
}

fn collect_markdown(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    elements: &mut DesignElements,
) -> Result<(), StorageError> {
    collect_markdown_metadata(document, graph_version, elements)?;
    for (index, line) in document.lines.iter().enumerate() {
        let trimmed = line.text.trim();
        let Some(title) = markdown_heading(trimmed) else {
            continue;
        };
        let Some(kind) = design_heading_kind(&title, &document.path) else {
            continue;
        };
        let mut input = design_input(document, graph_version, kind, &title, "markdown", line);
        input.summary = next_markdown_summary(&document.lines[index + 1..]);
        push_design_element(elements, input)?;
    }
    Ok(())
}

fn collect_markdown_metadata(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    elements: &mut DesignElements,
) -> Result<(), StorageError> {
    let Some(metadata_start) = document
        .lines
        .iter()
        .position(|line| !line.text.trim().is_empty())
    else {
        return Ok(());
    };
    if document.lines[metadata_start].text.trim() != "---" {
        return Ok(());
    }
    for line in document.lines.iter().skip(metadata_start + 1).take(64) {
        let trimmed = line.text.trim();
        if trimmed == "---" {
            break;
        }
        let Some((key, value)) = key_value(trimmed, ':') else {
            continue;
        };
        let element_kind = match key {
            "software-system" | "software_system" | "system" => "software_system",
            "component" => "component",
            "api" => "api",
            "resource" => "resource",
            _ => continue,
        };
        let name = clean_scalar(value);
        if name.is_empty() {
            continue;
        }
        push_design_element(
            elements,
            design_input(
                document,
                graph_version,
                element_kind,
                &name,
                "markdown-metadata",
                line,
            ),
        )?;
    }
    Ok(())
}

fn collect_manifest(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    ecosystem: &str,
    elements: &mut DesignElements,
) -> Result<(), StorageError> {
    for line in &document.lines {
        let trimmed = line.text.trim();
        let name = match ecosystem {
            "rust" | "python" => toml_value(trimmed, "name"),
            "npm" => json_string_value(trimmed, "name"),
            "go" => trimmed
                .strip_prefix("module ")
                .map(|value| value.trim().to_owned()),
            _ => None,
        };
        if let Some(name) = name {
            let mut input = design_input(document, graph_version, "module", &name, ecosystem, line);
            input.summary = Some(format!("{ecosystem} package/module boundary"));
            push_design_element(elements, input)?;
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "design_tests.rs"]
mod tests;
