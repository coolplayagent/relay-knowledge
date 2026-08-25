//! Direct tests for import-resolution metadata persisted during finalization.

use super::{ImportResolution, resolution_fields};

#[test]
fn finalization_preserves_canonical_unresolved_include_targets() {
    for (statement, expected) in [
        ("#include <vendor/runtime.h>", "vendor/runtime.h"),
        ("#include \"platform/driver.h\"", "platform/driver.h"),
    ] {
        let (_, _, _, target_hint) = resolution_fields(ImportResolution::Unresolved, statement);
        assert_eq!(target_hint, expected);
    }
}
