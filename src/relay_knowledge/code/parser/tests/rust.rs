//! Rust-specific structured extraction regressions.

use super::*;
use crate::domain::CodeRepositoryRegistration;

#[test]
fn trait_impls_are_indexed_as_implementation_surfaces() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let source = br#"
struct ApiError;
struct S3Error;

impl From<ApiError> for S3Error {
    fn from(_: ApiError) -> Self { S3Error }
}

impl S3Error {
    fn code(&self) -> usize { 0 }
}
"#;

    parse_indexed_file(&mut build, "src/error.rs", source).expect("Rust source should parse");
    let snapshot = build.finish();
    let implementations = snapshot
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == "implementation")
        .collect::<Vec<_>>();

    assert_eq!(implementations.len(), 1);
    assert_eq!(implementations[0].name, "S3Error");
    assert!(
        implementations[0]
            .signature
            .starts_with("impl From<ApiError> for S3Error {")
    );
}
