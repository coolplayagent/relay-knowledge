use crate::{
    domain::{GraphVersion, SoftwareBuildTarget},
    storage::StorageError,
};

use super::{
    IndexedDocument, build_input, clean_scalar, file_name, first_call_arg, gradle_plugin,
    indentation, json_string_pair, json_string_value, key_value, push_build_target, strip_comment,
    toml_section, toml_value,
};

pub(super) fn collect(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    targets: &mut Vec<SoftwareBuildTarget>,
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
        Some(".gitlab-ci.yml") | Some(".gitlab-ci.yaml") => {
            collect_ci_jobs(document, graph_version, "gitlab-ci", targets)
        }
        _ if document.path.starts_with(".github/workflows/") => {
            collect_ci_jobs(document, graph_version, "github-actions", targets)
        }
        _ => Ok(()),
    }
}

fn collect_cargo(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    targets: &mut Vec<SoftwareBuildTarget>,
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
    targets: &mut Vec<SoftwareBuildTarget>,
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
    targets: &mut Vec<SoftwareBuildTarget>,
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
    targets: &mut Vec<SoftwareBuildTarget>,
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
    targets: &mut Vec<SoftwareBuildTarget>,
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
    targets: &mut Vec<SoftwareBuildTarget>,
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
    targets: &mut Vec<SoftwareBuildTarget>,
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
    targets: &mut Vec<SoftwareBuildTarget>,
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
