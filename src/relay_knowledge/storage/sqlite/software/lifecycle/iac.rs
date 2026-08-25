//! Infrastructure-as-code resource extraction from indexed repository documents.

use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SoftwareGlobalRequest, SoftwareIacResource,
        SoftwareIacResourceInput,
    },
    storage::StorageError,
};

use super::{
    BoundedFacts,
    document::{IndexedDocument, IndexedLine},
    syntax::{
        clean_scalar, file_name, file_stem, indentation, strip_comment, terraform_block,
        xml_string, yaml_value,
    },
};

const HIGH_CONFIDENCE: u16 = 9_000;
const MAX_IAC_RESOURCES_PER_SCOPE: usize = 65_536;
type IacResources = BoundedFacts<SoftwareIacResource>;

pub(super) fn new_resources() -> IacResources {
    IacResources::new(MAX_IAC_RESOURCES_PER_SCOPE, "IaC resources")
}

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
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
        ",
    )?;

    Ok(())
}

pub(super) fn delete_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "DELETE FROM software_iac_resources WHERE source_scope = ?1",
        params![source_scope],
    )?;

    Ok(())
}

pub(super) fn persist(
    connection: &Connection,
    resources: &[SoftwareIacResource],
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        INSERT OR REPLACE INTO software_iac_resources (
            resource_id, repository_id, source_scope, language_id, provider, resource_kind,
            name, scope_hint, target_hint, resolution_state, source_kind, evidence_path,
            evidence_line_start, evidence_line_end, confidence_basis_points,
            created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ",
    )?;
    for resource in resources {
        statement.execute(params![
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
        ])?;
    }

    Ok(())
}

pub(in super::super) fn iac_resources_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareIacResource>, StorageError> {
    let path_filter =
        super::super::path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
    let language_filter = super::super::language_filter_sql_for_column(
        "language_id",
        &request.repository.language_filters,
    );
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
    super::super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), iac_resource_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
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

fn push_iac_resource(
    resources: &mut IacResources,
    input: SoftwareIacResourceInput,
) -> Result<(), StorageError> {
    let resource = SoftwareIacResource::new(input)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    resources.insert(resource.resource_id.clone(), resource)
}

fn iac_input(
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
        evidence_line_range: RepositoryCodeRange {
            start: line.number,
            end: line.number,
        },
        confidence_basis_points: HIGH_CONFIDENCE,
        created_graph_version: graph_version,
    }
}

pub(super) fn collect(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    resources: &mut IacResources,
) -> Result<(), StorageError> {
    let file_name = file_name(&document.path);
    let lower_path = document.path.to_ascii_lowercase();
    if file_name
        .as_deref()
        .is_some_and(|name| name == "Dockerfile" || name == "Containerfile")
        || file_name.as_deref().is_some_and(|name| {
            name.starts_with("Dockerfile.") || name.starts_with("Containerfile.")
        })
    {
        collect_dockerfile(document, graph_version, resources)?;
    } else if lower_path.ends_with(".tf") {
        collect_terraform(document, graph_version, resources)?;
    } else if lower_path.ends_with(".service") {
        collect_systemd(document, graph_version, resources)?;
    } else if document.language_id == "yaml"
        || lower_path.ends_with(".yml")
        || lower_path.ends_with(".yaml")
    {
        collect_yaml(document, graph_version, resources)?;
    } else if lower_path.ends_with(".plist") {
        collect_launchd(document, graph_version, resources)?;
    }
    Ok(())
}

fn collect_dockerfile(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    resources: &mut IacResources,
) -> Result<(), StorageError> {
    for line in &document.lines {
        let trimmed = strip_comment(&line.text, '#').trim();
        if let Some(image) = trimmed.strip_prefix("FROM ").map(str::trim) {
            let image = image.split_whitespace().next().unwrap_or(image);
            let mut input = iac_input(
                document,
                graph_version,
                "container",
                "base_image",
                image,
                "Dockerfile",
                line,
            );
            input.target_hint = Some(image.to_owned());
            push_iac_resource(resources, input)?;
        } else if let Some(port) = trimmed.strip_prefix("EXPOSE ").map(str::trim) {
            push_iac_resource(
                resources,
                iac_input(
                    document,
                    graph_version,
                    "container",
                    "port",
                    port,
                    "Dockerfile",
                    line,
                ),
            )?;
        }
    }
    Ok(())
}

fn collect_terraform(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    resources: &mut IacResources,
) -> Result<(), StorageError> {
    for line in &document.lines {
        let trimmed = strip_comment(&line.text, '#').trim();
        for (prefix, kind) in [
            ("resource ", "resource"),
            ("module ", "module"),
            ("provider ", "provider"),
        ] {
            if let Some((resource_kind, name)) = terraform_block(trimmed, prefix) {
                let mut input = iac_input(
                    document,
                    graph_version,
                    "terraform",
                    kind,
                    &name,
                    "terraform",
                    line,
                );
                input.scope_hint = Some(resource_kind);
                push_iac_resource(resources, input)?;
            }
        }
    }
    Ok(())
}

