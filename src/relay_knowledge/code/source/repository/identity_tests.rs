use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn filesystem_identity_is_stable_and_source_kind_is_explicit() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-repository-identity-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).expect("temporary source root should be created");

    let first = filesystem_registration_identity(&root).expect("identity should resolve");
    let second = filesystem_registration_identity(&root).expect("identity should be stable");
    let kind = source_kind(&root).expect("plain directory should use filesystem source");
    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(first, second);
    assert!(kind.is_filesystem());
}
