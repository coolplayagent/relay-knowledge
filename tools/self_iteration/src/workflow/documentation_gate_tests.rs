use super::*;

#[test]
fn documentation_gate_limits_evaluation_specs_to_eval_only_changes() {
    assert!(self_iteration_algorithm_documentation_path(
        "docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md"
    ));
    assert!(!self_iteration_algorithm_documentation_path(
        "docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md"
    ));
    assert!(self_iteration_evaluation_set_documentation_path(
        "docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md"
    ));
    assert!(self_iteration_evaluation_set_documentation_path(
        "docs/zh/05-benchmarks/07-multilingual-syntax-self-iteration-evaluation.md"
    ));
    assert!(!self_iteration_algorithm_documentation_path(
        "docs/zh/05-benchmarks/05-competitive-performance-benchmark-targets-2026-05-17.md"
    ));

    assert_eq!(
        self_iteration_documentation_gate_coverage(&[
            "tools/self_iteration/src/evaluator_tail.rs".to_owned(),
            "docs/zh/05-benchmarks/04-self-iteration-accepted-optimizations.md".to_owned()
        ]),
        Some(DocumentationGateCoverage::Algorithm)
    );
    assert_eq!(
        self_iteration_documentation_gate_coverage(&[
            "tools/self_iteration/cases.json".to_owned(),
            "tools/self_iteration/cases/repository_c_syntax_fixture_targets.json".to_owned(),
            "docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md".to_owned()
        ]),
        Some(DocumentationGateCoverage::EvaluationSet)
    );
    assert_eq!(
        self_iteration_documentation_gate_coverage(&[
            "tools/self_iteration/src/evaluator_tail.rs".to_owned(),
            "docs/zh/05-benchmarks/06-c-cpp-syntax-self-iteration-evaluation.md".to_owned()
        ]),
        None
    );
}
