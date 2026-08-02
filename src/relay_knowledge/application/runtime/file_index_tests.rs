use std::path::PathBuf;

use super::*;

#[test]
fn root_ids_use_canonical_paths_when_available() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be valid")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-runtime-root-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("fixture root should be created");

    let direct = FileIndexRootConfig::new("local-files", root.clone());
    let dotted = FileIndexRootConfig::new("local-files", root.join("."));
    assert_eq!(direct.root_id, dotted.root_id);
    assert_eq!(direct.root_path, dotted.root_path);

    std::fs::remove_dir_all(root).expect("fixture root should be removed");
}

#[test]
fn root_ids_normalize_nonexistent_trailing_separators() {
    let plain = FileIndexRootConfig::new("local-files", PathBuf::from("/opt/docs"));
    let trailing = FileIndexRootConfig::new("local-files", PathBuf::from("/opt/docs/"));

    assert_eq!(plain.root_id, trailing.root_id);
    assert_eq!(plain.root_path, trailing.root_path);
}

#[test]
fn roots_from_environment_must_be_absolute() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [("RELAY_KNOWLEDGE_FILE_INDEX_ROOTS", "docs;/opt/docs")],
    )
    .expect("environment should parse");

    let error = FileIndexRuntimeConfig::from_environment(&environment)
        .expect_err("relative file index roots should be rejected");

    assert_eq!(
        error,
        FileIndexRuntimeConfigError::RelativeRoot("docs".to_owned())
    );
    assert!(error.to_string().contains("absolute path"));
}

#[test]
fn roots_accept_windows_drive_and_unc_paths() {
    assert!(is_absolute_file_index_root(
        r"D:\Documents",
        PlatformKind::Windows
    ));
    assert!(is_absolute_file_index_root(
        r"\\server\share\Documents",
        PlatformKind::Windows
    ));
    assert!(!is_absolute_file_index_root(
        r"D:Documents",
        PlatformKind::Windows
    ));
}
