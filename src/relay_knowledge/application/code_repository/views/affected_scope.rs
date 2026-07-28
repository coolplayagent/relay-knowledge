use std::collections::BTreeSet;

use crate::domain::{CodebaseViewCall, CodebaseViewRequest, CodebaseViewSnapshot};

use super::{
    builder::{SectionRefs, ViewBuilder},
    rules::{
        affected_candidate_matches_changed_path, is_test_config_or_doc, module_key,
        normalized_view_paths,
    },
};

pub(super) fn derive_affected_scope(
    builder: &mut ViewBuilder,
    request: &CodebaseViewRequest,
    snapshot: &CodebaseViewSnapshot,
) {
    if request.changed_paths.is_empty() {
        let diagnostic =
            "affected_scope requires one or more --changed-path values in deterministic v1"
                .to_owned();
        builder.diagnostic(diagnostic.clone());
        builder.section(
            "section:affected_scope:missing_changes".to_owned(),
            "Affected scope needs changed paths".to_owned(),
            "No affected scope was derived because changed paths were not provided.".to_owned(),
            0.0,
            SectionRefs {
                diagnostics: vec![diagnostic],
                ..SectionRefs::default()
            },
        );
        return;
    }
    let changed_paths = normalized_view_paths(&request.changed_paths);
    if changed_paths.is_empty() {
        let diagnostic =
            "affected_scope requires one or more --changed-path values in deterministic v1"
                .to_owned();
        builder.diagnostic(diagnostic.clone());
        builder.section(
            "section:affected_scope:missing_changes".to_owned(),
            "Affected scope needs changed paths".to_owned(),
            "No affected scope was derived because changed paths were not provided.".to_owned(),
            0.0,
            SectionRefs {
                diagnostics: vec![diagnostic],
                ..SectionRefs::default()
            },
        );
        return;
    }
    let changed = changed_paths.iter().cloned().collect::<BTreeSet<_>>();
    let affected_calls = snapshot
        .calls
        .iter()
        .filter(|call| affected_call_matches_changed_paths(call, &changed, &changed_paths))
        .collect::<Vec<_>>();
    let verification_candidates = snapshot
        .files
        .iter()
        .filter(|file| is_test_config_or_doc(&file.path))
        .filter(|file| {
            changed_paths.iter().any(|changed_path| {
                affected_candidate_matches_changed_path(changed_path, &file.path)
            })
        })
        .collect::<Vec<_>>();
    let has_derived_scope = !affected_calls.is_empty() || !verification_candidates.is_empty();
    let changed_file_node_limit =
        affected_changed_file_node_limit(builder.limit, changed_paths.len(), has_derived_scope);
    let mut node_ids = Vec::new();
    let mut edge_ids = Vec::new();
    let mut evidence_ids = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, path) in changed_paths.iter().enumerate() {
        let evidence_id = builder.evidence("changed_path", path, None, None, None, "changed input");
        if index < changed_file_node_limit {
            let node_id = builder.node(
                format!("file:{path}"),
                path.clone(),
                "changed_file",
                Some(path.clone()),
                0.90,
                Some(evidence_id.clone()),
            );
            if let Some(node_id) = node_id {
                node_ids.push(node_id);
            }
        }
        evidence_ids.push(evidence_id);
    }
    if changed_file_node_limit < changed_paths.len() {
        builder.mark_node_budget_truncated();
        diagnostics.push(format!(
            "changed file nodes summarized from {} paths to preserve affected modules and verification candidates",
            changed_paths.len()
        ));
    }
    for call in affected_calls {
        let target_path = call
            .callee_path
            .clone()
            .unwrap_or_else(|| call.call.path.clone());
        let evidence_id = builder.evidence(
            "call",
            &call.call.path,
            call.call.caller_name.clone(),
            Some(call.call.line_range.clone()),
            Some(call.call.resolution_state.clone()),
            format!("affected call to {}", call.call.callee_name),
        );
        let source_id = builder.node(
            format!("module:{}", module_key(&call.call.path)),
            module_key(&call.call.path),
            "affected_module",
            Some(call.call.path.clone()),
            0.70,
            Some(evidence_id.clone()),
        );
        let target_id = builder.node(
            format!("module:{}", module_key(&target_path)),
            module_key(&target_path),
            "affected_module",
            Some(target_path),
            0.70,
            Some(evidence_id.clone()),
        );
        if let (Some(source_id), Some(target_id)) = (&source_id, &target_id) {
            if let Some(edge_id) = builder.edge(
                source_id,
                target_id,
                "affected_call",
                0.70,
                Some(evidence_id),
            ) {
                edge_ids.push(edge_id);
            }
        }
        node_ids.extend([source_id, target_id].into_iter().flatten());
    }
    for file in verification_candidates.into_iter().take(builder.limit) {
        let evidence_id = builder.evidence(
            "candidate",
            &file.path,
            None,
            None,
            None,
            "test, configuration, or documentation candidate in changed module",
        );
        let node_id = builder.node(
            format!("candidate:{}", file.path),
            file.path.clone(),
            "verification_candidate",
            Some(file.path.clone()),
            0.62,
            Some(evidence_id.clone()),
        );
        if let Some(node_id) = node_id {
            node_ids.push(node_id);
        }
        evidence_ids.push(evidence_id);
    }
    node_ids.sort();
    node_ids.dedup();
    builder.section(
        "section:affected_scope".to_owned(),
        "Affected scope".to_owned(),
        format!(
            "Affected scope was derived from {} changed path(s), call edges, and nearby verification candidates.",
            changed_paths.len()
        ),
        0.68,
        SectionRefs {
            node_ids,
            edge_ids,
            evidence_ids,
            diagnostics,
        },
    );
}

fn affected_call_matches_changed_paths(
    call: &CodebaseViewCall,
    changed: &BTreeSet<String>,
    changed_paths: &[String],
) -> bool {
    changed.contains(&call.call.path)
        || path_matches_changed_prefix(&call.call.path, changed_paths)
        || call.callee_path.as_ref().is_some_and(|path| {
            changed.contains(path) || path_matches_changed_prefix(path, changed_paths)
        })
}

fn affected_changed_file_node_limit(
    node_limit: usize,
    changed_path_count: usize,
    has_derived_scope: bool,
) -> usize {
    if !has_derived_scope {
        return changed_path_count.min(node_limit);
    }
    changed_path_count.min((node_limit / 3).max(1))
}

fn path_matches_changed_prefix(path: &str, changed_paths: &[String]) -> bool {
    changed_paths.iter().any(|changed_path| {
        path.strip_prefix(changed_path)
            .is_some_and(|tail| tail.starts_with('/'))
    })
}
