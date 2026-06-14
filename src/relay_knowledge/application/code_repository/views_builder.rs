use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{
    CodebaseViewBudget, CodebaseViewEdge, CodebaseViewEvidence, CodebaseViewNode,
    CodebaseViewSection, RepositoryCodeRange,
};

pub(super) struct DerivedView {
    pub(super) nodes: Vec<CodebaseViewNode>,
    pub(super) edges: Vec<CodebaseViewEdge>,
    pub(super) sections: Vec<CodebaseViewSection>,
    pub(super) evidence: Vec<CodebaseViewEvidence>,
    pub(super) budget: CodebaseViewBudget,
    pub(super) diagnostics: Vec<String>,
}

pub(super) struct ViewBuilder {
    pub(super) limit: usize,
    nodes: Vec<CodebaseViewNode>,
    edges: Vec<CodebaseViewEdge>,
    sections: Vec<CodebaseViewSection>,
    evidence: Vec<CodebaseViewEvidence>,
    diagnostics: Vec<String>,
    node_index: BTreeMap<String, usize>,
    edge_ids: BTreeSet<String>,
    budget: CodebaseViewBudget,
}

impl ViewBuilder {
    pub(super) fn new(limit: usize, row_limit: usize, snapshot_truncated: bool) -> Self {
        Self {
            limit,
            nodes: Vec::new(),
            edges: Vec::new(),
            sections: Vec::new(),
            evidence: Vec::new(),
            diagnostics: Vec::new(),
            node_index: BTreeMap::new(),
            edge_ids: BTreeSet::new(),
            budget: CodebaseViewBudget::new(limit, row_limit, snapshot_truncated),
        }
    }

    pub(super) fn evidence(
        &mut self,
        kind: &str,
        path: &str,
        symbol: Option<String>,
        line_range: Option<RepositoryCodeRange>,
        edge_kind: Option<String>,
        detail: impl Into<String>,
    ) -> String {
        let id = format!("evidence:{}", self.evidence.len() + 1);
        self.evidence.push(CodebaseViewEvidence {
            id: id.clone(),
            evidence_kind: kind.to_owned(),
            path: path.to_owned(),
            symbol,
            line_range,
            edge_kind,
            retrieval_layer: None,
            detail: detail.into(),
        });
        id
    }

    pub(super) fn node(
        &mut self,
        id: String,
        label: String,
        node_kind: &str,
        path: Option<String>,
        confidence: f64,
        evidence_id: Option<String>,
    ) -> Option<String> {
        if let Some(index) = self.node_index.get(&id).copied() {
            if let Some(evidence_id) = evidence_id {
                let evidence_ids = &mut self.nodes[index].evidence_ids;
                if !evidence_ids.contains(&evidence_id) {
                    evidence_ids.push(evidence_id);
                }
            }
            return Some(id);
        }
        if self.nodes.len() >= self.limit {
            self.budget.nodes_truncated = true;
            return None;
        }
        let evidence_ids = evidence_id.into_iter().collect();
        self.node_index.insert(id.clone(), self.nodes.len());
        self.nodes.push(CodebaseViewNode {
            id: id.clone(),
            label,
            node_kind: node_kind.to_owned(),
            path,
            confidence,
            evidence_ids,
        });
        Some(id)
    }

    pub(super) fn existing_node_id(&self, id: String) -> Option<String> {
        self.node_index.contains_key(&id).then_some(id)
    }

    pub(super) fn can_insert_nodes(&self, count: usize) -> bool {
        self.nodes.len().saturating_add(count) <= self.limit
    }

    pub(super) fn mark_node_budget_truncated(&mut self) {
        self.budget.nodes_truncated = true;
        self.budget.sections_truncated = true;
    }

    pub(super) fn mark_edge_budget_truncated(&mut self) {
        self.budget.nodes_truncated = true;
        self.budget.edges_truncated = true;
    }

