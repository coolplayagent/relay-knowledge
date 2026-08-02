//! Direct validation, numeric-boundary, and deadline contracts for file search.

use std::time::Instant;

use rusqlite::Connection;

use super::{limit_i64, search, u64_from_sql};
use crate::storage::FileSearchRequest;

#[test]
fn validates_terms_numeric_boundaries_and_expired_deadlines() {
    let connection = Connection::open_in_memory().expect("connection should open");
    let error = search(
        &connection,
        FileSearchRequest {
            query: "files".to_owned(),
            source_scope: None,
            root_id: None,
            limit: 10,
            timeout_ms: 750,
        },
        Instant::now(),
    )
    .expect_err("expired deadline should fail before SQL preparation");
    assert!(error.to_string().contains("timed out"));

    assert!(limit_i64(usize::MAX).is_err());
    assert!(u64_from_sql(-1).is_err());
    assert_eq!(
        u64_from_sql(42).expect("positive integer should convert"),
        42
    );
}
