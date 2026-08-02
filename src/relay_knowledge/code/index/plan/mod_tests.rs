// Direct tests for bounded index planning.

use super::*;

#[test]
fn parser_worker_count_keeps_tiny_batches_serial() {
    assert_eq!(worker_count(7, 32 * 1024), 1);
}

#[test]
fn parser_worker_count_scales_with_bounded_batch_work() {
    let available = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let workers = worker_count(96, 4 * 1024 * 1024);

    assert_eq!(workers, available.min(8).min(96));
    assert!(workers >= 1);
}

#[test]
fn parser_worker_count_caps_thread_fanout_for_small_byte_batches() {
    let available = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let workers = worker_count(40, 128 * 1024);

    assert_eq!(workers, available.min(3).min(40));
}

#[test]
fn batch_row_count_includes_feature_flags() {
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
    build.feature_flags = crate::code::feature_flags::extract_feature_flags(
        crate::code::feature_flags::FeatureFlagFileInput {
            repository_id: &build.repository_id,
            source_scope: &build.source_scope,
            file_id: "file",
            path: "src/lib.rs",
            language_id: "rust",
            content: "if env::var(\"CHECKOUT_V2\").is_ok() && env::var(\"PAYMENTS_V2\").is_ok() {}",
            config_facts: &[],
        },
    )
    .expect("feature flags should extract");

    assert_eq!(batch_row_count(&build), 2);
}
