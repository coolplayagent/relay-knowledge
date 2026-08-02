use super::super::test_support;
use super::{
    ImportResolution, ModuleFileResolution, apply_resolution, combined_resolution,
    module_file_resolution,
};

#[test]
fn applying_resolution_sets_the_public_confidence_contract() {
    let cases = [
        (ImportResolution::Resolved, "resolved", 8_000, "inferred"),
        (ImportResolution::Ambiguous, "ambiguous", 5_000, "ambiguous"),
        (
            ImportResolution::Unresolved,
            "unresolved",
            2_500,
            "ambiguous",
        ),
    ];

    for (resolution, state, confidence, tier) in cases {
        let mut import = test_support::import();
        apply_resolution(&mut import, resolution);
        assert_eq!(import.resolution_state, state);
        assert_eq!(import.confidence_basis_points, confidence);
        assert_eq!(import.confidence_tier, tier);
    }
}

#[test]
fn combined_resolution_requires_every_member_to_resolve() {
    assert_eq!(
        combined_resolution([ImportResolution::Resolved, ImportResolution::Resolved]),
        ImportResolution::Resolved
    );
    assert_eq!(
        combined_resolution([ImportResolution::Resolved, ImportResolution::Unresolved]),
        ImportResolution::Ambiguous
    );
    assert_eq!(
        combined_resolution([ImportResolution::Unresolved]),
        ImportResolution::Unresolved
    );
    assert_eq!(combined_resolution([]), ImportResolution::Unresolved);
}

#[test]
fn module_file_resolution_exposes_only_unique_target_hints() {
    assert_eq!(
        module_file_resolution(ModuleFileResolution::Resolved("src/client.rs".to_owned())),
        (ImportResolution::Resolved, Some("src/client.rs".to_owned()))
    );
    assert_eq!(
        module_file_resolution(ModuleFileResolution::Ambiguous),
        (ImportResolution::Ambiguous, None)
    );
}
