// Direct tests for version parsing and ordering.

use super::*;

#[test]
fn stable_versions_parse_and_prerelease_metadata_is_rejected() {
    assert_eq!(
        stable_version("v1.2.3").expect("version should parse"),
        StableVersion::new(1, 2, 3)
    );
    assert_eq!(
        comparable_version("1.2.3-rc.1").expect("current prerelease should compare"),
        StableVersion::prerelease(1, 2, 3)
    );
    assert!(StableVersion::new(1, 2, 3) > StableVersion::prerelease(1, 2, 3));
    assert!(stable_version("1.2.3-rc.1").is_err());
}

#[test]
fn semver_core_requires_exactly_three_numeric_components() {
    for invalid in ["1.2", "1.2.3.4", "1.two.3", "1..3"] {
        assert!(
            comparable_version(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }
}
