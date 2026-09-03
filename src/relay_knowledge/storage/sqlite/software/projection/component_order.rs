//! Canonical ordering for dependency components in a projection response.

use crate::domain::SoftwareComponent;

pub(super) fn sort_by_canonical_evidence(components: &mut [SoftwareComponent]) {
    components.sort_by(|left, right| {
        (
            &left.ecosystem,
            &left.name,
            std::cmp::Reverse(&left.relationship_state),
            &left.evidence_path,
            &left.component_id,
        )
            .cmp(&(
                &right.ecosystem,
                &right.name,
                std::cmp::Reverse(&right.relationship_state),
                &right.evidence_path,
                &right.component_id,
            ))
    });
}
