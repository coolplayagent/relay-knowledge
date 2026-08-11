use super::{
    IndexedRepositoryDocument, RepositoryGraphNeighborhoodRequest,
    okf::{MAX_CONCEPT_LINKS, MAX_CONCEPT_SOURCES, parse_concept},
    project_okf_neighborhood, selected_concepts,
};
use crate::domain::CodeRepositorySelector;
use std::collections::BTreeMap;

#[test]
fn projects_okf_sources_and_links_into_a_bounded_neighborhood() {
    let request = RepositoryGraphNeighborhoodRequest::new(
        CodeRepositorySelector::new(
            "stone-star",
            "0123456789012345678901234567890123456789",
            vec!["knowledge/investment-research".to_owned()],
            Vec::new(),
        )
        .expect("selector"),
        "knowledge/investment-research/rates.md",
        1,
        100,
        200,
    )
    .expect("request");
    let documents = vec![
        document(
            "knowledge/investment-research/rates.md",
            r#"---
type: Research Claim
title: 政策利率
status: stable
sources:
  - id: pbc
    resource: https://www.pbc.gov.cn/rates
    title: 中国人民银行政策利率
---

政策利率保持稳定。[^pbc]
参见[收益率曲线](curves.md)。

[^pbc]: 中国人民银行政策利率
"#,
        ),
        document(
            "knowledge/investment-research/curves.md",
            r#"---
type: Research Claim
title: 收益率曲线
status: draft
---

曲线概念。
"#,
        ),
    ];

    let graph = project_okf_neighborhood(&documents, &request).expect("neighborhood");

    assert_eq!(graph.nodes.len(), 3);
    assert!(graph.nodes.iter().any(|node| {
        node.kind == "okf_concept"
            && node.path.as_deref() == Some("knowledge/investment-research/rates.md")
            && node.details.get("status").map(String::as_str) == Some("stable")
    }));
    assert!(graph.nodes.iter().any(|node| {
        node.kind == "external_source"
            && node.resource.as_deref() == Some("https://www.pbc.gov.cn/rates")
    }));
    assert!(graph.edges.iter().any(|edge| edge.kind == "cites_source"));
    assert!(graph.edges.iter().any(|edge| edge.kind == "concept_link"));
    assert!(!graph.truncated);
}

#[test]
fn rejects_unbounded_or_out_of_scope_graph_requests() {
    let selector = CodeRepositorySelector::new(
        "stone-star",
        "HEAD",
        vec!["knowledge/investment-research".to_owned()],
        Vec::new(),
    )
    .expect("selector");

    assert!(
        RepositoryGraphNeighborhoodRequest::new(selector.clone(), "docs/outside.md", 1, 100, 200,)
            .is_err()
    );
    assert!(
        RepositoryGraphNeighborhoodRequest::new(
            selector,
            "knowledge/investment-research/rates.md",
            3,
            100,
            200,
        )
        .is_err()
    );
}

#[test]
fn root_scope_preserves_internal_external_and_unresolved_sources() {
    let request = RepositoryGraphNeighborhoodRequest::new(
        selector(vec![".".to_owned()]),
        "bundle/focus.md",
        1,
        100,
        200,
    )
    .expect("repository root is a valid scope");
    let documents = vec![
        document(
            "bundle/focus.md",
            r#"---
type: Research Claim
sources:
  - resource: ./relative.md
  - resource: /root-target.md
  - resource: https://example.com/evidence
  - resource: all rows in dataset alpha
  - resource: projects/acme/queries
  - resource: references/source.pdf
  - resource: ../../outside.md
---
"#,
        ),
        document("bundle/relative.md", "---\ntype: Research Claim\n---\n"),
        document("root-target.md", "---\ntype: Research Claim\n---\n"),
    ];

    let graph = project_okf_neighborhood(&documents, &request).expect("neighborhood");

    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|node| node.kind == "okf_concept")
            .count(),
        3
    );
    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|node| node.kind == "external_source")
            .count(),
        3
    );
    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|node| node.kind == "unresolved_source")
            .count(),
        2
    );
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == "cites_source" && edge.target == "okf-concept:bundle/relative.md"
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == "cites_source" && edge.target == "okf-concept:root-target.md"
    }));
    assert!(
        graph
            .nodes
            .iter()
            .filter(|node| node.kind != "okf_concept")
            .all(|node| { graph.edges.iter().any(|edge| edge.target == node.id) })
    );
}

#[test]
fn selects_the_most_specific_matching_bundle_root() {
    let request = RepositoryGraphNeighborhoodRequest::new(
        selector(vec!["knowledge".to_owned(), "knowledge/bundle".to_owned()]),
        "knowledge/bundle/focus.md",
        1,
        100,
        200,
    )
    .expect("request");
    let documents = vec![
        document(
            "knowledge/bundle/focus.md",
            "---\ntype: Research Claim\nsources:\n  - resource: /target.md\n---\n",
        ),
        document(
            "knowledge/bundle/target.md",
            "---\ntype: Research Claim\n---\n",
        ),
        document("knowledge/target.md", "---\ntype: Research Claim\n---\n"),
    ];

    let graph = project_okf_neighborhood(&documents, &request).expect("neighborhood");

    assert!(
        graph
            .nodes
            .iter()
            .any(|node| { node.path.as_deref() == Some("knowledge/bundle/target.md") })
    );
    assert!(
        !graph
            .nodes
            .iter()
            .any(|node| node.path.as_deref() == Some("knowledge/target.md"))
    );
}

