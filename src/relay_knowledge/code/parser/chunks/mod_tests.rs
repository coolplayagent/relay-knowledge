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

#[test]
fn structured_documents_use_bounded_nonduplicating_source_windows() {
    let registration = registration();
    let build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let content = (0..600)
        .map(|index| format!("rk_config_key_{index:04}: value_{index:04}_with_padding\n"))
        .collect::<String>();

    let chunks = chunks_for_symbols(
        &build,
        "config/large.yaml",
        "file-yaml",
        "yaml",
        &content,
        &[],
    )
    .expect("structured source windows should build");

    assert!(chunks.len() > 1);
    assert!(chunks.iter().all(|chunk| chunk.symbol_snapshot_id.is_none()
        && chunk.content.len() <= MAX_SOURCE_SURFACE_CHUNK_BYTES));
    assert!(chunks[0].content.contains("rk_config_key_0000"));
    assert!(
        chunks
            .last()
            .is_some_and(|chunk| chunk.content.contains("rk_config_key_0599"))
    );
    for window in chunks.windows(2) {
        assert_eq!(window[0].byte_range.end, window[1].byte_range.start);
    }
}

#[test]
fn dense_sources_use_windows_and_keep_callable_context_chunks() {
    let registration = registration();
    let build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let mut content = String::new();
    let mut symbols = Vec::new();
    for index in 0..80 {
        let start = content.len();
        content.push_str(&format!("#define RK_DENSE_{index:03} {index}\n"));
        symbols.push(symbol(
            &format!("RK_DENSE_{index:03}"),
            "macro",
            start,
            content.len(),
            index + 1,
        ));
    }
    let function_start = content.len();
    content.push_str("int rk_dense_run(void) { return RK_DENSE_079; }\n");
    symbols.push(symbol(
        "rk_dense_run",
        "function",
        function_start,
        content.len(),
        81,
    ));

    let chunks = chunks_for_symbols(
        &build,
        "include/dense.h",
        "file-dense",
        "c",
        &content,
        &symbols,
    )
    .expect("dense source chunks should build");

    assert_eq!(chunks.len(), 2);
    assert!(chunks[0].symbol_snapshot_id.is_none());
    assert!(chunks[0].content.contains("RK_DENSE_000"));
    assert!(chunks[0].content.contains("RK_DENSE_079"));
    assert_eq!(
        chunks[1].symbol_snapshot_id.as_deref(),
        Some("symbol-rk_dense_run")
    );
    assert!(chunks[1].content.contains("rk_dense_run"));
}

fn symbol(
    name: &str,
    kind: &str,
    byte_start: usize,
    byte_end: usize,
    line: usize,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        symbol_snapshot_id: format!("symbol-{name}"),
        canonical_symbol_id: format!("repo://repo/{name}"),
        file_id: "file-dense".to_owned(),
        path: "include/dense.h".to_owned(),
        language_id: "c".to_owned(),
        name: name.to_owned(),
        qualified_name: format!("include::dense::{name}"),
        kind: kind.to_owned(),
        signature: name.to_owned(),
        doc_comment: None,
        byte_range: RepositoryCodeRange {
            start: byte_start as u32,
            end: byte_end as u32,
        },
        line_range: RepositoryCodeRange {
            start: line as u32,
            end: line as u32,
        },
        symbol_role: None,
    }
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
