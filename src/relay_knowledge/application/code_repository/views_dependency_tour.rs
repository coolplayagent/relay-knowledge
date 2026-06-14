use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{CodeImportRecord, CodebaseViewDependency, CodebaseViewSnapshot};

use super::{
    views_builder::{SectionRefs, ViewBuilder},
    views_rules::{module_key, topological_tour},
};

pub(super) fn derive_dependency_tour(builder: &mut ViewBuilder, snapshot: &CodebaseViewSnapshot) {
    let mut modules = BTreeSet::<String>::new();
    let mut graph = BTreeMap::<String, BTreeSet<String>>::new();
    let mut edge_evidence = Vec::new();
    let indexed_paths = snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    for file in &snapshot.files {
        modules.insert(module_key(&file.path));
    }
    for import in &snapshot.imports {
        if let Some(target_path) = resolved_indexed_import_target(import, &indexed_paths) {
            let source = module_key(&import.path);
            let target = module_key(target_path);
            modules.insert(source.clone());
            modules.insert(target.clone());
            graph
                .entry(source.clone())
                .or_default()
                .insert(target.clone());
            let evidence_id = builder.evidence(
                "import",
                &import.path,
                Some(import.module.clone()),
                Some(import.line_range.clone()),
                Some(import.resolution_state.clone()),
                "dependency tour import edge",
            );
            edge_evidence.push((source, target, evidence_id));
        }
    }
    for call in &snapshot.calls {
        if let Some(target_path) = call.callee_path.as_deref() {
            let source = module_key(&call.call.path);
            let target = module_key(target_path);
            modules.insert(source.clone());
            modules.insert(target.clone());
            graph
                .entry(source.clone())
                .or_default()
                .insert(target.clone());
            let evidence_id = builder.evidence(
                "call",
                &call.call.path,
                call.call.caller_name.clone(),
                Some(call.call.line_range.clone()),
                Some(call.call.resolution_state.clone()),
                "dependency tour call edge",
            );
            edge_evidence.push((source, target, evidence_id));
        }
    }
    let package_evidence = collect_package_evidence(builder, &snapshot.dependencies, &mut modules);
    let (tour, cycle) = topological_tour(&modules, &graph);
    let package_count = unique_package_count(&package_evidence);
    if tour.len().saturating_add(package_count) > builder.limit {
        builder.mark_node_budget_truncated();
    }
    let package_slots = reserved_package_slots(builder.limit, package_count);
    let package_source_modules = selected_package_source_modules(&package_evidence, package_slots);
    let selected_modules = selected_tour_modules(
        &tour,
        builder.limit.saturating_sub(package_slots),
        &package_source_modules,
    );
    for module in &selected_modules {
        builder.node(
            format!("module:{module}"),
            module.clone(),
            "module",
            None,
            0.70,
            None,
        );
    }
    let mut node_ids = selected_modules
        .iter()
        .map(|module| format!("module:{module}"))
        .collect::<Vec<_>>();
    let selected_node_ids = node_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut edge_ids = module_edge_ids(builder, edge_evidence, &selected_node_ids);
    let mut inserted_package_ids = BTreeSet::new();
    for package in package_evidence {
        let package_id = package_node_id(package.dependency);
        let package_seen = inserted_package_ids.contains(&package_id);
        if !package_seen && node_ids.len() >= builder.limit {
            break;
        }
        let Some(node_id) = builder.node(
            package_id.clone(),
            package.dependency.package_name.clone(),
            "package",
            Some(package.dependency.path.clone()),
            0.66,
            Some(package.evidence_id.clone()),
        ) else {
            break;
        };
        let source_id = format!("module:{}", package.source_module);
        if let Some(edge_id) = builder.edge(
            &source_id,
            &node_id,
            "depends_on",
            0.66,
            Some(package.evidence_id),
        ) {
            push_unique_edge_id(&mut edge_ids, edge_id);
        }
        if !package_seen {
            inserted_package_ids.insert(package_id);
            node_ids.push(node_id);
        }
    }
    let mut diagnostics = Vec::new();
    if cycle {
        diagnostics.push(
            "dependency cycle detected; tour starts with the lowest-indegree modules".to_owned(),
        );
    }
    if selected_modules.len() < tour.len()
        || node_ids.len() < tour.len().saturating_add(package_count)
    {
        diagnostics
            .push("dependency tour truncated to returned module and package nodes".to_owned());
    }
    let narrative = if node_ids.is_empty() {
        "No dependency tour was derived because no dependency evidence was indexed.".to_owned()
    } else {
        let labels = node_ids
            .iter()
            .map(|id| {
                id.trim_start_matches("module:")
                    .trim_start_matches("package:")
            })
            .collect::<Vec<_>>();
        format!("Suggested tour order: {}.", labels.join(" -> "))
    };
    builder.section(
        "section:dependency_tour".to_owned(),
        "Dependency tour".to_owned(),
        narrative,
        if cycle { 0.45 } else { 0.72 },
        SectionRefs {
            node_ids,
            edge_ids,
            diagnostics,
            ..SectionRefs::default()
        },
    );
}

