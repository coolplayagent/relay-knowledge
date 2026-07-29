use super::*;

#[test]
fn symbol_identity_query_extracts_safe_exact_symbol_anchors() {
    let scoped = SymbolIdentityQuery::from_query("DBImpl::Get deleted files")
        .expect("scoped identity should parse");

    assert_eq!(scoped.leaf_name(), "Get");
    assert!(scoped.is_scoped());
    assert!(scoped.matches_symbol(
        "Get",
        "leveldb::DBImpl::Get",
        "Status DBImpl::Get(const ReadOptions& options) {",
        "repo://leveldb/db::DBImpl.Get",
    ));
    assert!(!scoped.matches_symbol(
        "Get",
        "benchmarks::db_bench::leveldb.Benchmark.Get",
        "Status Get(const ReadOptions& options) {",
        "repo://leveldb/benchmarks::db_bench.Get",
    ));

    let simple =
        SymbolIdentityQuery::from_query("NewLRUCache").expect("single identifier should parse");
    assert!(!simple.is_scoped());
    assert!(simple.scoped_like_pattern().is_none());
    assert!(simple.matches_symbol("NewLRUCache", "", "", ""));
    assert!(SymbolIdentityQuery::from_query("cache interface").is_none());
    assert!(SymbolIdentityQuery::from_query("leveldb/filter_policy.h").is_none());
    assert!(SymbolIdentityQuery::from_query("linux.debugfs.h").is_none());
}

#[test]
fn scoped_identity_query_builds_literal_sql_prefilter_pattern() {
    let scoped = SymbolIdentityQuery::from_query("DBImpl::Get deleted files")
        .expect("scoped identity should parse");
    assert_eq!(
        scoped.scoped_like_pattern().as_deref(),
        Some("%dbimpl%get%")
    );

    let underscored = SymbolIdentityQuery::from_query("rustfs_iam::error::Error")
        .expect("underscored scope should parse");
    assert_eq!(
        underscored.scoped_like_pattern().as_deref(),
        Some("%rustfs\\_iam%error%error%")
    );
}

#[test]
fn scoped_identity_query_bonus_matches_qualified_edge_targets() {
    assert_eq!(
        scoped_identity_query_bonus(
            "pkg.service.TargetThing",
            ["repo://example/src::pkg::service::TargetThing"],
        ),
        2.0
    );
    assert_eq!(
        scoped_identity_query_bonus("TargetThing", ["pkg.service.TargetThing"]),
        0.0
    );
    assert_eq!(
        scoped_identity_query_bonus("pkg.Client", ["repo://example/src::pkg::test::Client"]),
        0.0
    );
}
