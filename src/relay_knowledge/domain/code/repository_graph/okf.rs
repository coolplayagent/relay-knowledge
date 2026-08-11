use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde_norway::Value;
use tree_sitter::{Node, Parser, Tree, TreeCursor};

use super::{
    IndexedRepositoryDocument, RepositoryGraphEdge, RepositoryGraphNode, normalize_repository_path,
    path_is_within,
};

const MAX_LABEL_CHARS: usize = 256;
const MAX_RESOURCE_BYTES: usize = 4_096;
const MAX_MARKDOWN_BODY_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_CONCEPT_LINKS: usize = 256;
pub(super) const MAX_CONCEPT_SOURCES: usize = 256;
const MAX_REFERENCE_DEFINITIONS: usize = 1_024;
const MAX_REFERENCE_LABEL_CHARS: usize = 999;

#[derive(Debug, Default)]
struct MarkdownLinks {
    targets: BTreeSet<String>,
    truncated: bool,
}

impl MarkdownLinks {
    fn insert(&mut self, target: &str) -> bool {
        if self.targets.contains(target) {
            return false;
        }
        if self.targets.len() >= MAX_CONCEPT_LINKS {
            self.truncated = true;
            return true;
        }
        self.targets.insert(target.to_owned());
        false
    }
}

#[derive(Debug, Clone)]
pub(super) struct OkfConcept {
    pub path: String,
    pub title: String,
    pub details: BTreeMap<String, String>,
    pub sources: Vec<OkfSource>,
    pub links: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub(super) struct OkfSource {
    pub(super) id: Option<String>,
    pub(super) resource: String,
    title: String,
    pub(super) candidate_path: Option<String>,
    pub(super) bundle_path_hint: bool,
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

    pub fn relationship_targets(&self) -> impl Iterator<Item = &str> {
        self.links.iter().map(String::as_str).chain(
            self.sources
                .iter()
                .filter_map(|source| source.candidate_path.as_deref()),
        )
    }
}

impl OkfSource {
    pub fn leaf_node(&self) -> RepositoryGraphNode {
        let kind = self.leaf_kind();
        let mut details = source_details(self.id.as_deref());
        if kind == "unresolved_source" {
            details.insert("resolution_state".to_owned(), "unresolved".to_owned());
            details.insert("target_hint".to_owned(), self.resource.clone());
        }
        RepositoryGraphNode {
            id: source_node_id(kind, &self.resource),
            kind: kind.to_owned(),
            label: truncate_label(&self.title),
            path: None,
            resource: Some(self.resource.clone()),
            details,
        }
    }

    pub fn edge_to_leaf(&self, concept: &OkfConcept) -> RepositoryGraphEdge {
        self.edge_to(concept, source_node_id(self.leaf_kind(), &self.resource))
    }

    pub fn edge_to_concept(&self, concept: &OkfConcept, target_path: &str) -> RepositoryGraphEdge {
        self.edge_to(concept, concept_node_id(target_path))
    }

    fn edge_to(&self, concept: &OkfConcept, target: String) -> RepositoryGraphEdge {
        let id = self.id.as_deref().unwrap_or_default();
        RepositoryGraphEdge {
            id: format!(
                "cites:{}:{}:{}:{}:{}:{}",
                concept.path.len(),
                concept.path,
                self.resource.len(),
                self.resource,
                id.len(),
                id
            ),
            kind: "cites_source".to_owned(),
            source: concept_node_id(&concept.path),
            target,
            label: "cites".to_owned(),
            details: source_details(self.id.as_deref()),
        }
    }

    fn leaf_kind(&self) -> &'static str {
        if self.bundle_path_hint {
            "unresolved_source"
        } else {
            "external_source"
        }
    }
}

