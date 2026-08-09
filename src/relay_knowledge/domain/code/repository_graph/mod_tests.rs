use super::{
    IndexedRepositoryDocument, RepositoryGraphNeighborhoodRequest, project_okf_neighborhood,
};
use crate::domain::CodeRepositorySelector;

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

fn document(path: &str, content: &str) -> IndexedRepositoryDocument {
    IndexedRepositoryDocument {
        path: path.to_owned(),
        language_id: "markdown".to_owned(),
        content: content.to_owned(),
    }
}
