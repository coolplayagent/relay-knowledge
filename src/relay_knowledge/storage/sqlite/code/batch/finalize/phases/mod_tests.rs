//! Direct tests for observable finalization phase identities.

use std::collections::BTreeSet;

use super::{
    PUBLISH_SCOPE, REBUILD_CALLS, REBUILD_REFERENCE_SEARCH, REFRESH_DEPENDENCIES,
    RESOLVE_CALL_TARGETS, RESOLVE_IMPORTS, RESOLVE_REFERENCES, RESOLVE_WORKSPACE_IMPORTS,
};

#[test]
fn finalization_phase_states_are_unique_and_namespaced() {
    let phases = [
        RESOLVE_REFERENCES,
        RESOLVE_IMPORTS,
        RESOLVE_CALL_TARGETS,
        REFRESH_DEPENDENCIES,
        REBUILD_REFERENCE_SEARCH,
        REBUILD_CALLS,
        RESOLVE_WORKSPACE_IMPORTS,
        PUBLISH_SCOPE,
    ];

    assert!(phases.iter().all(|phase| phase.starts_with("finalizing:")));
    assert_eq!(phases.into_iter().collect::<BTreeSet<_>>().len(), 8);
}
