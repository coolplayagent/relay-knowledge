//! Build target extraction from indexed repository documents.

use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SoftwareBuildTarget, SoftwareBuildTargetInput,
        SoftwareGlobalRequest,
    },
    storage::{StorageError, sqlite::maven},
};

use super::{
    BoundedFacts,
    document::IndexedDocument,
    syntax::{
        clean_scalar, file_name, first_call_arg, gradle_plugin, indentation, json_string_pair,
        json_string_value, key_value, strip_comment, toml_section, toml_value,
    },
};

const HIGH_CONFIDENCE: u16 = 9_000;
const MAX_BUILD_TARGETS_PER_SCOPE: usize = 65_536;
type BuildTargets = BoundedFacts<SoftwareBuildTarget>;

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

    Ok(())
}

pub(super) fn begin_refresh(
    connection: &Connection,
    source_scope: &str,
) -> Result<BuildTargets, StorageError> {
    let mut targets = BuildTargets::new(MAX_BUILD_TARGETS_PER_SCOPE, "build targets");
    for target in
        existing_maven_build_targets(connection, source_scope, MAX_BUILD_TARGETS_PER_SCOPE)?
    {
        targets.insert(target.target_id.clone(), target)?;
    }
    Ok(targets)
}

fn existing_maven_build_targets(
    connection: &Connection,
    source_scope: &str,
    limit: usize,
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
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(
        params![source_scope, limit.saturating_add(1) as i64],
        build_target_from_row,
    )?;

    let targets = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    if targets.len() > limit {
        return Err(StorageError::CapacityExceeded(format!(
            "existing Maven build targets exceed the bounded limit {limit}"
        )));
    }
    Ok(targets)
}

