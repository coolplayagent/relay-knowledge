use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{CodeImportRecord, CodebaseViewSnapshot};

use super::{
    builder::{SectionRefs, ViewBuilder},
    rules::{architecture_layer, layer_confidence},
};

pub(super) fn derive_architecture_layers(
    builder: &mut ViewBuilder,
    snapshot: &CodebaseViewSnapshot,
) {
    let indexed_paths = snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    let mut layer_candidates = BTreeMap::<String, (Vec<&str>, Vec<String>)>::new();
    for file in &snapshot.files {
        let layer = architecture_layer(&file.path);
        let evidence_id = builder.evidence(
            "file",
            &file.path,
            None,
            None,
            None,
            format!("{} file in {layer} layer", file.language_id),
        );
        let (files, evidence_ids) = layer_candidates.entry(layer.to_owned()).or_default();
        files.push(file.path.as_str());
        evidence_ids.push(evidence_id);
    }
    let mut ordered_layers = layer_candidates.into_iter().collect::<Vec<_>>();
    ordered_layers
        .sort_by_cached_key(|(layer, (files, _))| (std::cmp::Reverse(files.len()), layer.clone()));
    let mut layer_files = BTreeMap::<String, Vec<&str>>::new();
    let mut layer_evidence = BTreeMap::<String, Vec<String>>::new();
    if ordered_layers.len() > builder.limit {
        builder.mark_node_budget_truncated();
    }
    for (layer, (files, evidence_ids)) in ordered_layers.into_iter().take(builder.limit) {
        let Some(node_id) = builder.node(
            format!("layer:{layer}"),
            layer.clone(),
            "architecture_layer",
            None,
            layer_confidence(&layer),
            evidence_ids.first().cloned(),
        ) else {
            continue;
        };
        layer_files.insert(node_id.clone(), files);
        layer_evidence.insert(node_id, evidence_ids);
    }
    for import in &snapshot.imports {
        if let Some(target_path) = resolved_indexed_import_target(import, &indexed_paths) {
            let source = format!("layer:{}", architecture_layer(&import.path));
            let target = format!("layer:{}", architecture_layer(target_path));
            let evidence_id = builder.evidence(
                "import",
                &import.path,
                Some(import.module.clone()),
                Some(import.line_range.clone()),
                Some(import.resolution_state.clone()),
                "import edge between architecture layers",
            );
            let source_id = builder.node(
                source.clone(),
                source.trim_start_matches("layer:").to_owned(),
                "architecture_layer",
                None,
                0.74,
                Some(evidence_id.clone()),
            );
            let target_id = builder.node(
                target.clone(),
                target.trim_start_matches("layer:").to_owned(),
                "architecture_layer",
                None,
                0.74,
                Some(evidence_id.clone()),
            );
            if let (Some(source_id), Some(target_id)) = (source_id, target_id) {
                builder.edge(&source_id, &target_id, "imports", 0.72, Some(evidence_id));
            }
        }
    }
    for call in &snapshot.calls {
        if let Some(target_path) = call.callee_path.as_deref() {
            let source = format!("layer:{}", architecture_layer(&call.call.path));
            let target = format!("layer:{}", architecture_layer(target_path));
            let evidence_id = builder.evidence(
                "call",
                &call.call.path,
                call.call.caller_name.clone(),
                Some(call.call.line_range.clone()),
                Some(call.call.resolution_state.clone()),
                format!("call to {}", call.call.callee_name),
            );
            let source_id = builder.node(
                source.clone(),
                source.trim_start_matches("layer:").to_owned(),
                "architecture_layer",
                None,
                0.74,
                Some(evidence_id.clone()),
            );
            let target_id = builder.node(
                target.clone(),
                target.trim_start_matches("layer:").to_owned(),
                "architecture_layer",
                None,
                0.74,
                Some(evidence_id.clone()),
            );
            if let (Some(source_id), Some(target_id)) = (source_id, target_id) {
                builder.edge(&source_id, &target_id, "calls", 0.76, Some(evidence_id));
            }
        }
    }
    let mut ordered_layers = layer_files.into_iter().collect::<Vec<_>>();
    ordered_layers
        .sort_by(|left, right| right.1.len().cmp(&left.1.len()).then(left.0.cmp(&right.0)));
    for (node_id, files) in ordered_layers.into_iter().take(builder.limit) {
        let layer = node_id.trim_start_matches("layer:");
        let evidence_ids = layer_evidence.remove(&node_id).unwrap_or_default();
        builder.section(
            format!("section:{node_id}"),
            format!("{layer} layer"),
            format!(
                "{layer} contains {} indexed file(s) and is derived from path and graph boundary evidence.",
                files.len()
            ),
            layer_confidence(layer),
            SectionRefs {
                node_ids: vec![node_id],
                evidence_ids,
                ..SectionRefs::default()
            },
        );
    }
}

fn resolved_indexed_import_target<'a>(
    import: &'a CodeImportRecord,
    indexed_paths: &BTreeSet<&str>,
) -> Option<&'a str> {
    let target_path = import.target_hint.as_deref()?;
    (import.resolution_state == "resolved" && indexed_paths.contains(target_path))
        .then_some(target_path)
}
