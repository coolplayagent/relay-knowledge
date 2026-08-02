// Direct tests for untracked broad-directory admission.

use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

use super::*;

#[test]
fn broad_untracked_directories_require_explicit_path_opt_in() {
    let registration = CodeRepositoryRegistration::new(
        "repository-1",
        "fixture",
        "/tmp/fixture",
        Vec::new(),
        Vec::new(),
    )
    .expect("registration should be valid");
    let unrestricted = CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), Vec::new())
        .expect("selector should be valid");
    let selected =
        CodeRepositorySelector::new("fixture", "HEAD", vec!["vendor/sdk".to_owned()], Vec::new())
            .expect("selector should be valid");

    assert!(!allowed(
        "vendor/sdk/include/api.h",
        &registration,
        &unrestricted
    ));
    assert!(allowed(
        "vendor/sdk/include/api.h",
        &registration,
        &selected
    ));
}
