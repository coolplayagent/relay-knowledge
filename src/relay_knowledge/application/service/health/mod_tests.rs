use super::*;
use crate::storage::SqliteStorageDiagnostics;

#[test]
fn maintenance_advice_uses_observed_pressure_and_age_without_claiming_timeout_cause() {
    let mut sqlite = SqliteStorageDiagnostics::default();
    assert!(storage_recommendations(&sqlite, MAINTENANCE_AGE_MS, false).is_empty());
    let advice = storage_recommendations(&sqlite, MAINTENANCE_AGE_MS, true);
    assert_eq!(advice.len(), 1);
    assert!(advice[0].contains("does not establish a WAL problem"));
    sqlite.wal_size_bytes = Some(LARGE_WAL_BYTES - 1);
    sqlite.last_maintenance_at_ms = Some(1);
    assert!(storage_recommendations(&sqlite, MAINTENANCE_AGE_MS, false).is_empty());
    sqlite.wal_size_bytes = Some(LARGE_WAL_BYTES);
    let advice = storage_recommendations(&sqlite, MAINTENANCE_AGE_MS + 1, false);
    assert_eq!(advice.len(), 2);
    assert!(advice[0].contains("checkpoint"));
    assert!(advice[1].contains("service doctor"));
    sqlite.wal_size_bytes = Some(0);
    sqlite.last_maintenance_at_ms = Some(u64::MAX);
    assert!(storage_recommendations(&sqlite, 0, false).is_empty());
    sqlite.last_maintenance_error = Some("private failure details".into());
    let advice = storage_recommendations(&sqlite, 0, false);
    assert_eq!(advice.len(), 1);
    assert!(!advice[0].contains("private"));
}
