use super::*;
use crate::application::update::version::StableVersion;

#[test]
fn crates_payload_uses_stable_version_field_and_reports_missing_value() {
    let stable = crates_candidate(CratesPackageResponse {
        package: CratesPackage {
            max_stable_version: Some("2.0.0".to_owned()),
        },
    })
    .expect("stable crates release should parse");
    let missing_stable = crates_candidate(CratesPackageResponse {
        package: CratesPackage {
            max_stable_version: None,
        },
    })
    .expect_err("missing stable version should be diagnostic");

    assert_eq!(stable.version, StableVersion::new(2, 0, 0));
    assert_eq!(missing_stable.code, "stable_version_unavailable");
}
