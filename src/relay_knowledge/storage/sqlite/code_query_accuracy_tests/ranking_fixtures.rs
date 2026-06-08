use super::*;

pub(super) fn snapshot_with_archive_output_dir_noise() -> CodeIndexSnapshot {
    let mut target = symbol(
        "checkpoint-symbol",
        "checkpoint-file",
        "src/relay_teams_evals/checkpoint.py",
        "archive_output_dir",
    );
    let mut output_noise = symbol(
        "output-symbol",
        "output-file",
        "src/relay_teams/sessions/runs/background_tasks/projection.py",
        "_OUTPUT_TRUNCATED_SUFFIX",
    );
    let mut directory_noise = symbol(
        "directory-symbol",
        "directory-file",
        "src/relay_teams/workspace/directory_picker.py",
        "_pick_directory_macos",
    );
    let mut archive_noise = symbol(
        "archive-symbol",
        "archive-file",
        "tests/unit_tests/net/test_github_cli.py",
        "test_archive_output_dir_moves_existing_contents_to_timestamped_sibling",
    );
    for symbol in [
        &mut target,
        &mut output_noise,
        &mut directory_noise,
        &mut archive_noise,
    ] {
        symbol.doc_comment = Some("archive old eval output directory timestamp suffix".to_owned());
    }

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 4,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "checkpoint-file",
                "src/relay_teams_evals/checkpoint.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "output-file",
                "src/relay_teams/sessions/runs/background_tasks/projection.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "directory-file",
                "src/relay_teams/workspace/directory_picker.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "archive-file",
                "tests/unit_tests/net/test_github_cli.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![target, output_noise, directory_noise, archive_noise],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn snapshot_with_checkpoint_version_constant_noise() -> CodeIndexSnapshot {
    let mut target = symbol(
        "checkpoint-version-symbol",
        "checkpoint-file",
        "src/relay_teams_evals/checkpoint.py",
        "_CHECKPOINT_VERSION",
    );
    target.kind = "constant".to_owned();
    target.signature = "_CHECKPOINT_VERSION = 1".to_owned();

    let mut checkpoint_noise = symbol(
        "checkpoint-noise",
        "checkpoint-noise-file",
        "src/relay_teams_evals/reporting.py",
        "checkpoint_metadata_report",
    );
    checkpoint_noise.signature = "def checkpoint_metadata_report() -> None:".to_owned();

    let mut version_noise = symbol(
        "version-noise",
        "version-noise-file",
        "src/relay_teams_evals/versioning.py",
        "metadata_version_report",
    );
    version_noise.signature = "def metadata_version_report() -> None:".to_owned();

    let mut constant_noise = symbol(
        "constant-noise",
        "constant-noise-file",
        "src/relay_teams_evals/constants.py",
        "metadata_constant_report",
    );
    constant_noise.signature = "def metadata_constant_report() -> None:".to_owned();

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 4,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "checkpoint-file",
                "src/relay_teams_evals/checkpoint.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "checkpoint-noise-file",
                "src/relay_teams_evals/reporting.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "version-noise-file",
                "src/relay_teams_evals/versioning.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "constant-noise-file",
                "src/relay_teams_evals/constants.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![target, checkpoint_noise, version_noise, constant_noise],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}
