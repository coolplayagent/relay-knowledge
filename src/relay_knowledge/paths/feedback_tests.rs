use crate::{
    env::{EnvironmentConfig, PlatformKind},
    paths::RuntimePaths,
    project::FEEDBACK_DIRECTORY_NAME,
};

#[test]
fn feedback_publication_files_share_private_state_directory() {
    let environment =
        EnvironmentConfig::from_pairs(PlatformKind::Unix, [("HOME", "/home/test")]).unwrap();
    let runtime = RuntimePaths::resolve(&environment.platform, &environment.paths).unwrap();
    let feedback = runtime.feedback_store_paths();
    assert_eq!(
        feedback.directory.parent(),
        Some(runtime.data_dir.as_path())
    );
    assert_eq!(
        feedback.directory.file_name().unwrap(),
        FEEDBACK_DIRECTORY_NAME
    );
    for path in [&feedback.journal, &feedback.lock, &feedback.prepared] {
        assert_eq!(path.parent(), Some(feedback.directory.as_path()));
    }
    assert_ne!(feedback.journal, feedback.prepared);
    assert_ne!(feedback.lock, feedback.journal);
}
