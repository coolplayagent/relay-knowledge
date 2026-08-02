use super::SqliteDiagnosticsAggregate;
use crate::storage::{SqliteStorageDiagnostics, StorageError};

#[test]
fn aggregate_reports_mixed_journals_saturating_wal_and_labeled_errors() {
    let mut aggregate = SqliteDiagnosticsAggregate::new();
    aggregate.push("control", diagnostics("wal", Some(u64::MAX), Some(10)));
    aggregate.push("shard repo", diagnostics("delete", Some(8), Some(12)));
    aggregate.push_error(
        "shard missing",
        StorageError::InvalidInput("repository shard is missing".to_owned()),
    );

    let result = aggregate.finish();
    assert_eq!(result.journal_mode, "mixed");
    assert_eq!(result.wal_size_bytes, None);
    assert_eq!(result.last_maintenance_at_ms, Some(12));
    assert!(
        result
            .last_maintenance_error
            .expect("missing shard should be reported")
            .contains("shard missing")
    );
}

fn diagnostics(
    journal_mode: &str,
    wal_size_bytes: Option<u64>,
    last_maintenance_at_ms: Option<u64>,
) -> SqliteStorageDiagnostics {
    SqliteStorageDiagnostics {
        journal_mode: journal_mode.to_owned(),
        wal_size_bytes,
        last_maintenance_at_ms,
        last_maintenance_error: None,
    }
}
