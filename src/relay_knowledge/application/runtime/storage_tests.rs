use super::*;
use crate::env::{EnvironmentConfig, PlatformKind, RELAY_KNOWLEDGE_STORAGE_TOPOLOGY};

#[test]
fn resolves_storage_topology_from_environment() {
    let default_environment = storage_topology_test_environment(None);
    let default_runtime = StorageRuntimeConfig::from_environment(&default_environment)
        .expect("storage runtime should compose");

    assert_eq!(default_runtime.topology, StorageTopology::SingleSqlite);

    let partitioned_environment = storage_topology_test_environment(Some("partitioned_sqlite"));
    let partitioned_runtime = StorageRuntimeConfig::from_environment(&partitioned_environment)
        .expect("storage runtime should compose");

    assert_eq!(
        partitioned_runtime.topology,
        StorageTopology::PartitionedSqlite
    );
}

#[test]
fn rejects_invalid_storage_topology_from_environment() {
    let environment = storage_topology_test_environment(Some("distributed_sqlite"));

    let error = StorageRuntimeConfig::from_environment(&environment)
        .expect_err("invalid storage topology should be rejected");

    assert!(error.to_string().contains("single_sqlite"));
    assert!(error.to_string().contains("partitioned_sqlite"));
}

fn storage_topology_test_environment(topology: Option<&str>) -> EnvironmentConfig {
    let suffix = topology.unwrap_or("default");
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-runtime-storage-{suffix}-{}",
        std::process::id()
    ));
    let mut pairs = vec![(
        "RELAY_KNOWLEDGE_HOME".to_owned(),
        root.display().to_string(),
    )];
    if let Some(topology) = topology {
        pairs.push((
            RELAY_KNOWLEDGE_STORAGE_TOPOLOGY.to_owned(),
            topology.to_owned(),
        ));
    }

    EnvironmentConfig::from_pairs(PlatformKind::current(), pairs).expect("environment should parse")
}

#[test]
fn validates_health_snapshot_budget_and_environment_errors() {
    use crate::env::RELAY_KNOWLEDGE_HEALTH_STORAGE_BUDGET_MS;
    let mut environment = storage_topology_test_environment(None);
    assert_eq!(
        StorageRuntimeConfig::from_environment(&environment)
            .unwrap()
            .health_budget,
        Duration::from_millis(500)
    );
    for millis in [1, 5000, 30000] {
        environment.health_storage_budget_ms = Some(millis);
        assert_eq!(
            StorageRuntimeConfig::from_environment(&environment)
                .unwrap()
                .health_budget,
            Duration::from_millis(millis)
        );
    }
    for millis in [0, 30001, u64::MAX] {
        environment.health_storage_budget_ms = Some(millis);
        assert_eq!(
            StorageRuntimeConfig::from_environment(&environment).unwrap_err(),
            StorageRuntimeConfigError::InvalidHealthBudget
        );
    }
    for value in ["0", "-1", "invalid", "18446744073709551616"] {
        assert!(
            EnvironmentConfig::from_pairs(
                PlatformKind::current(),
                [(RELAY_KNOWLEDGE_HEALTH_STORAGE_BUDGET_MS, value)]
            )
            .is_err()
        );
    }
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [(RELAY_KNOWLEDGE_HEALTH_STORAGE_BUDGET_MS, "5000")],
    )
    .unwrap();
    assert_eq!(
        StorageRuntimeConfig::from_environment(&environment)
            .unwrap()
            .health_budget,
        Duration::from_secs(5)
    );
    assert!(
        StorageRuntimeConfigError::InvalidHealthBudget
            .to_string()
            .contains("30000")
    );
}

#[test]
fn health_budget_reserves_time_for_http_fallback_assembly() {
    let mut config =
        StorageRuntimeConfig::from_environment(&storage_topology_test_environment(None)).unwrap();
    assert_eq!(
        config.health_budget_for_request(Duration::from_secs(30)),
        Duration::from_millis(500)
    );
    config.health_budget = Duration::from_secs(30);
    for (request, expected) in [(30_000, 29_900), (1000, 900), (100, 50), (10, 5)] {
        let budget = config.health_budget_for_request(Duration::from_millis(request));
        assert_eq!(budget, Duration::from_millis(expected));
        assert!(budget < Duration::from_millis(request));
    }
    assert_eq!(
        config.health_budget_for_request(Duration::from_millis(1)),
        Duration::from_micros(500)
    );
}