pub(super) fn parse_concept(
    document: &IndexedRepositoryDocument,
    root: &str,
) -> Option<OkfConcept> {
    let (frontmatter, body) = split_frontmatter(&document.content)?;
    let frontmatter: Value = serde_norway::from_str(frontmatter).ok()?;
    let Value::Mapping(_) = frontmatter else {
        return None;
    };
    let concept_type = string_value(frontmatter.get("type")?)?;
    if concept_type.is_empty() {
        return None;
    }

    let mut details = BTreeMap::from([("type".to_owned(), concept_type)]);
    for key in ["title", "description", "status", "stale_after"] {
        if let Some(value) = frontmatter.get(key).and_then(scalar_value)
            && !value.is_empty()
        {
            details.insert(key.to_owned(), value);
        }
    }
    let title = details
        .get("title")
        .cloned()
        .unwrap_or_else(|| title_from_path(&document.path));
    let (sources, sources_truncated) = parse_sources(&frontmatter, &document.path, root);
    let extracted_links = markdown_links(body);
    if extracted_links.truncated {
        details.insert("link_extraction_truncated".to_owned(), "true".to_owned());
    }
    if sources_truncated {
        details.insert("source_extraction_truncated".to_owned(), "true".to_owned());
    }
    let mut links = extracted_links
        .targets
        .into_iter()
        .filter_map(|target| resolve_repository_relative_path(&document.path, root, &target))
        .collect::<Vec<_>>();
    links.sort();
    links.dedup();

    Some(OkfConcept {
        path: document.path.clone(),
        title,
        details,
        sources,
        links,
        truncated: extracted_links.truncated || sources_truncated,
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
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let opening_end = content.find('\n')?;
    (content[..opening_end]
        .strip_suffix('\r')
        .unwrap_or(&content[..opening_end])
        == "---")
        .then_some(())?;

    let frontmatter_start = opening_end + 1;
    let mut line_start = frontmatter_start;
    loop {
        let next_newline = content[line_start..].find('\n');
        let line_end = next_newline
            .map(|offset| line_start + offset)
            .unwrap_or(content.len());
        let line = content[line_start..line_end]
            .strip_suffix('\r')
            .unwrap_or(&content[line_start..line_end]);
        if line == "---" {
            let body_start = next_newline.map_or(line_end, |_| line_end + 1);
            return Some((
                &content[frontmatter_start..line_start],
                &content[body_start..],
            ));
        }
        next_newline?;
        line_start = line_end + 1;
    }
}

fn parse_sources(frontmatter: &Value, current: &str, root: &str) -> (Vec<OkfSource>, bool) {
    let Some(Value::Sequence(sources)) = frontmatter.get("sources") else {
        return (Vec::new(), false);
    };
    (
        sources
            .iter()
            .take(MAX_CONCEPT_SOURCES)
            .filter_map(|source| parse_source(source, current, root))
            .collect(),
        sources.len() > MAX_CONCEPT_SOURCES,
    )
}

fn parse_source(source: &Value, current: &str, root: &str) -> Option<OkfSource> {
    let Value::Mapping(_) = source else {
        return None;
    };
    let resource = string_value(source.get("resource")?)?;
    if resource.is_empty() || resource.len() > MAX_RESOURCE_BYTES {
        return None;
    }
    let id = source
        .get("id")
        .and_then(string_value)
        .filter(|id| !id.is_empty() && id.chars().count() <= MAX_LABEL_CHARS);
    let title = source
        .get("title")
        .and_then(string_value)
        .filter(|title| !title.is_empty())
        .or_else(|| id.clone())
        .unwrap_or_else(|| resource.clone());
    let bundle_path_hint = has_bundle_path_syntax(&resource);
    let candidate_path = candidate_bundle_path(current, root, &resource);
    Some(OkfSource {
        id,
        resource,
        title,
        candidate_path,
        bundle_path_hint,
    })
}

fn string_value(value: &Value) -> Option<String> {
    let Value::String(value) = value else {
        return None;
    };
    Some(value.trim().to_owned())
}

fn scalar_value(value: &Value) -> Option<String> {
    let value = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => return None,
    };
    Some(value.trim().to_owned())
}

fn markdown_links(body: &str) -> MarkdownLinks {
    if body.len() > MAX_MARKDOWN_BODY_BYTES {
        return MarkdownLinks {
            truncated: true,
            ..MarkdownLinks::default()
        };
    }

    let mut block_parser = Parser::new();
    if block_parser
        .set_language(&tree_sitter_md::LANGUAGE.into())
        .is_err()
    {
        return MarkdownLinks {
            truncated: true,
            ..MarkdownLinks::default()
        };
    }
    let Some(tree) = block_parser.parse(body, None) else {
        return MarkdownLinks {
            truncated: true,
            ..MarkdownLinks::default()
        };
    };
    let mut links = MarkdownLinks::default();
    let definitions = reference_definitions(body, &tree, &mut links.truncated);
    collect_document_inline_links(body, &tree, &definitions, &mut links);
    links
}

fn reference_definitions(
    body: &str,
    tree: &Tree,
    truncated: &mut bool,
) -> BTreeMap<String, String> {
    let mut definitions = BTreeMap::new();
    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        let is_definition = node.kind() == "link_reference_definition";
        if is_definition
            && let Some(label) = direct_named_child_of_kind(node, "link_label")
                .and_then(|label| normalized_reference_label(body, label, truncated))
            && let Some(target) = direct_named_child_of_kind(node, "link_destination")
                .and_then(|target| bounded_markdown_target(body, target, truncated))
            && !definitions.contains_key(&label)
        {
            if definitions.len() >= MAX_REFERENCE_DEFINITIONS {
                *truncated = true;
            } else {
                definitions.insert(label, target);
            }
        }
        if !advance_markdown_cursor(&mut cursor, !is_definition) {
            break;
        }
    }
    definitions
}

