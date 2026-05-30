use crate::{
    domain::{GraphVersion, SoftwareIacResource},
    storage::StorageError,
};

use super::{
    IndexedDocument, file_name, file_stem, iac_input, indentation, push_iac_resource,
    strip_comment, terraform_block, xml_string, yaml_value,
};

pub(super) fn collect(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    resources: &mut Vec<SoftwareIacResource>,
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
    resources: &mut Vec<SoftwareIacResource>,
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
    resources: &mut Vec<SoftwareIacResource>,
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
    resources: &mut Vec<SoftwareIacResource>,
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
    resources: &mut Vec<SoftwareIacResource>,
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
    resources: &mut Vec<SoftwareIacResource>,
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
    resources: &mut Vec<SoftwareIacResource>,
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
    resources: &mut Vec<SoftwareIacResource>,
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
    resources: &mut Vec<SoftwareIacResource>,
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
    resources: &mut Vec<SoftwareIacResource>,
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