#[test]
fn node_limit_one_always_retains_the_focus_concept() {
    let request = RepositoryGraphNeighborhoodRequest::new(
        selector(vec!["knowledge".to_owned()]),
        "knowledge/z-focus.md",
        1,
        1,
        200,
    )
    .expect("request");
    let documents = vec![
        document(
            "knowledge/z-focus.md",
            "---\ntype: Research Claim\n---\n[neighbor](a-neighbor.md)\n",
        ),
        document(
            "knowledge/a-neighbor.md",
            "---\ntype: Research Claim\n---\n",
        ),
    ];

    let graph = project_okf_neighborhood(&documents, &request).expect("neighborhood");

    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.nodes[0].path.as_deref(), Some("knowledge/z-focus.md"));
    assert!(graph.edges.is_empty());
    assert!(graph.truncated);
}

#[test]
fn duplicate_source_ids_do_not_collapse_distinct_resources() {
    let request = RepositoryGraphNeighborhoodRequest::new(
        selector(vec![".".to_owned()]),
        "focus.md",
        1,
        100,
        200,
    )
    .expect("request");
    let documents = vec![document(
        "focus.md",
        r#"---
type: Research Claim
sources:
  - id: evidence
    resource: https://example.com/first
  - id: evidence
    resource: https://example.com/second
---
"#,
    )];

    let graph = project_okf_neighborhood(&documents, &request).expect("neighborhood");
    let source_edges = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == "cites_source")
        .collect::<Vec<_>>();

    assert_eq!(source_edges.len(), 2);
    assert_ne!(source_edges[0].id, source_edges[1].id);
    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|node| node.kind == "external_source")
            .count(),
        2
    );
    assert!(
        graph
            .nodes
            .iter()
            .filter(|node| node.kind == "external_source")
            .all(|node| { source_edges.iter().any(|edge| edge.target == node.id) })
    );
}

#[test]
fn concept_link_extraction_budget_marks_the_neighborhood_truncated() {
    let request = RepositoryGraphNeighborhoodRequest::new(
        selector(vec!["knowledge".to_owned()]),
        "knowledge/focus.md",
        1,
        100,
        200,
    )
    .expect("request");
    let body = (0..=MAX_CONCEPT_LINKS)
        .map(|index| format!("[concept {index}](concept-{index}.md)\n"))
        .collect::<String>();
    let documents = vec![document(
        "knowledge/focus.md",
        &format!("---\ntype: Research Claim\n---\n\n{body}"),
    )];

    let graph = project_okf_neighborhood(&documents, &request).expect("neighborhood");

    assert!(graph.truncated);
    assert_eq!(
        graph.nodes[0]
            .details
            .get("link_extraction_truncated")
            .map(String::as_str),
        Some("true")
    );
}

#[test]
fn omitted_reverse_link_from_an_unselected_concept_marks_truncation() {
    let request = RepositoryGraphNeighborhoodRequest::new(
        selector(vec!["knowledge".to_owned()]),
        "knowledge/focus.md",
        1,
        100,
        200,
    )
    .expect("request");
    let preceding_links = (0..MAX_CONCEPT_LINKS)
        .map(|index| format!("[concept {index}](concept-{index}.md)\n"))
        .collect::<String>();
    let documents = vec![
        document("knowledge/focus.md", "---\ntype: Research Claim\n---\n"),
        document(
            "knowledge/candidate.md",
            &format!("---\ntype: Research Claim\n---\n\n{preceding_links}[focus](focus.md)\n"),
        ),
    ];

    let graph = project_okf_neighborhood(&documents, &request).expect("neighborhood");

    assert_eq!(graph.nodes.len(), 1);
    assert!(graph.truncated);
}

#[test]
fn oversized_source_graph_bounds_parsing_selection_and_response_assembly() {
    let source_count = MAX_CONCEPT_SOURCES + 32;
    let sources = (0..source_count)
        .map(|index| format!("  - resource: ./target-{index:03}.md\n"))
        .collect::<String>();
    let mut documents = vec![document(
        "focus.md",
        &format!("---\ntype: Research Claim\nsources:\n{sources}---\n"),
    )];
    documents.extend((0..source_count).map(|index| {
        document(
            &format!("target-{index:03}.md"),
            "---\ntype: Research Claim\n---\n",
        )
    }));

    let concepts = documents
        .iter()
        .filter_map(|document| parse_concept(document, "."))
        .map(|concept| (concept.path.clone(), concept))
        .collect::<BTreeMap<_, _>>();
    let focus = &concepts["focus.md"];
    let selection = selected_concepts(&concepts, "focus.md", 1, 3);
    let request = RepositoryGraphNeighborhoodRequest::new(
        selector(vec![".".to_owned()]),
        "focus.md",
        1,
        3,
        2,
    )
    .expect("request");
    let graph = project_okf_neighborhood(&documents, &request).expect("neighborhood");

    assert_eq!(focus.sources.len(), MAX_CONCEPT_SOURCES);
    assert!(focus.truncated);
    assert_eq!(selection.paths.len(), 3);
    assert!(selection.truncated);
    assert_eq!(graph.nodes.len(), 3);
    assert_eq!(graph.edges.len(), 2);
    assert!(graph.truncated);
    assert_eq!(
        graph.nodes[0]
            .details
            .get("source_extraction_truncated")
            .map(String::as_str),
        Some("true")
    );
}

fn selector(path_filters: Vec<String>) -> CodeRepositorySelector {
    CodeRepositorySelector::new("stone-star", "HEAD", path_filters, Vec::new()).expect("selector")
}

fn document(path: &str, content: &str) -> IndexedRepositoryDocument {
    IndexedRepositoryDocument {
        path: path.to_owned(),
        language_id: "markdown".to_owned(),
        content: content.to_owned(),
    }
}