fn resolved_indexed_import_target<'a>(
    import: &'a CodeImportRecord,
    indexed_paths: &BTreeSet<&str>,
) -> Option<&'a str> {
    let target_path = import.target_hint.as_deref()?;
    (import.resolution_state == "resolved" && indexed_paths.contains(target_path))
        .then_some(target_path)
}

struct PackageEvidence<'a> {
    dependency: &'a CodebaseViewDependency,
    source_module: String,
    evidence_id: String,
}

fn collect_package_evidence<'a>(
    builder: &mut ViewBuilder,
    dependencies: &'a [CodebaseViewDependency],
    modules: &mut BTreeSet<String>,
) -> Vec<PackageEvidence<'a>> {
    let mut packages = Vec::new();
    for dependency in dependencies {
        let source_module = dependency_source_module(&dependency.path);
        modules.insert(source_module.clone());
        let evidence_id = builder.evidence(
            "dependency",
            &dependency.path,
            Some(dependency.package_name.clone()),
            Some(dependency.line_range.clone()),
            Some(dependency.source_kind.clone()),
            format!(
                "{} {} dependency {}",
                dependency.ecosystem, dependency.dependency_group, dependency.package_name
            ),
        );
        packages.push(PackageEvidence {
            dependency,
            source_module,
            evidence_id,
        });
    }
    packages
}

fn unique_package_count(package_evidence: &[PackageEvidence<'_>]) -> usize {
    package_evidence
        .iter()
        .map(|package| package_node_id(package.dependency))
        .collect::<BTreeSet<_>>()
        .len()
}

fn reserved_package_slots(limit: usize, package_count: usize) -> usize {
    if package_count == 0 || limit < 2 {
        return 0;
    }
    package_count.min((limit / 4).max(1))
}

fn selected_package_source_modules(
    package_evidence: &[PackageEvidence<'_>],
    package_slots: usize,
) -> BTreeSet<String> {
    let mut selected_package_ids = BTreeSet::new();
    let mut source_modules = BTreeSet::new();
    for package in package_evidence {
        let package_id = package_node_id(package.dependency);
        if selected_package_ids.contains(&package_id) {
            continue;
        }
        if selected_package_ids.len() >= package_slots {
            break;
        }
        selected_package_ids.insert(package_id);
        source_modules.insert(package.source_module.clone());
    }
    source_modules
}

fn selected_tour_modules(
    tour: &[String],
    module_limit: usize,
    required_modules: &BTreeSet<String>,
) -> Vec<String> {
    if module_limit == 0 {
        return Vec::new();
    }
    let mut selected = tour
        .iter()
        .take(module_limit)
        .cloned()
        .collect::<BTreeSet<_>>();
    for module in required_modules {
        if selected.contains(module) {
            continue;
        }
        while selected.len() >= module_limit {
            let Some(removable) = tour
                .iter()
                .rev()
                .find(|candidate| {
                    selected.contains(*candidate) && !required_modules.contains(*candidate)
                })
                .cloned()
            else {
                break;
            };
            selected.remove(&removable);
        }
        if selected.len() < module_limit {
            selected.insert(module.clone());
        }
    }
    tour.iter()
        .filter(|module| selected.contains(*module))
        .cloned()
        .collect()
}

fn module_edge_ids(
    builder: &mut ViewBuilder,
    edge_evidence: Vec<(String, String, String)>,
    selected_node_ids: &BTreeSet<String>,
) -> Vec<String> {
    let mut edge_ids = Vec::new();
    for (source, target, evidence_id) in edge_evidence {
        let source_id = format!("module:{source}");
        let target_id = format!("module:{target}");
        if !selected_node_ids.contains(&source_id) || !selected_node_ids.contains(&target_id) {
            continue;
        }
        if let Some(edge_id) = builder.edge(
            &source_id,
            &target_id,
            "depends_on",
            0.70,
            Some(evidence_id),
        ) {
            push_unique_edge_id(&mut edge_ids, edge_id);
        }
    }
    edge_ids
}

fn push_unique_edge_id(edge_ids: &mut Vec<String>, edge_id: String) {
    if !edge_ids.contains(&edge_id) {
        edge_ids.push(edge_id);
    }
}

fn dependency_source_module(path: &str) -> String {
    let Some((parent, _file_name)) = path.rsplit_once('/') else {
        return "root".to_owned();
    };
    if parent.is_empty() {
        "root".to_owned()
    } else {
        module_key(parent)
    }
}

fn package_node_id(dependency: &CodebaseViewDependency) -> String {
    format!(
        "package:{}-{}",
        escaped_package_key_part(&dependency.ecosystem),
        escaped_package_key_part(&dependency.package_name)
    )
}

fn escaped_package_key_part(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' {
            escaped.push(char::from(byte));
        } else {
            escaped.push('~');
            escaped.push(hex_digit(byte >> 4));
            escaped.push(hex_digit(byte & 0x0f));
        }
    }
    escaped
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        10..=15 => char::from(b'A' + value - 10),
        _ => unreachable!("hex digit input is masked to four bits"),
    }
}
