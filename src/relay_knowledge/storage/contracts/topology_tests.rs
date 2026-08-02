use super::*;

#[test]
fn topology_parser_normalizes_supported_aliases() {
    assert_eq!(
        StorageTopology::parse(" sqlite ").expect("single alias should parse"),
        StorageTopology::SingleSqlite
    );
    assert_eq!(
        StorageTopology::parse("SQLITE_PARTITIONED").expect("partitioned alias should parse"),
        StorageTopology::PartitionedSqlite
    );
    assert_eq!(
        StorageTopology::PartitionedSqlite.as_str(),
        "partitioned_sqlite"
    );
}

#[test]
fn topology_parser_rejects_unknown_values() {
    let error = StorageTopology::parse("remote").expect_err("unknown topology should fail");

    assert!(
        error
            .to_string()
            .contains("must be single_sqlite or partitioned_sqlite")
    );
}