pub(in super::super) fn build_targets_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareBuildTarget>, StorageError> {
    let path_filter =
        super::super::path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
    let language_filter = super::super::language_filter_sql_for_column(
        "language_id",
        &request.repository.language_filters,
    );
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
    super::super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), build_target_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn persist(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    targets: &mut BuildTargets,
) -> Result<(), StorageError> {
    maven::visit_build_target_inputs(connection, source_scope, graph_version, |input| {
        push_build_target(targets, input)
    })?;
    let mut statement = connection.prepare(
        "
        INSERT OR REPLACE INTO software_build_targets (
            target_id, repository_id, source_scope, ecosystem, language_id, name, kind,
            command, output_hint, source_kind, evidence_path, evidence_line_start,
            evidence_line_end, confidence_basis_points, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ",
    )?;
    for target in targets.as_slice() {
        statement.execute(params![
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
        ])?;
    }
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

fn push_build_target(
    targets: &mut BuildTargets,
    mut input: SoftwareBuildTargetInput,
) -> Result<(), StorageError> {
    input.command = non_empty_optional(input.command);
    input.output_hint = non_empty_optional(input.output_hint);
    let target = SoftwareBuildTarget::new(input)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    targets.insert(target.target_id.clone(), target)
}

fn non_empty_optional(value: Option<String>) -> Option<String> {
    value
        .map(|text| clean_scalar(&text))
        .filter(|text| !text.is_empty())
}

fn build_input(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    ecosystem: &str,
    kind: &str,
    name: &str,
    source_kind: &str,
    line: &super::document::IndexedLine,
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
    targets: &mut BuildTargets,
) -> Result<(), StorageError> {
    let file_name = file_name(&document.path);
    match file_name.as_deref() {
        Some("Cargo.toml") => collect_cargo(document, graph_version, targets),
        Some("package.json") => collect_package_json(document, graph_version, targets),
        Some("pyproject.toml") => collect_pyproject(document, graph_version, targets),
        Some("go.mod") => collect_go_mod(document, graph_version, targets),
        Some("CMakeLists.txt") => collect_cmake(document, graph_version, targets),
        Some("Makefile") | Some("makefile") | Some("GNUmakefile") => {
            collect_makefile(document, graph_version, targets)
        }
        Some("build.gradle") | Some("build.gradle.kts") => {
            collect_gradle(document, graph_version, targets)
        }
        Some(name)
            if name == "Dockerfile"
                || name == "Containerfile"
                || name.starts_with("Dockerfile.")
                || name.starts_with("Containerfile.") =>
        {
            collect_dockerfile(document, graph_version, targets)
        }
        Some(".gitlab-ci.yml") | Some(".gitlab-ci.yaml") => {
            collect_ci_jobs(document, graph_version, "gitlab-ci", targets)
        }
        _ if document.path.starts_with(".github/workflows/") => {
            collect_ci_jobs(document, graph_version, "github-actions", targets)
        }
        _ => Ok(()),
    }
}

fn collect_dockerfile(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    targets: &mut BuildTargets,
) -> Result<(), StorageError> {
    let Some(evidence_line) = document
        .lines
        .iter()
        .find(|line| !line.text.trim().is_empty())
    else {
        return Ok(());
    };
    let mut input = build_input(
        document,
        graph_version,
        "container",
        "definition",
        &document.path,
        "Dockerfile",
        evidence_line,
    );
    input.command = document
        .lines
        .iter()
        .find_map(|line| line.text.trim().strip_prefix("FROM ").map(str::trim))
        .map(|image| format!("FROM {image}"));
    push_build_target(targets, input)
}

fn collect_cargo(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    targets: &mut BuildTargets,
) -> Result<(), StorageError> {
    let mut section = "";
    for line in &document.lines {
        let trimmed = strip_comment(&line.text, '#').trim();
        if let Some(next) = toml_section(trimmed) {
            section = next;
            continue;
        }
        if matches!(section, "package" | "lib" | "bin")
            && let Some(name) = toml_value(trimmed, "name")
        {
            let kind = match section {
                "lib" => "library",
                "bin" => "binary",
                _ => "package",
            };
            push_build_target(
                targets,
                build_input(
                    document,
                    graph_version,
                    "rust",
                    kind,
                    &name,
                    "Cargo.toml",
                    line,
                ),
            )?;
        }
        if section == "features"
            && let Some((name, _)) = key_value(trimmed, '=')
        {
            push_build_target(
                targets,
                build_input(
                    document,
                    graph_version,
                    "rust",
                    "feature",
                    name,
                    "Cargo.toml",
                    line,
                ),
            )?;
        }
    }
    Ok(())
}

fn collect_package_json(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    targets: &mut BuildTargets,
) -> Result<(), StorageError> {
    let mut in_scripts = false;
    for line in &document.lines {
        let trimmed = line.text.trim();
        if let Some(name) = json_string_value(trimmed, "name") {
            push_build_target(
                targets,
                build_input(
                    document,
                    graph_version,
                    "npm",
                    "package",
                    &name,
                    "package.json",
                    line,
                ),
            )?;
        }
        if trimmed.starts_with("\"scripts\"") && trimmed.contains('{') {
            in_scripts = !trimmed.contains('}');
            continue;
        }
        if in_scripts && trimmed.starts_with('}') {
            in_scripts = false;
            continue;
        }
        if in_scripts && let Some((name, command)) = json_string_pair(trimmed) {
            if command.is_empty() {
                continue;
            }
            let mut input = build_input(
                document,
                graph_version,
                "npm",
                "script",
                &name,
                "package.json",
                line,
            );
            input.command = Some(command);
            push_build_target(targets, input)?;
        }
    }
    Ok(())
}

fn collect_pyproject(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    targets: &mut BuildTargets,
) -> Result<(), StorageError> {
    let mut section = "";
    for line in &document.lines {
        let trimmed = strip_comment(&line.text, '#').trim();
        if let Some(next) = toml_section(trimmed) {
            section = next;
            continue;
        }
        if matches!(section, "project" | "tool.poetry")
            && let Some(name) = toml_value(trimmed, "name")
        {
            push_build_target(
                targets,
                build_input(
                    document,
                    graph_version,
                    "python",
                    "package",
                    &name,
                    "pyproject.toml",
                    line,
                ),
            )?;
        }
        if matches!(section, "project.scripts" | "tool.poetry.scripts")
            && let Some((name, command)) = key_value(trimmed, '=')
        {
            let command = clean_scalar(command);
            if command.is_empty() {
                continue;
            }
            let mut input = build_input(
                document,
                graph_version,
                "python",
                "script",
                name,
                "pyproject.toml",
                line,
            );
            input.command = Some(command);
            push_build_target(targets, input)?;
        }
    }
    Ok(())
}

fn collect_go_mod(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    targets: &mut BuildTargets,
) -> Result<(), StorageError> {
    for line in &document.lines {
        if let Some(module) = line.text.trim().strip_prefix("module ").map(str::trim) {
            push_build_target(
                targets,
                build_input(
                    document,
                    graph_version,
                    "go",
                    "module",
                    module,
                    "go.mod",
                    line,
                ),
            )?;
        }
    }
    Ok(())
}

fn collect_cmake(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    targets: &mut BuildTargets,
) -> Result<(), StorageError> {
    for line in &document.lines {
        let trimmed = strip_comment(&line.text, '#').trim();
        for (prefix, kind) in [
            ("project(", "project"),
            ("add_executable(", "executable"),
            ("add_library(", "library"),
        ] {
            if let Some(name) = first_call_arg(trimmed, prefix) {
                push_build_target(
                    targets,
                    build_input(
                        document,
                        graph_version,
                        "cmake",
                        kind,
                        &name,
                        "CMakeLists.txt",
                        line,
                    ),
                )?;
            }
        }
    }
    Ok(())
}

fn collect_makefile(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    targets: &mut BuildTargets,
) -> Result<(), StorageError> {
    for line in &document.lines {
        let trimmed = strip_comment(&line.text, '#').trim();
        if trimmed.starts_with('.') || trimmed.starts_with('\t') || trimmed.contains('=') {
            continue;
        }
        let Some((target, _)) = key_value(trimmed, ':') else {
            continue;
        };
        if target.is_empty() || target.contains('%') || target.contains(' ') {
            continue;
        }
        push_build_target(
            targets,
            build_input(
                document,
                graph_version,
                "make",
                "target",
                target,
                "Makefile",
                line,
            ),
        )?;
    }
    Ok(())
}

fn collect_gradle(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    targets: &mut BuildTargets,
) -> Result<(), StorageError> {
    for line in &document.lines {
        let trimmed = line
            .text
            .split_once("//")
            .map_or(line.text.as_str(), |(value, _)| value)
            .trim();
        if let Some(name) = first_call_arg(trimmed, "rootProject.name =") {
            push_build_target(
                targets,
                build_input(
                    document,
                    graph_version,
                    "gradle",
                    "project",
                    &name,
                    "gradle",
                    line,
                ),
            )?;
        } else if let Some(name) = first_call_arg(trimmed, "tasks.register(") {
            push_build_target(
                targets,
                build_input(
                    document,
                    graph_version,
                    "gradle",
                    "task",
                    &name,
                    "gradle",
                    line,
                ),
            )?;
        } else if let Some(plugin) = gradle_plugin(trimmed) {
            push_build_target(
                targets,
                build_input(
                    document,
                    graph_version,
                    "gradle",
                    "plugin",
                    &plugin,
                    "gradle",
                    line,
                ),
            )?;
        }
    }
    Ok(())
}

fn collect_ci_jobs(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    source_kind: &str,
    targets: &mut BuildTargets,
) -> Result<(), StorageError> {
    let mut in_jobs = false;
    for line in &document.lines {
        let trimmed = strip_comment(&line.text, '#').trim();
        if trimmed == "jobs:" || trimmed == "stages:" {
            in_jobs = true;
            continue;
        }
        if in_jobs && !line.text.starts_with(' ') && trimmed.ends_with(':') {
            in_jobs = false;
        }
        if in_jobs
            && indentation(&line.text) == 2
            && let Some(name) = trimmed.strip_suffix(':')
            && !name.starts_with('-')
        {
            push_build_target(
                targets,
                build_input(
                    document,
                    graph_version,
                    source_kind,
                    "job",
                    name,
                    source_kind,
                    line,
                ),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
