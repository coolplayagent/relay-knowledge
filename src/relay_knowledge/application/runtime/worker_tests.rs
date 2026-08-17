use super::*;
use crate::env::{
    EnvironmentConfig, PlatformKind, RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT,
    RELAY_KNOWLEDGE_CODE_INDEX_MAX_INDEXED_REPOSITORIES,
};

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
fn resolves_indexed_repository_retention_limit_from_environment() {
    let default_environment =
        EnvironmentConfig::from_pairs(PlatformKind::Unix, std::iter::empty::<(&str, &str)>())
            .expect("empty environment should parse");
    let default_runtime = WorkerRuntimeConfig::from_environment(&default_environment)
        .expect("default worker runtime should compose");
    assert_eq!(default_runtime.code_index_max_indexed_repositories, 10);

    let overridden_environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [(RELAY_KNOWLEDGE_CODE_INDEX_MAX_INDEXED_REPOSITORIES, "12")],
    )
    .expect("repository retention limit should parse");
    let overridden_runtime = WorkerRuntimeConfig::from_environment(&overridden_environment)
        .expect("overridden worker runtime should compose");
    assert_eq!(overridden_runtime.code_index_max_indexed_repositories, 12);
}

#[cfg(target_pointer_width = "64")]
#[test]
fn rejects_indexed_repository_limit_above_sqlite_integer_range() {
    let oversized = (i64::MAX as u64 + 1).to_string();
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [(
            RELAY_KNOWLEDGE_CODE_INDEX_MAX_INDEXED_REPOSITORIES,
            oversized.as_str(),
        )],
    )
    .expect("platform usize should parse the oversized SQLite value");

    let error = WorkerRuntimeConfig::from_environment(&environment)
        .expect_err("SQLite-incompatible retention limit should fail early");

    assert_eq!(
        error,
        WorkerRuntimeConfigError::IndexedRepositoryLimitTooLarge(i64::MAX as usize + 1)
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
