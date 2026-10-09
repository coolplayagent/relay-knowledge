//! Bounded authored graph validation and source-specific impact.

use super::{
    audit::{ResearchDiagnostic, audit_artifact, diagnostic},
    reader::{ResearchReader, digest},
};
use crate::{
    api::ApiError,
    domain::research::{AuthoredEvidenceBundle, EvidenceInterpretation},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone, Serialize)]
pub struct AuthoredRelationAudit {
    pub index: usize,
    pub id: String,
    pub state: String,
    pub interpretations: Vec<EvidenceInterpretation>,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthoredBundleAudit {
    pub input_sha256: String,
    pub bundle_sha256: String,
    pub bundle_id: String,
    pub source_scope: String,
    pub valid: bool,
    pub import_state: String,
    pub stored_fact_status: Option<crate::domain::FactStatus>,
    pub node_count: usize,
    pub edge_count: usize,
    pub relations: Vec<AuthoredRelationAudit>,
    pub diagnostics: Vec<ResearchDiagnostic>,
}

pub(super) struct LoadedBundle {
    pub bundle: AuthoredEvidenceBundle,
    pub report: AuthoredBundleAudit,
    pub snippets: BTreeMap<String, String>,
    pub source_paths: BTreeMap<String, String>,
}

pub(super) fn load_bundle(
    reader: &mut ResearchReader,
    input: &Path,
    scope: &str,
) -> Result<LoadedBundle, ApiError> {
    let (bundle, input_sha256): (AuthoredEvidenceBundle, _) = reader.json(input)?;
    bundle
        .validate_shape()
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    if bundle.source_scope != scope {
        return Err(ApiError::invalid_argument(
            "bundle source_scope does not match the explicitly authorized scope",
        ));
    }
    let payload = serde_json::to_vec(&bundle)
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let mut report = AuthoredBundleAudit {
        input_sha256,
        bundle_sha256: digest(&payload),
        bundle_id: bundle.id.clone(),
        source_scope: scope.into(),
        valid: true,
        import_state: "not_checked".into(),
        stored_fact_status: None,
        node_count: bundle.graph.nodes.len(),
        edge_count: bundle.graph.edges.len(),
        relations: Vec::new(),
        diagnostics: Vec::new(),
    };
    let mut node_ids = BTreeSet::new();
    for node in &bundle.graph.nodes {
        if !node_ids.insert(&node.id) {
            report.diagnostics.push(diagnostic(
                &node.id,
                "duplicate_node",
                "node id is not unique",
                "Use stable distinct concept ids.",
            ));
        }
    }
    let mut pins = BTreeMap::new();
    let mut snippets = BTreeMap::new();
    let mut source_paths = BTreeMap::new();
    let mut snippet_bytes = 0;
    for pin in &bundle.evidence {
        if pins.insert(&pin.id, pin).is_some() {
            report.diagnostics.push(diagnostic(
                &pin.id,
                "duplicate_evidence",
                "evidence id is not unique",
                "Assign distinct evidence ids.",
            ));
        }
        if pin.source_scope != scope {
            report.diagnostics.push(diagnostic(
                &pin.id,
                "scope_violation",
                "evidence belongs to another scope",
                "Supply a bundle confined to the authorized source scope.",
            ));
            continue;
        }
        let (audit, bytes) = audit_artifact(
            reader,
            input,
            &pin.artifact,
            &pin.id,
            &mut report.diagnostics,
        );
        if audit.state != "verified" {
            continue;
        }
        let bytes = bytes.expect("verified artifacts have bytes");
        let excerpt = if let Some(span) = pin.span {
            let Some(excerpt) = bytes.get(span.start_byte as usize..span.end_byte as usize) else {
                report.diagnostics.push(diagnostic(
                    &pin.id,
                    "invalid_span",
                    "evidence byte span is outside its pinned artifact",
                    "Rebind the exact evidence span to this content version.",
                ));
                continue;
            };
            let start_line = bytes[..span.start_byte as usize]
                .iter()
                .filter(|b| **b == b'\n')
                .count()
                + 1;
            let end_line = bytes[..span.end_byte as usize - 1]
                .iter()
                .filter(|b| **b == b'\n')
                .count()
                + 1;
            if start_line != span.start_line as usize || end_line != span.end_line as usize {
                report.diagnostics.push(diagnostic(
                    &pin.id,
                    "invalid_span_lines",
                    "line coordinates do not match the byte span",
                    "Correct line coordinates without changing the original bytes.",
                ));
                continue;
            }
            excerpt
        } else {
            bytes.as_slice()
        };
        let text = if excerpt.len() <= 65536 {
            std::str::from_utf8(excerpt).ok().map(str::to_owned)
        } else {
            None
        };
        let content = serde_json::to_string(&serde_json::json!({"pin":pin,"text":text,"capture":if text.is_some() {"excerpt"} else {"hash_only"}})).map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        snippet_bytes += content.len();
        if snippet_bytes > 2 * 1024 * 1024 {
            return Err(ApiError::invalid_argument(
                "bundle evidence projection exceeds 2 MiB; use bounded spans",
            ));
        }
        let source_path = match pin.artifact.path_base {
            crate::domain::research::ResearchPathBase::Repository => {
                std::path::PathBuf::from(&pin.artifact.path)
            }
            crate::domain::research::ResearchPathBase::Catalog => input
                .parent()
                .unwrap_or(Path::new(""))
                .join(&pin.artifact.path),
        };
        source_paths.insert(
            pin.id.clone(),
            source_path.to_string_lossy().replace('\\', "/"),
        );
        snippets.insert(pin.id.clone(), content);
    }
    let mut edge_ids = BTreeSet::new();
    for (index, edge) in bundle.graph.edges.iter().enumerate() {
        let id = edge_identity(edge)?;
        let mut valid = true;
        if !edge_ids.insert(id.clone()) {
            valid = false;
            report.diagnostics.push(diagnostic(
                &id,
                "duplicate_relation",
                "relation identity is duplicated",
                "Provide distinct explicit relation ids for parallel claims.",
            ));
        }
        if !node_ids.contains(&edge.source) || !node_ids.contains(&edge.target) {
            valid = false;
            report.diagnostics.push(diagnostic(
                &id,
                "dangling_edge",
                "relation endpoint is absent",
                "Add the stable endpoint or explicitly revise the relation.",
            ));
        }
        if edge.evidence.is_empty() || edge.evidence.iter().any(|id| !snippets.contains_key(id)) {
            valid = false;
            report.diagnostics.push(diagnostic(
                &id,
                "missing_or_changed_evidence",
                "relation lacks valid, in-scope pinned evidence",
                "Repair the listed pins; only dependent relations require renewed review.",
            ));
        }
        report.relations.push(AuthoredRelationAudit {
            index,
            id,
            state: if valid { "proposed" } else { "needs_review" }.into(),
            interpretations: edge
                .evidence
                .iter()
                .filter_map(|id| pins.get(id).map(|pin| pin.interpretation))
                .collect(),
            evidence_ids: edge.evidence.clone(),
        });
    }
    report.valid = report.diagnostics.is_empty();
    Ok(LoadedBundle {
        bundle,
        report,
        snippets,
        source_paths,
    })
}

pub(super) fn edge_identity(
    edge: &crate::domain::research::AuthoredEvidenceEdge,
) -> Result<String, ApiError> {
    if let Some(id) = &edge.id {
        return Ok(id.clone());
    }
    let identity = serde_json::to_vec(&(
        &edge.source,
        &edge.relation,
        &edge.target,
        edge.metadata.get("qualifiers"),
    ))
    .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    Ok(digest(&identity))
}

pub(super) fn document_id(bundle_id: &str, scope: &str, revision: &str) -> String {
    let identity = serde_json::to_vec(&(scope, bundle_id)).expect("string identity serializes");
    format!("authored-bundle:{}:{revision}", digest(&identity))
}

#[cfg(test)]
#[path = "bundle_tests.rs"]
mod tests;
