use std::collections::{BTreeMap, BTreeSet};

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

struct ConceptSelection {
    paths: BTreeMap<String, u8>,
    truncated: bool,
}

struct NodeAssembly {
    nodes: Vec<RepositoryGraphNode>,
    concept_paths: BTreeSet<String>,
    leaf_resources: BTreeSet<String>,
    truncated: bool,
}

#[derive(Clone, Copy)]
enum EdgeCandidate<'a> {
    SourceConcept {
        concept: &'a okf::OkfConcept,
        source: &'a okf::OkfSource,
        target: &'a str,
    },
    SourceLeaf {
        concept: &'a okf::OkfConcept,
        source: &'a okf::OkfSource,
    },
    ConceptLink {
        concept: &'a okf::OkfConcept,
        target: &'a str,
    },
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
enum EdgeCandidateKey<'a> {
    Source {
        concept_path: &'a str,
        resource: &'a str,
        source_id: Option<&'a str>,
    },
    Link {
        concept_path: &'a str,
        target: &'a str,
    },
}

/// Projects OKF v0.2 concepts from indexed Markdown without reading the live worktree.
pub fn project_okf_neighborhood(
    documents: &[IndexedRepositoryDocument],
    request: &RepositoryGraphNeighborhoodRequest,
) -> Result<RepositoryGraphNeighborhood, DomainError> {
    let root = matching_repository_root(&request.focus_path, &request.repository.path_filters)
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

    let selected = selected_concepts(
        &concepts,
        &request.focus_path,
        request.depth,
        request.node_limit,
    );
    let source_truncated = concepts.values().any(|concept| concept.truncated);
    let nodes = assemble_nodes(&concepts, &selected, request.node_limit);
    let (edges, edges_truncated) = assemble_edges(
        &concepts,
        &nodes.concept_paths,
        &nodes.leaf_resources,
        request.edge_limit,
    );

    Ok(RepositoryGraphNeighborhood {
        truncated: source_truncated || selected.truncated || nodes.truncated || edges_truncated,
        nodes: nodes.nodes,
        edges,
    })
}

fn selected_concepts(
    concepts: &BTreeMap<String, okf::OkfConcept>,
    focus: &str,
    depth: u8,
    limit: usize,
) -> ConceptSelection {
    let mut paths = BTreeMap::from([(focus.to_owned(), 0_u8)]);
    let mut frontier = BTreeSet::from([focus.to_owned()]);
    let mut truncated = false;
    for distance in 1..=depth {
        let remaining = limit.saturating_sub(paths.len());
        let candidate_capacity = remaining.saturating_add(1);
        let mut candidates = BTreeSet::new();
        for (source_path, concept) in concepts {
            for target in concept.relationship_targets() {
                if !concepts.contains_key(target) {
                    continue;
                }
                let candidate = if frontier.contains(source_path) && !paths.contains_key(target) {
                    Some(target)
                } else if frontier.contains(target) && !paths.contains_key(source_path) {
                    Some(source_path.as_str())
                } else {
                    None
                };
                if let Some(candidate) = candidate {
                    truncated |= insert_bounded_set(
                        &mut candidates,
                        candidate.to_owned(),
                        candidate_capacity,
                    );
                }
            }
        }
        if candidates.len() > remaining {
            truncated = true;
            candidates.pop_last();
        }
        if candidates.is_empty() {
            break;
        }
        for path in &candidates {
            paths.insert(path.clone(), distance);
        }
        frontier = candidates;
    }
    ConceptSelection { paths, truncated }
}

fn assemble_nodes(
    concepts: &BTreeMap<String, okf::OkfConcept>,
    selected: &ConceptSelection,
    node_limit: usize,
) -> NodeAssembly {
    let mut nodes = Vec::with_capacity(node_limit);
    let mut concept_paths = BTreeSet::new();
    let mut leaf_resources = BTreeSet::new();
    let mut truncated = false;
    let max_distance = selected
        .paths
        .values()
        .copied()
        .max()
        .unwrap_or_default()
        .saturating_add(1);

    for distance in 0..=max_distance {
        for (path, candidate_distance) in &selected.paths {
            if *candidate_distance != distance {
                continue;
            }
            if nodes.len() >= node_limit {
                truncated = true;
                continue;
            }
            nodes.push(concepts[path].node());
            concept_paths.insert(path.clone());
        }
        if distance == 0 {
            continue;
        }

        let remaining = node_limit.saturating_sub(nodes.len());
        let candidate_capacity = remaining.saturating_add(1);
        let mut candidates = BTreeMap::<(bool, &str), &okf::OkfSource>::new();
        for path in &concept_paths {
            if selected.paths[path].saturating_add(1) != distance {
                continue;
            }
            for source in &concepts[path].sources {
                let targets_concept = source
                    .candidate_path
                    .as_ref()
                    .is_some_and(|target| concepts.contains_key(target));
                if targets_concept || leaf_resources.contains(source.resource.as_str()) {
                    continue;
                }
                truncated |= insert_bounded_map(
                    &mut candidates,
                    (source.bundle_path_hint, source.resource.as_str()),
                    source,
                    candidate_capacity,
                );
            }
        }
        if candidates.len() > remaining {
            truncated = true;
            candidates.pop_last();
        }
        for source in candidates.into_values() {
            nodes.push(source.leaf_node());
            leaf_resources.insert(source.resource.clone());
        }
    }

    NodeAssembly {
        nodes,
        concept_paths,
        leaf_resources,
        truncated,
    }
}