fn collect_systemd(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    resources: &mut IacResources,
) -> Result<(), StorageError> {
    let name = file_stem(&document.path).unwrap_or_else(|| document.path.clone());
    for line in &document.lines {
        if let Some(command) = line.text.trim().strip_prefix("ExecStart=") {
            let mut input = iac_input(
                document,
                graph_version,
                "systemd",
                "service",
                &name,
                "systemd",
                line,
            );
            input.target_hint = Some(command.trim().to_owned());
            push_iac_resource(resources, input)?;
        }
    }
    Ok(())
}

fn collect_launchd(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    resources: &mut IacResources,
) -> Result<(), StorageError> {
    for (index, line) in document.lines.iter().enumerate() {
        if line.text.contains("<key>Label</key>")
            && let Some(next) = document.lines.get(index + 1)
            && let Some(label) = xml_string(&next.text)
        {
            push_iac_resource(
                resources,
                iac_input(
                    document,
                    graph_version,
                    "launchd",
                    "service",
                    &label,
                    "launchd",
                    next,
                ),
            )?;
        }
    }
    Ok(())
}

fn collect_yaml(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    resources: &mut IacResources,
) -> Result<(), StorageError> {
    let file_name = file_name(&document.path).unwrap_or_default();
    if matches!(
        file_name.as_str(),
        "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml"
    ) {
        collect_compose(document, graph_version, resources)?;
    }
    collect_kubernetes(document, graph_version, resources)?;
    if file_name == "Chart.yaml" {
        collect_helm(document, graph_version, resources)?;
    }
    if document.path.starts_with(".github/workflows/") {
        collect_workflow(document, graph_version, "github-actions", resources)?;
    }
    if matches!(file_name.as_str(), ".gitlab-ci.yml" | ".gitlab-ci.yaml") {
        collect_workflow(document, graph_version, "gitlab-ci", resources)?;
    }
    Ok(())
}

fn collect_compose(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    resources: &mut IacResources,
) -> Result<(), StorageError> {
    let mut in_services = false;
    let mut current_service = None::<String>;
    for line in &document.lines {
        let trimmed = strip_comment(&line.text, '#').trim();
        if trimmed == "services:" {
            in_services = true;
            continue;
        }
        if in_services && indentation(&line.text) == 2 && trimmed.ends_with(':') {
            current_service = Some(trimmed.trim_end_matches(':').to_owned());
            push_iac_resource(
                resources,
                iac_input(
                    document,
                    graph_version,
                    "compose",
                    "service",
                    current_service.as_deref().unwrap_or("service"),
                    "compose",
                    line,
                ),
            )?;
        }
        if let Some(service) = current_service.as_deref()
            && let Some(image) = yaml_value(trimmed, "image")
        {
            let mut input = iac_input(
                document,
                graph_version,
                "compose",
                "image",
                service,
                "compose",
                line,
            );
            input.target_hint = Some(image);
            push_iac_resource(resources, input)?;
        }
    }
    Ok(())
}

fn collect_kubernetes(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    resources: &mut IacResources,
) -> Result<(), StorageError> {
    let mut kind = None::<String>;
    let mut in_metadata = false;
    for line in &document.lines {
        let trimmed = strip_comment(&line.text, '#').trim();
        if let Some(value) = yaml_value(trimmed, "kind") {
            kind = Some(value);
            continue;
        }
        if trimmed == "metadata:" {
            in_metadata = true;
            continue;
        }
        if in_metadata && indentation(&line.text) == 0 && !trimmed.is_empty() {
            in_metadata = false;
        }
        if in_metadata
            && let Some(name) = yaml_value(trimmed, "name")
            && let Some(resource_kind) = kind.take()
        {
            let mut input = iac_input(
                document,
                graph_version,
                "kubernetes",
                &resource_kind,
                &name,
                "kubernetes-yaml",
                line,
            );
            input.scope_hint = Some(resource_kind);
            push_iac_resource(resources, input)?;
        }
    }
    Ok(())
}

fn collect_helm(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    resources: &mut IacResources,
) -> Result<(), StorageError> {
    for line in &document.lines {
        let trimmed = strip_comment(&line.text, '#').trim();
        if let Some(name) = yaml_value(trimmed, "name") {
            push_iac_resource(
                resources,
                iac_input(
                    document,
                    graph_version,
                    "helm",
                    "chart",
                    &name,
                    "Chart.yaml",
                    line,
                ),
            )?;
            break;
        }
    }
    Ok(())
}

fn collect_workflow(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    provider: &str,
    resources: &mut IacResources,
) -> Result<(), StorageError> {
    let mut in_jobs = false;
    for line in &document.lines {
        let trimmed = strip_comment(&line.text, '#').trim();
        if trimmed == "jobs:" {
            in_jobs = true;
            continue;
        }
        if in_jobs
            && indentation(&line.text) == 2
            && let Some(name) = trimmed.strip_suffix(':')
        {
            push_iac_resource(
                resources,
                iac_input(
                    document,
                    graph_version,
                    provider,
                    "job",
                    name,
                    provider,
                    line,
                ),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "iac_tests.rs"]
mod tests;
