pub(crate) fn apply_candidate_documentation_gate(
    evaluation: &mut evaluator::EvaluationRun,
    patch: &PatchSnapshot,
) {
    let changed_paths = git_ops::changed_paths_from_diff(&patch.diff);
    let requires_docs = changed_paths
        .iter()
        .any(|path| !path.starts_with("docs/") && !path.ends_with(".md"));
    let documentation_coverage = self_iteration_documentation_gate_coverage(&changed_paths);
    let gate = GateObservation {
        name: "self_iteration_algorithm_documentation".to_owned(),
        passed: !requires_docs || documentation_coverage.is_some(),
        duration_ms: 0,
        message: if !requires_docs {
            "documentation not required for documentation-only candidate".to_owned()
        } else if matches!(
            documentation_coverage,
            Some(DocumentationGateCoverage::Algorithm)
        ) {
            "self-iteration algorithm documentation updated".to_owned()
        } else if matches!(
            documentation_coverage,
            Some(DocumentationGateCoverage::EvaluationSet)
        ) {
            "self-iteration evaluation-set documentation updated".to_owned()
        } else {
            "missing candidate algorithm and architecture notes".to_owned()
        },
    };
    evaluation.observation.gates.push(gate.clone());
    if let Some(gates) = evaluation
        .report
        .get_mut("gates")
        .and_then(serde_json::Value::as_array_mut)
    {
        gates.push(serde_json::to_value(gate).expect("gate should serialize"));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocumentationGateCoverage {
    Algorithm,
    EvaluationSet,
}

fn self_iteration_documentation_gate_coverage(
    changed_paths: &[String],
) -> Option<DocumentationGateCoverage> {
    if changed_paths
        .iter()
        .any(|path| self_iteration_algorithm_documentation_path(path))
    {
        return Some(DocumentationGateCoverage::Algorithm);
    }
    if changed_paths
        .iter()
        .any(|path| self_iteration_evaluation_set_documentation_path(path))
        && changed_paths
            .iter()
            .all(|path| self_iteration_evaluation_set_change_path(path))
    {
        return Some(DocumentationGateCoverage::EvaluationSet);
    }
    None
}

fn self_iteration_algorithm_documentation_path(path: &str) -> bool {
    matches!(
        path,
        "docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md"
    )
}

fn self_iteration_evaluation_set_documentation_path(path: &str) -> bool {
    matches!(
        path,
        "docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md"
            | "docs/zh/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md"
    )
}

fn self_iteration_evaluation_set_change_path(path: &str) -> bool {
    path == "tools/self_iteration/cases.json"
        || path.starts_with("tools/self_iteration/cases/")
        || path.starts_with("docs/")
        || path.ends_with(".md")
}
