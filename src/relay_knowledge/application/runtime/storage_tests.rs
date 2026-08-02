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
