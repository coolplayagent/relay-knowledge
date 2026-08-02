//! Feature-flag projection tests.

use crate::{code::SnapshotBuild, domain::CodeRepositoryRegistration};

use super::record_feature_flags;

#[test]
fn derives_boolean_configuration_facts_before_projection() {
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );

    record_feature_flags(
        &mut build,
        "config/flags.yaml",
        "flags-file",
        "yaml",
        "checkout_v2: true\n",
        None,
    )
    .expect("feature flags should project");

    assert!(build.feature_flags.iter().any(|record| {
        record.source_key == "checkout_v2" && record.edge_kind == "defines_config"
    }));
}
