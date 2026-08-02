// Direct tests for source-surface chunk construction.

use crate::{code::SnapshotBuild, domain::CodeRepositoryRegistration};

use super::*;

#[test]
fn package_json_file_chunks_keep_complete_manifest_content() {
    let registration = registration();
    let build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let manifest = format!(
        "{{\"padding\":\"{}\",\"name\":\"@myorg/ui-components\",\"main\":\"src/index.ts\"}}",
        "x".repeat(MAX_SOURCE_SURFACE_CHUNK_BYTES)
    );
    let mut chunks = Vec::new();

    add_file_chunk_to_vec(
        &build,
        "packages/ui/package.json",
        "file-package-json",
        "json",
        &manifest,
        &mut chunks,
    )
    .expect("package chunk should build");

    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.len() > MAX_SOURCE_SURFACE_CHUNK_BYTES);
    assert!(chunks[0].content.contains("@myorg/ui-components"));
    assert!(chunks[0].content.contains("src/index.ts"));
}

#[test]
fn workspace_manifest_file_chunks_keep_complete_content() {
    let registration = registration();
    let build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    for (path, tail) in [
        ("go.mod", "module example.com/root"),
        ("go.work", "use ./late-module"),
        ("pnpm-workspace.yaml", "  - 'late-package'"),
        ("pnpm-workspace.yml", "  - 'late-package-yml'"),
    ] {
        let content = format!(
            "padding: {}\n{tail}\n",
            "x".repeat(MAX_SOURCE_SURFACE_CHUNK_BYTES)
        );
        let mut chunks = Vec::new();

        add_file_chunk_to_vec(
            &build,
            path,
            "file-manifest",
            "unknown",
            &content,
            &mut chunks,
        )
        .expect("workspace manifest chunk should build");

        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.len() > MAX_SOURCE_SURFACE_CHUNK_BYTES);
        assert!(
            chunks[0].content.contains(tail),
            "{path} should retain tail"
        );
    }
}

#[test]
fn non_manifest_file_chunks_stay_within_surface_budget() {
    let registration = registration();
    let build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let content = "x".repeat(MAX_SOURCE_SURFACE_CHUNK_BYTES + 512);
    let mut chunks = Vec::new();

    add_file_chunk_to_vec(
        &build,
        "src/config.json",
        "file-config-json",
        "json",
        &content,
        &mut chunks,
    )
    .expect("config chunk should build");

    assert_eq!(chunks[0].content.len(), MAX_SOURCE_SURFACE_CHUNK_BYTES);
}

fn registration() -> CodeRepositoryRegistration {
    let root = std::env::temp_dir().join("relay-knowledge-parser-chunk-test");
    CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        root.to_string_lossy().into_owned(),
        Vec::new(),
        Vec::new(),
    )
    .expect("registration should validate")
}