fn collect_document_inline_links(
    body: &str,
    tree: &Tree,
    definitions: &BTreeMap<String, String>,
    links: &mut MarkdownLinks,
) {
    let mut inline_parser = Parser::new();
    if inline_parser
        .set_language(&tree_sitter_md::INLINE_LANGUAGE.into())
        .is_err()
    {
        links.truncated = true;
        return;
    }

    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        let is_inline = matches!(node.kind(), "inline" | "pipe_table_cell");
        if is_inline {
            let Some(source) = body.get(node.byte_range()) else {
                links.truncated = true;
                return;
            };
            let Some(inline_tree) = inline_parser.parse(source, None) else {
                links.truncated = true;
                return;
            };
            if collect_inline_links(source, inline_tree.root_node(), definitions, links) {
                return;
            }
        }
        if !advance_markdown_cursor(&mut cursor, !is_inline) {
            return;
        }
    }
}

fn collect_inline_links(
    body: &str,
    root: Node<'_>,
    definitions: &BTreeMap<String, String>,
    links: &mut MarkdownLinks,
) -> bool {
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        let (target, descend) = match node.kind() {
            "inline_link" => (
                direct_named_child_of_kind(node, "link_destination")
                    .and_then(|target| bounded_markdown_target(body, target, &mut links.truncated)),
                false,
            ),
            "full_reference_link" => (
                direct_named_child_of_kind(node, "link_label")
                    .and_then(|label| normalized_reference_label(body, label, &mut links.truncated))
                    .and_then(|label| definitions.get(&label).cloned()),
                false,
            ),
            "collapsed_reference_link" | "shortcut_link" => (
                direct_named_child_of_kind(node, "link_text")
                    .and_then(|label| normalized_reference_label(body, label, &mut links.truncated))
                    .and_then(|label| definitions.get(&label).cloned()),
                false,
            ),
            "image" | "code_span" => (None, false),
            _ => (None, true),
        };
        if target.as_deref().is_some_and(|target| links.insert(target)) {
            return true;
        }
        if !advance_markdown_cursor(&mut cursor, descend) {
            return false;
        }
    }
}

fn direct_named_child_of_kind<'tree>(root: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    (0..root.named_child_count()).find_map(|index| {
        let child = root.named_child(u32::try_from(index).ok()?)?;
        (child.kind() == kind).then_some(child)
    })
}

fn bounded_markdown_target(body: &str, node: Node<'_>, truncated: &mut bool) -> Option<String> {
    let target = body.get(node.byte_range())?.trim();
    let target = target
        .strip_prefix('<')
        .and_then(|target| target.strip_suffix('>'))
        .unwrap_or(target)
        .trim();
    if target.is_empty() {
        return None;
    }
    if target.len() > MAX_RESOURCE_BYTES {
        *truncated = true;
        return None;
    }
    let target = markdown_path_without_query_or_fragment(target).trim();
    if target.is_empty() {
        return None;
    }
    let target = decode_markdown_escapes(target);
    let target = percent_decode_path(&target).unwrap_or(target);
    if target.len() > MAX_RESOURCE_BYTES {
        *truncated = true;
        return None;
    }
    if has_uri_scheme(&target) || target.starts_with("//") {
        return None;
    }
    Some(target)
}

fn markdown_path_without_query_or_fragment(target: &str) -> &str {
    let mut escaped = false;
    for (index, character) in target.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if matches!(character, '?' | '#') {
            return &target[..index];
        }
    }
    target
}

