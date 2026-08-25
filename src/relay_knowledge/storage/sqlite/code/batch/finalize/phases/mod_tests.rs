//! Direct tests for observable finalization phase identities.

use std::collections::BTreeSet;

use super::{
    BUILD_QUERY_INDEXES, PARTITIONED_PUBLISH, PUBLISH_SCOPE, REBUILD_CALLS,
    REBUILD_REFERENCE_SEARCH, REFRESH_DEPENDENCIES, RESOLVE_CALL_TARGETS, RESOLVE_IMPORTS,
    RESOLVE_REFERENCES, RESOLVE_WORKSPACE_IMPORTS, SOFTWARE_PROJECTION,
};

#[test]
fn finalization_phase_states_are_unique_and_namespaced() {
    let phases = [
        BUILD_QUERY_INDEXES,
        RESOLVE_REFERENCES,
        RESOLVE_IMPORTS,
        RESOLVE_CALL_TARGETS,
        REFRESH_DEPENDENCIES,
        REBUILD_REFERENCE_SEARCH,
        REBUILD_CALLS,
        RESOLVE_WORKSPACE_IMPORTS,
        PUBLISH_SCOPE,
        SOFTWARE_PROJECTION,
        PARTITIONED_PUBLISH,
    ];

    assert!(phases.iter().all(|phase| phase.starts_with("finalizing:")));
    assert_eq!(
        phases.iter().copied().collect::<BTreeSet<_>>().len(),
        phases.len()
    );
}
