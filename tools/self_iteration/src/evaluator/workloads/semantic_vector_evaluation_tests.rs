use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::evaluate_semantic_vector_suite;
use crate::evaluator::{EvalRuntime, Limiter};

#[test]
fn external_backend_without_required_environment_stops_before_provider_process() {
    let runtime = EvalRuntime {
        binary: PathBuf::from("relay-knowledge"),
        workspace: PathBuf::from("."),
        env: BTreeMap::from([(
            "RELAY_KNOWLEDGE_SEMANTIC_BACKEND".to_owned(),
            "external".to_owned(),
        )]),
        timeout: 1,
        limiter: Limiter::new(1),
        writer_lock: Arc::new(Mutex::new(())),
        query_jobs: 1,
    };

    let report = evaluate_semantic_vector_suite(&runtime, &serde_json::json!({}))
        .expect("missing environment should produce a failed gate report");

    assert_eq!(report.commands.len(), 1);
    assert_eq!(report.commands[0].name, "semantic_vector_external_env");
    assert!(!report.commands[0].passed());
    assert!(
        report.commands[0]
            .stderr
            .contains("missing external semantic/vector env")
    );
}
