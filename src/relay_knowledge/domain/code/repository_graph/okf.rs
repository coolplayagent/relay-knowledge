use std::{collections::BTreeMap, path::Path};

use super::{
    IndexedRepositoryDocument, RepositoryGraphEdge, RepositoryGraphNode, normalize_repository_path,
    path_is_within,
};

const MAX_LABEL_CHARS: usize = 256;
const MAX_RESOURCE_BYTES: usize = 4_096;

#[derive(Debug, Clone)]
pub(super) struct OkfConcept {
    pub path: String,
    pub title: String,
    pub details: BTreeMap<String, String>,
    pub sources: Vec<OkfSource>,
    pub links: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct OkfSource {
    id: String,
    resource: String,
    title: String,
}

impl OkfConcept {
    pub fn node(&self) -> RepositoryGraphNode {
        RepositoryGraphNode {
            id: concept_node_id(&self.path),
            kind: "okf_concept".to_owned(),
            label: truncate_label(&self.title),
            path: Some(self.path.clone()),
            resource: None,
            details: self.details.clone(),
        }
    }
}

impl OkfSource {
    pub fn node(&self) -> RepositoryGraphNode {
        RepositoryGraphNode {
            id: source_node_id(&self.resource),
            kind: "external_source".to_owned(),
            label: truncate_label(&self.title),
            path: None,
            resource: Some(self.resource.clone()),
            details: BTreeMap::from([("source_id".to_owned(), self.id.clone())]),
        }
    }

    pub fn edge_to(&self, concept: &OkfConcept) -> RepositoryGraphEdge {
        RepositoryGraphEdge {
            id: format!("cites:{}:{}", concept.path, self.id),
            kind: "cites_source".to_owned(),
            source: concept_node_id(&concept.path),
            target: source_node_id(&self.resource),
            label: "cites".to_owned(),
            details: BTreeMap::from([("source_id".to_owned(), self.id.clone())]),
        }
    }
}

pub(super) fn parse_concept(
    document: &IndexedRepositoryDocument,
    root: &str,
) -> Option<OkfConcept> {
    let (frontmatter, body) = split_frontmatter(&document.content)?;
    let details = top_level_details(frontmatter);
    details.get("type")?;
    let title = details
        .get("title")
        .cloned()
        .unwrap_or_else(|| title_from_path(&document.path));
    let sources = parse_sources(frontmatter)
        .into_iter()
        .filter(|source| body.contains(&format!("[^{}]", source.id)))
        .collect();
    let mut links = markdown_links(body)
        .into_iter()
        .filter_map(|target| resolve_link(&document.path, root, &target))
        .collect::<Vec<_>>();
    links.sort();
    links.dedup();

    Some(OkfConcept {
        path: document.path.clone(),
        title,
        details,
        sources,
        links,
    })
}

pub(super) fn concept_link_edge(concept: &OkfConcept, target: &str) -> RepositoryGraphEdge {
    RepositoryGraphEdge {
        id: format!("link:{}:{target}", concept.path),
        kind: "concept_link".to_owned(),
        source: concept_node_id(&concept.path),
        target: concept_node_id(target),
        label: "links".to_owned(),
        details: BTreeMap::new(),
    }
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let normalized = content.strip_prefix("\u{feff}").unwrap_or(content);
    let rest = normalized.strip_prefix("---\n")?;
    let boundary = rest.find("\n---")?;
    let body_start = boundary + 4;
    Some((
        &rest[..boundary],
        rest.get(body_start..)?.trim_start_matches(['\r', '\n']),
    ))
}

fn top_level_details(frontmatter: &str) -> BTreeMap<String, String> {
    let mut details = BTreeMap::new();
    for line in frontmatter.lines() {
        if line.starts_with(char::is_whitespace) || line.trim_start().starts_with('-') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if matches!(
            key,
            "type" | "title" | "description" | "status" | "stale_after"
        ) {
            let value = yaml_scalar(value);
            if !value.is_empty() {
                details.insert(key.to_owned(), value);
            }
        }
    }
    details
}

fn parse_sources(frontmatter: &str) -> Vec<OkfSource> {
    let mut in_sources = false;
    let mut current = BTreeMap::new();
    let mut sources = Vec::new();
    for line in frontmatter.lines() {
        if line == "sources:" {
            in_sources = true;
            continue;
        }
        if !in_sources {
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            break;
        }
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- ") {
            push_source(&mut sources, &mut current);
            insert_source_field(&mut current, rest);
        } else {
            insert_source_field(&mut current, trimmed);
        }
    }
    push_source(&mut sources, &mut current);
    sources
}

fn insert_source_field(fields: &mut BTreeMap<String, String>, line: &str) {
    let Some((key, value)) = line.split_once(':') else {
        return;
    };
    if matches!(key.trim(), "id" | "resource" | "title") {
        fields.insert(key.trim().to_owned(), yaml_scalar(value));
    }
}

fn push_source(sources: &mut Vec<OkfSource>, fields: &mut BTreeMap<String, String>) {
    let Some(id) = fields.remove("id") else {
        fields.clear();
        return;
    };
    let Some(resource) = fields.remove("resource") else {
        fields.clear();
        return;
    };
    if id.is_empty()
        || id.len() > MAX_LABEL_CHARS
        || resource.is_empty()
        || resource.len() > MAX_RESOURCE_BYTES
    {
        fields.clear();
        return;
    }
    let title = fields
        .remove("title")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| id.clone());
    sources.push(OkfSource {
        id,
        resource,
        title,
    });
    fields.clear();
}

fn markdown_links(body: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut offset = 0;
    while let Some(relative_end) = body[offset..].find("](") {
        let end = offset + relative_end;
        let Some(start) = body[..end].rfind('[') else {
            offset = end + 2;
            continue;
        };
        if body.as_bytes().get(start + 1) == Some(&b'^') {
            offset = end + 2;
            continue;
        }
        let target_start = end + 2;
        let Some(relative_close) = body[target_start..].find(')') else {
            break;
        };
        let target = body[target_start..target_start + relative_close].trim();
        let target = target.split_whitespace().next().unwrap_or_default();
        if !target.is_empty() {
            links.push(target.trim_matches(['<', '>']).to_owned());
        }
        offset = target_start + relative_close + 1;
    }
    links
}

fn resolve_link(current: &str, root: &str, target: &str) -> Option<String> {
    let without_fragment = target.split('#').next().unwrap_or_default();
    if without_fragment.is_empty()
        || without_fragment.contains("://")
        || without_fragment.starts_with("mailto:")
    {
        return None;
    }
    let candidate = if let Some(relative) = without_fragment.strip_prefix('/') {
        format!("{root}/{relative}")
    } else {
        let parent = Path::new(current).parent()?.to_string_lossy();
        format!("{parent}/{without_fragment}")
    };
    let normalized = normalize_repository_path(&candidate)?;
    path_is_within(&normalized, root).then_some(normalized)
}

fn concept_node_id(path: &str) -> String {
    format!("okf-concept:{path}")
}

fn source_node_id(resource: &str) -> String {
    format!("external-source:{resource}")
}

fn title_from_path(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .replace('-', " ")
}

fn yaml_scalar(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_owned()
}

fn truncate_label(value: &str) -> String {
    value.chars().take(MAX_LABEL_CHARS).collect()
}
