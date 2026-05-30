use crate::{
    domain::{GraphVersion, SoftwareDesignElement},
    storage::StorageError,
};

use super::{
    IndexedDocument, design_heading_kind, design_input, file_name, json_string_value,
    markdown_heading, next_markdown_summary, push_design_element, toml_value,
};

pub(super) fn collect(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    elements: &mut Vec<SoftwareDesignElement>,
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
    elements: &mut Vec<SoftwareDesignElement>,
) -> Result<(), StorageError> {
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

fn collect_manifest(
    document: &IndexedDocument,
    graph_version: GraphVersion,
    ecosystem: &str,
    elements: &mut Vec<SoftwareDesignElement>,
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
