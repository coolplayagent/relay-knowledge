use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use super::{CodeRepositorySelector, DomainError};

mod okf;

pub const REPOSITORY_GRAPH_DEFAULT_NODE_LIMIT: usize = 100;
pub const REPOSITORY_GRAPH_DEFAULT_EDGE_LIMIT: usize = 200;
pub const REPOSITORY_GRAPH_MAX_NODE_LIMIT: usize = 100;
pub const REPOSITORY_GRAPH_MAX_EDGE_LIMIT: usize = 200;
pub const REPOSITORY_GRAPH_MAX_DEPTH: u8 = 2;

/// Indexed, snapshot-bound text used to derive portable repository relationships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedRepositoryDocument {
    pub path: String,
    pub language_id: String,
    pub content: String,
}

/// Validated request for a bounded repository graph neighborhood.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryGraphNeighborhoodRequest {
    pub repository: CodeRepositorySelector,
    pub focus_path: String,
    pub depth: u8,
    pub node_limit: usize,
    pub edge_limit: usize,
}

impl RepositoryGraphNeighborhoodRequest {
    pub fn new(
        repository: CodeRepositorySelector,
        focus_path: impl Into<String>,
        depth: u8,
        node_limit: usize,
        edge_limit: usize,
    ) -> Result<Self, DomainError> {
        let focus_path = normalize_repository_path(&focus_path.into()).ok_or_else(|| {
            DomainError::invalid(
                "focus_path",
                "must be a normalized relative repository path",
            )
        })?;
        if depth == 0 || depth > REPOSITORY_GRAPH_MAX_DEPTH {
            return Err(DomainError::invalid(
                "depth",
                format!("must be between 1 and {REPOSITORY_GRAPH_MAX_DEPTH}"),
            ));
        }
        if node_limit == 0 || node_limit > REPOSITORY_GRAPH_MAX_NODE_LIMIT {
            return Err(DomainError::invalid(
                "node_limit",
                format!("must be between 1 and {REPOSITORY_GRAPH_MAX_NODE_LIMIT}"),
            ));
        }
        if edge_limit == 0 || edge_limit > REPOSITORY_GRAPH_MAX_EDGE_LIMIT {
            return Err(DomainError::invalid(
                "edge_limit",
                format!("must be between 1 and {REPOSITORY_GRAPH_MAX_EDGE_LIMIT}"),
            ));
        }
        if repository
            .language_filters
            .iter()
            .any(|value| value != "markdown")
        {
            return Err(DomainError::invalid(
                "language_filter",
                "repository graph neighborhoods only accept markdown",
            ));
        }
        if repository.path_filters.is_empty()
            || !repository
                .path_filters
                .iter()
                .any(|root| path_is_within(&focus_path, root))
        {
            return Err(DomainError::invalid(
                "focus_path",
                "must be inside an explicit repository path filter",
            ));
        }

        Ok(Self {
            repository,
            focus_path,
            depth,
            node_limit,
            edge_limit,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryGraphNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryGraphEdge {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub target: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryGraphNeighborhood {
    pub nodes: Vec<RepositoryGraphNode>,
    pub edges: Vec<RepositoryGraphEdge>,
    pub truncated: bool,
}

/// Projects OKF v0.2 concepts from indexed Markdown without reading the live worktree.
pub fn project_okf_neighborhood(
    documents: &[IndexedRepositoryDocument],
    request: &RepositoryGraphNeighborhoodRequest,
) -> Result<RepositoryGraphNeighborhood, DomainError> {
    let root = request
        .repository
        .path_filters
        .iter()
        .find(|root| path_is_within(&request.focus_path, root))
        .expect("request validation requires a matching root");
    let concepts = documents
        .iter()
        .filter(|document| {
            document.language_id == "markdown"
                && path_is_within(&document.path, root)
                && !reserved_okf_path(&document.path)
        })
        .filter_map(|document| okf::parse_concept(document, root))
        .map(|concept| (concept.path.clone(), concept))
        .collect::<BTreeMap<_, _>>();
    if !concepts.contains_key(&request.focus_path) {
        return Err(DomainError::invalid(
            "focus_path",
            "does not identify an indexed OKF concept",
        ));
    }

    let selected = selected_concepts(&concepts, &request.focus_path, request.depth);
    let mut nodes = Vec::new();
    for path in &selected {
        if let Some(concept) = concepts.get(path) {
            nodes.push(concept.node());
        }
    }
    let mut edges = Vec::new();
    for path in &selected {
        let concept = &concepts[path];
        for source in &concept.sources {
            nodes.push(source.node());
            edges.push(source.edge_to(concept));
        }
        for link in &concept.links {
            if selected.contains(link) {
                edges.push(okf::concept_link_edge(concept, link));
            }
        }
    }
    deduplicate_nodes(&mut nodes);
    edges.sort_by(|left, right| left.id.cmp(&right.id));
    edges.dedup_by(|left, right| left.id == right.id);

    let available_node_count = nodes.len();
    nodes.truncate(request.node_limit);
    let retained = nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    edges.retain(|edge| {
        retained.contains(edge.source.as_str()) && retained.contains(edge.target.as_str())
    });
    let available_edge_count = edges.len();
    edges.truncate(request.edge_limit);

    Ok(RepositoryGraphNeighborhood {
        truncated: available_node_count > nodes.len() || available_edge_count > edges.len(),
        nodes,
        edges,
    })
}

fn selected_concepts(
    concepts: &BTreeMap<String, okf::OkfConcept>,
    focus: &str,
    depth: u8,
) -> BTreeSet<String> {
    let mut selected = BTreeSet::new();
    let mut queue = VecDeque::from([(focus.to_owned(), 0_u8)]);
    while let Some((path, distance)) = queue.pop_front() {
        if !selected.insert(path.clone()) || distance >= depth {
            continue;
        }
        let mut neighbors = concepts
            .get(&path)
            .map(|concept| concept.links.clone())
            .unwrap_or_default();
        neighbors.extend(concepts.iter().filter_map(|(candidate, concept)| {
            concept.links.contains(&path).then_some(candidate.clone())
        }));
        neighbors.sort();
        neighbors.dedup();
        for neighbor in neighbors {
            if concepts.contains_key(&neighbor) {
                queue.push_back((neighbor, distance + 1));
            }
        }
    }
    selected
}

fn deduplicate_nodes(nodes: &mut Vec<RepositoryGraphNode>) {
    nodes.sort_by(|left, right| {
        node_priority(left)
            .cmp(&node_priority(right))
            .then_with(|| left.id.cmp(&right.id))
    });
    nodes.dedup_by(|left, right| left.id == right.id);
}

fn node_priority(node: &RepositoryGraphNode) -> u8 {
    match node.kind.as_str() {
        "okf_concept" => 0,
        "external_source" => 1,
        _ => 2,
    }
}

fn reserved_okf_path(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|name| matches!(name, "index.md" | "log.md"))
}

pub(super) fn path_is_within(path: &str, root: &str) -> bool {
    normalize_repository_path(root).is_some_and(|root| {
        path == root
            || path
                .strip_prefix(&root)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

pub(super) fn normalize_repository_path(path: &str) -> Option<String> {
    if path.is_empty() || path.starts_with('/') || path.contains('\0') || path.contains('\\') {
        return None;
    }
    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            value => components.push(value),
        }
    }
    (!components.is_empty()).then(|| components.join("/"))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