    pub(super) fn edge(
        &mut self,
        source_id: &str,
        target_id: &str,
        edge_kind: &str,
        confidence: f64,
        evidence_id: Option<String>,
    ) -> Option<String> {
        if source_id == target_id {
            return None;
        }
        if !self.node_index.contains_key(source_id) || !self.node_index.contains_key(target_id) {
            return None;
        }
        let id = format!("{edge_kind}:{source_id}->{target_id}");
        if self.edge_ids.contains(&id) {
            if let Some(evidence_id) = evidence_id
                && let Some(edge) = self.edges.iter_mut().find(|edge| edge.id == id)
                && !edge.evidence_ids.contains(&evidence_id)
            {
                edge.evidence_ids.push(evidence_id);
            }
            return Some(id);
        }
        if self.edges.len() >= self.limit {
            self.budget.edges_truncated = true;
            return None;
        }
        self.edge_ids.insert(id.clone());
        self.edges.push(CodebaseViewEdge {
            id: id.clone(),
            source_id: source_id.to_owned(),
            target_id: target_id.to_owned(),
            edge_kind: edge_kind.to_owned(),
            confidence,
            evidence_ids: evidence_id.into_iter().collect(),
        });
        Some(id)
    }

    pub(super) fn section(
        &mut self,
        id: String,
        title: String,
        narrative: String,
        confidence: f64,
        refs: SectionRefs,
    ) {
        if self.sections.len() >= self.limit {
            self.budget.sections_truncated = true;
            return;
        }
        self.sections.push(CodebaseViewSection {
            id,
            title,
            narrative,
            confidence,
            node_ids: refs.node_ids,
            edge_ids: refs.edge_ids,
            evidence_ids: refs.evidence_ids,
            diagnostics: refs.diagnostics,
        });
    }

    pub(super) fn diagnostic(&mut self, diagnostic: impl Into<String>) {
        self.diagnostics.push(diagnostic.into());
    }

    pub(super) fn finish(mut self) -> DerivedView {
        if self.sections.is_empty() {
            let diagnostic = "no indexed graph evidence matched the requested view".to_owned();
            self.diagnostic(diagnostic.clone());
            self.section(
                "section:empty".to_owned(),
                "No matching view evidence".to_owned(),
                "No section was derived because the indexed graph snapshot had no matching evidence."
                    .to_owned(),
                0.0,
                SectionRefs {
                    diagnostics: vec![diagnostic],
                    ..SectionRefs::default()
                },
            );
        }
        self.prune_evidence();
        DerivedView {
            nodes: self.nodes,
            edges: self.edges,
            sections: self.sections,
            evidence: self.evidence,
            budget: self.budget,
            diagnostics: self.diagnostics,
        }
    }

    fn prune_evidence(&mut self) {
        let evidence_limit = self.limit.saturating_mul(4).max(self.limit);
        let referenced = self.referenced_evidence_ids();
        let mut retained_ids = BTreeSet::new();
        let mut retained = Vec::new();
        for evidence in self.evidence.drain(..) {
            if referenced.contains(&evidence.id) && retained.len() < evidence_limit {
                retained_ids.insert(evidence.id.clone());
                retained.push(evidence);
            }
        }
        if retained_ids.len() < referenced.len() {
            self.budget.evidence_truncated = true;
        }
        self.evidence = retained;
        for node in &mut self.nodes {
            node.evidence_ids
                .retain(|evidence_id| retained_ids.contains(evidence_id));
        }
        for edge in &mut self.edges {
            edge.evidence_ids
                .retain(|evidence_id| retained_ids.contains(evidence_id));
        }
        for section in &mut self.sections {
            section
                .evidence_ids
                .retain(|evidence_id| retained_ids.contains(evidence_id));
        }
    }

    fn referenced_evidence_ids(&self) -> BTreeSet<String> {
        let mut referenced = BTreeSet::new();
        for node in &self.nodes {
            referenced.extend(node.evidence_ids.iter().cloned());
        }
        for edge in &self.edges {
            referenced.extend(edge.evidence_ids.iter().cloned());
        }
        for section in &self.sections {
            referenced.extend(section.evidence_ids.iter().cloned());
        }
        referenced
    }
}

#[derive(Default)]
pub(super) struct SectionRefs {
    pub(super) node_ids: Vec<String>,
    pub(super) edge_ids: Vec<String>,
    pub(super) evidence_ids: Vec<String>,
    pub(super) diagnostics: Vec<String>,
}