fn assemble_edges(
    concepts: &BTreeMap<String, okf::OkfConcept>,
    concept_paths: &BTreeSet<String>,
    leaf_resources: &BTreeSet<String>,
    edge_limit: usize,
) -> (Vec<RepositoryGraphEdge>, bool) {
    let candidate_capacity = edge_limit.saturating_add(1);
    let mut candidates = BTreeMap::new();
    let mut truncated = false;
    for path in concept_paths {
        let concept = &concepts[path];
        for source in &concept.sources {
            let candidate = if let Some(target) = source
                .candidate_path
                .as_deref()
                .filter(|target| concepts.contains_key(*target))
            {
                concept_paths
                    .contains(target)
                    .then_some(EdgeCandidate::SourceConcept {
                        concept,
                        source,
                        target,
                    })
            } else {
                leaf_resources
                    .contains(source.resource.as_str())
                    .then_some(EdgeCandidate::SourceLeaf { concept, source })
            };
            if let Some(candidate) = candidate {
                truncated |= insert_bounded_map(
                    &mut candidates,
                    EdgeCandidateKey::Source {
                        concept_path: &concept.path,
                        resource: &source.resource,
                        source_id: source.id.as_deref(),
                    },
                    candidate,
                    candidate_capacity,
                );
            }
        }
        for target in &concept.links {
            if concept_paths.contains(target) {
                truncated |= insert_bounded_map(
                    &mut candidates,
                    EdgeCandidateKey::Link {
                        concept_path: &concept.path,
                        target,
                    },
                    EdgeCandidate::ConceptLink { concept, target },
                    candidate_capacity,
                );
            }
        }
    }
    if candidates.len() > edge_limit {
        truncated = true;
        candidates.pop_last();
    }
    let mut edges = candidates
        .into_values()
        .map(|candidate| match candidate {
            EdgeCandidate::SourceConcept {
                concept,
                source,
                target,
            } => source.edge_to_concept(concept, target),
            EdgeCandidate::SourceLeaf { concept, source } => source.edge_to_leaf(concept),
            EdgeCandidate::ConceptLink { concept, target } => {
                okf::concept_link_edge(concept, target)
            }
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| left.id.cmp(&right.id));
    (edges, truncated)
}

fn insert_bounded_set<T: Ord>(set: &mut BTreeSet<T>, value: T, capacity: usize) -> bool {
    if !set.insert(value) || set.len() <= capacity {
        return false;
    }
    set.pop_last();
    true
}

fn insert_bounded_map<K: Ord, V>(
    map: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    capacity: usize,
) -> bool {
    if map.contains_key(&key) {
        return false;
    }
    map.insert(key, value);
    if map.len() <= capacity {
        return false;
    }
    map.pop_last();
    true
}

fn matching_repository_root<'a>(focus: &str, roots: &'a [String]) -> Option<&'a str> {
    roots
        .iter()
        .filter(|root| path_is_within(focus, root))
        .max_by_key(|root| root_specificity(root))
        .map(String::as_str)
}

fn root_specificity(root: &str) -> (usize, usize) {
    normalize_repository_root(root)
        .map(|root| {
            (
                root.split('/')
                    .filter(|component| !component.is_empty())
                    .count(),
                root.len(),
            )
        })
        .unwrap_or_default()
}

fn normalize_repository_root(root: &str) -> Option<String> {
    if root == "." {
        Some(String::new())
    } else {
        normalize_repository_path(root)
    }
}

fn reserved_okf_path(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|name| matches!(name, "index.md" | "log.md"))
}

pub(super) fn path_is_within(path: &str, root: &str) -> bool {
    let Some(path) = normalize_repository_path(path) else {
        return false;
    };
    normalize_repository_root(root).is_some_and(|root| {
        root.is_empty()
            || path == root
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
