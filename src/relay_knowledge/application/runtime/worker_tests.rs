use super::*;
use crate::env::{EnvironmentConfig, PlatformKind, RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT};

#[test]
fn resolves_code_index_concurrency_from_environment() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [(RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT, "4")],
    )
    .expect("environment should parse");

    let runtime =
        WorkerRuntimeConfig::from_environment(&environment).expect("worker runtime should compose");

    assert_eq!(runtime.code_index_max_in_flight, 4);
}

#[test]
fn caps_code_index_concurrency_from_environment() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [(RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT, "99")],
    )
    .expect("environment should parse");

    let runtime =
        WorkerRuntimeConfig::from_environment(&environment).expect("worker runtime should compose");

    assert_eq!(
        runtime.code_index_max_in_flight,
        WorkerRuntimeConfig::MAX_CODE_INDEX_MAX_IN_FLIGHT
    );
}

#[test]
fn rejects_endpoint_without_http_host() {
    for endpoint in ["https://worker.local", "http://", "http://:8792"] {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [("RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT", endpoint)],
        )
        .expect("environment should parse");

        let error = WorkerRuntimeConfig::from_environment(&environment)
            .expect_err("invalid worker endpoint should fail");

        assert!(matches!(
            error,
            WorkerRuntimeConfigError::InvalidEndpoint(_)
        ));
    }
}
