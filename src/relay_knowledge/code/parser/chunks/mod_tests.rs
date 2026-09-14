// Direct tests for source-surface chunk construction.

use crate::{
    code::{SnapshotBuild, parser::parse_indexed_file},
    domain::CodeRepositoryRegistration,
    storage::{
        CodeIndexPublicationStore as _, CodeIndexSourceStore as _, RepositoryCatalogStore as _,
        SqliteGraphStore,
    },
};

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

#[tokio::test]
async fn markdown_windows_round_trip_losslessly_through_snapshot_storage() {
    let registration = registration();
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree-lossless-markdown".to_owned(),
        true,
        1,
        0,
    );
    let mut content =
        String::from("---\ntype: research\nname: Lossless windows\n---\n\n# Exact Markdown\n\n");
    for index in 0..240 {
        content.push_str(&format!(
            "    indented_{index:03} = value_{index:03}_with_window_padding\n"
        ));
    }
    content.push_str("\n```md\n[inside-fence][ref]\n```\n\n[outside][ref]\n\n[ref]: ./target.md\n");
    assert!(content.len() > MAX_SOURCE_SURFACE_CHUNK_BYTES);
    assert!(content.lines().count() > MAX_SOURCE_SURFACE_CHUNK_LINES);

    parse_indexed_file(&mut build, "docs/large.md", content.as_bytes())
        .expect("large Markdown should parse");
    let snapshot = build.finish();
    assert!(snapshot.chunks.len() > 1);
    assert_eq!(
        snapshot
            .chunks
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<String>(),
        content
    );
    let source_scope = snapshot.source_scope.clone();
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should persist");

    let documents = store
        .repository_documents_for_scope(source_scope, vec!["docs".to_owned()], 1, content.len())
        .await
        .expect("lossless Markdown windows should materialize");

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].path, "docs/large.md");
    assert_eq!(documents[0].content, content);
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

#[test]
fn large_symbolized_sources_keep_top_level_relationship_assertions() {
    let registration = registration();
    let build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let assertion = "var _ runtime.Contract = &Worker{}\n";
    let padding = "// bounded top-level context\n".repeat(340);
    let worker_start = padding.len() + assertion.len();
    let worker = "type Worker struct{}\n";
    let content = format!("{padding}{assertion}{worker}");
    assert!(content.len() > MAX_SOURCE_SURFACE_CHUNK_BYTES);
    let symbols = vec![symbol(
        "Worker",
        "struct",
        worker_start,
        worker_start + worker.len(),
        342,
    )];

    let chunks = chunks_for_symbols(
        &build,
        "runtime/worker.go",
        "file-worker",
        "go",
        &content,
        &symbols,
    )
    .expect("large Go source chunks should build");

    assert!(chunks.iter().any(|chunk| {
        chunk.symbol_snapshot_id.is_none()
            && chunk.content.contains("var _ runtime.Contract = &Worker{}")
    }));
    assert!(chunks.iter().all(|chunk| {
        chunk.symbol_snapshot_id.is_some() || chunk.content.len() <= MAX_SOURCE_SURFACE_CHUNK_BYTES
    }));
}

#[test]
fn uncovered_source_chunks_have_a_per_file_footprint_ceiling() {
    let registration = registration();
    let build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    let symbol_count = 64usize;
    let mut content = String::new();
    let mut symbols = Vec::with_capacity(symbol_count);
    for index in 0..symbol_count {
        content.push_str(&format!("// file-level relationship context {index}\n").repeat(50));
        let start = content.len();
        content.push_str(&format!("struct Item{index:02};\n"));
        symbols.push(symbol(
            &format!("Item{index:02}"),
            "struct",
            start,
            content.len(),
            index.saturating_mul(51).saturating_add(51),
        ));
    }
    content.push_str("// trailing file-level relationship context\n");
    let uncovered_gap_count = symbol_count.saturating_add(1);

    let chunks = chunks_for_symbols(
        &build,
        "include/footprint.h",
        "file-footprint",
        "c",
        &content,
        &symbols,
    )
    .expect("bounded uncovered source chunks should build");
    let uncovered_chunk_count = chunks
        .iter()
        .filter(|chunk| chunk.symbol_snapshot_id.is_none())
        .count();

    assert_eq!(
        uncovered_chunk_count,
        uncovered_gap_count.min(MAX_UNCOVERED_SOURCE_CHUNKS_PER_FILE)
    );
    assert_eq!(chunks.len(), symbol_count + uncovered_chunk_count);
    assert!(chunks.len() <= symbol_count.saturating_add(MAX_UNCOVERED_SOURCE_CHUNKS_PER_FILE));
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