fn decode_markdown_escapes(path: &str) -> String {
    if !path.as_bytes().contains(&b'\\') {
        return path.to_owned();
    }

    let mut decoded = String::with_capacity(path.len());
    let mut characters = path.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\'
            && let Some(next) = characters.peek()
            && next.is_ascii_punctuation()
        {
            decoded.push(*next);
            characters.next();
        } else {
            decoded.push(character);
        }
    }
    decoded
}

fn percent_decode_path(path: &str) -> Option<String> {
    if !path.as_bytes().contains(&b'%') {
        return Some(path.to_owned());
    }

    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(hex) = bytes.get(index + 1..index + 3)
            && let Some(byte) = decode_hex_pair(hex)
        {
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn decode_hex_pair(hex: &[u8]) -> Option<u8> {
    let [high, low] = hex else {
        return None;
    };
    Some(hex_value(*high)? << 4 | hex_value(*low)?)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn normalized_reference_label(body: &str, node: Node<'_>, truncated: &mut bool) -> Option<String> {
    let label = body.get(node.byte_range())?.trim();
    let label = label
        .strip_prefix('[')
        .and_then(|label| label.strip_suffix(']'))
        .unwrap_or(label)
        .trim();
    if label.is_empty() || label.starts_with('^') {
        return None;
    }
    if label.chars().count() > MAX_REFERENCE_LABEL_CHARS || label.len() > MAX_RESOURCE_BYTES {
        *truncated = true;
        return None;
    }

    let label = decode_markdown_escapes(label);
    let mut normalized = String::with_capacity(label.len());
    for word in label.split_whitespace() {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.extend(word.chars().flat_map(char::to_lowercase));
    }
    (!normalized.is_empty()).then_some(normalized)
}

fn advance_markdown_cursor(cursor: &mut TreeCursor<'_>, descend: bool) -> bool {
    if descend && cursor.goto_first_child() {
        return true;
    }
    while !cursor.goto_next_sibling() {
        if !cursor.goto_parent() {
            return false;
        }
    }
    true
}

fn has_bundle_path_syntax(resource: &str) -> bool {
    let path = resource.split(['#', '?']).next().unwrap_or_default().trim();
    !path.is_empty()
        && !has_uri_scheme(path)
        && !path.starts_with("//")
        && (path.starts_with('/')
            || path.starts_with("./")
            || path.starts_with("../")
            || path.starts_with("references/")
            || Path::new(path).extension().is_some())
}

fn candidate_bundle_path(current: &str, root: &str, resource: &str) -> Option<String> {
    let path = resource.split(['#', '?']).next().unwrap_or_default();
    (!path.is_empty() && !has_uri_scheme(path) && !path.starts_with("//"))
        .then(|| resolve_link(current, root, resource))
        .flatten()
}

fn resolve_link(current: &str, root: &str, target: &str) -> Option<String> {
    let path = target.split(['#', '?']).next().unwrap_or_default();
    resolve_repository_relative_path(current, root, path)
}

fn resolve_repository_relative_path(current: &str, root: &str, path: &str) -> Option<String> {
    if path.is_empty() || has_uri_scheme(path) || path.starts_with("//") {
        return None;
    }
    let candidate = if let Some(relative) = path.strip_prefix('/') {
        join_path(root, relative)
    } else {
        let parent = Path::new(current).parent()?.to_string_lossy();
        join_path(&parent, path)
    };
    let normalized = normalize_repository_path(&candidate)?;
    path_is_within(&normalized, root).then_some(normalized)
}

fn join_path(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() || prefix == "." {
        suffix.to_owned()
    } else {
        format!("{prefix}/{suffix}")
    }
}

fn has_uri_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme.chars().enumerate().all(|(index, character)| {
            if index == 0 {
                character.is_ascii_alphabetic()
            } else {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
            }
        })
}

fn source_details(id: Option<&str>) -> BTreeMap<String, String> {
    id.map(|id| BTreeMap::from([("source_id".to_owned(), id.to_owned())]))
        .unwrap_or_default()
}

fn concept_node_id(path: &str) -> String {
    format!("okf-concept:{path}")
}

fn source_node_id(kind: &str, resource: &str) -> String {
    match kind {
        "unresolved_source" => format!("unresolved-source:{resource}"),
        _ => format!("external-source:{resource}"),
    }
}

fn title_from_path(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .replace('-', " ")
}

fn truncate_label(value: &str) -> String {
    value.chars().take(MAX_LABEL_CHARS).collect()
}

#[cfg(test)]
#[path = "okf_tests.rs"]
mod tests;
