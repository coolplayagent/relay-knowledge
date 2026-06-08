use crate::domain::{RepositoryCodeChunkRecord, RepositoryCodeRange, RepositoryCodeSymbolRecord};

use super::{
    super::{CodeIndexError, SnapshotBuild, stable_content_hash, stable_id},
    text::count_lines,
};

const MAX_SOURCE_SURFACE_CHUNK_BYTES: usize = 8_000;
const MAX_SOURCE_SURFACE_CHUNK_LINES: usize = 200;

pub(super) fn chunks_for_symbols(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
    symbols: &[RepositoryCodeSymbolRecord],
) -> Result<Vec<RepositoryCodeChunkRecord>, CodeIndexError> {
    let mut chunks = Vec::new();
    for symbol in symbols {
        let start = symbol.byte_range.start as usize;
        let end = symbol.byte_range.end as usize;
        let excerpt = content.get(start..end).unwrap_or(&symbol.signature).trim();
        chunks.push(RepositoryCodeChunkRecord {
            repository_id: build.repository_id.clone(),
            source_scope: build.source_scope.clone(),
            chunk_id: stable_id(
                "chunk",
                [
                    &build.repository_id,
                    &build.source_scope,
                    path,
                    &symbol.symbol_snapshot_id,
                    excerpt,
                ],
            ),
            file_id: file_id.to_owned(),
            path: path.to_owned(),
            language_id: language_id.to_owned(),
            content: trim_to_budget(excerpt, MAX_SOURCE_SURFACE_CHUNK_BYTES),
            byte_range: symbol.byte_range.clone(),
            line_range: symbol.line_range.clone(),
            symbol_snapshot_id: Some(symbol.symbol_snapshot_id.clone()),
        });
    }
    if chunks.is_empty() || keeps_file_chunk_with_symbol_chunks(language_id, content, symbols) {
        add_file_chunk_to_vec(build, path, file_id, language_id, content, &mut chunks)?;
    }

    Ok(chunks)
}

fn keeps_file_chunk_with_symbol_chunks(
    language_id: &str,
    content: &str,
    symbols: &[RepositoryCodeSymbolRecord],
) -> bool {
    if language_keeps_complete_file_chunk(language_id) {
        return true;
    }
    content.len() <= MAX_SOURCE_SURFACE_CHUNK_BYTES
        && count_lines(content.as_bytes()) <= MAX_SOURCE_SURFACE_CHUNK_LINES
        && has_uncovered_source_surface(content, symbols)
}

fn language_keeps_complete_file_chunk(language_id: &str) -> bool {
    matches!(
        language_id,
        "cmake"
            | "dockerfile"
            | "gomod"
            | "gotemplate"
            | "ini"
            | "jinja2"
            | "json"
            | "make"
            | "markdown"
            | "ninja"
            | "properties"
            | "starlark"
            | "toml"
            | "xml"
            | "yaml"
    )
}

fn has_uncovered_source_surface(content: &str, symbols: &[RepositoryCodeSymbolRecord]) -> bool {
    let mut ranges = symbols
        .iter()
        .filter_map(|symbol| {
            let start = usize::try_from(symbol.byte_range.start).ok()?;
            let end = usize::try_from(symbol.byte_range.end).ok()?;
            (start < end && end <= content.len()).then_some((start, end))
        })
        .collect::<Vec<_>>();
    ranges.sort_unstable_by_key(|range| range.0);

    let mut covered_end = 0usize;
    for (start, end) in ranges {
        if start > covered_end && contains_source_token(&content[covered_end..start]) {
            return true;
        }
        covered_end = covered_end.max(end);
    }

    covered_end < content.len() && contains_source_token(&content[covered_end..])
}

fn contains_source_token(content: &str) -> bool {
    content
        .chars()
        .any(|character| character.is_alphanumeric() || matches!(character, '_' | '#' | '@'))
}

pub(super) fn add_file_chunk(
    build: &mut SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
) -> Result<(), CodeIndexError> {
    let mut chunks = Vec::new();
    add_file_chunk_to_vec(build, path, file_id, language_id, content, &mut chunks)?;
    build.chunks.extend(chunks);

    Ok(())
}

fn add_file_chunk_to_vec(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
    chunks: &mut Vec<RepositoryCodeChunkRecord>,
) -> Result<(), CodeIndexError> {
    let byte_end = content.len();
    let line_end = count_lines(content.as_bytes()).max(1);
    chunks.push(RepositoryCodeChunkRecord {
        repository_id: build.repository_id.clone(),
        source_scope: build.source_scope.clone(),
        chunk_id: stable_id(
            "chunk",
            [
                &build.repository_id,
                &build.source_scope,
                path,
                "file",
                &stable_content_hash(content.as_bytes()),
            ],
        ),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        content: file_chunk_content(path, content),
        byte_range: RepositoryCodeRange::new("byte_range", 0, byte_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        line_range: RepositoryCodeRange::new("line_range", 1, line_end)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        symbol_snapshot_id: None,
    });

    Ok(())
}

fn file_chunk_content(path: &str, content: &str) -> String {
    if keeps_complete_manifest_content(path) {
        content.trim().to_owned()
    } else {
        trim_to_budget(content, MAX_SOURCE_SURFACE_CHUNK_BYTES)
    }
}

fn keeps_complete_manifest_content(path: &str) -> bool {
    path.replace('\\', "/")
        .rsplit('/')
        .next()
        .is_some_and(|name| {
            matches!(
                name,
                "go.mod"
                    | "go.work"
                    | "package.json"
                    | "pnpm-workspace.yaml"
                    | "pnpm-workspace.yml"
            )
        })
}

fn trim_to_budget(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.trim().to_owned();
    }
    let mut end = max_bytes;
    while !content.is_char_boundary(end) {
        end -= 1;
    }

    content[..end].trim().to_owned()
}

#[cfg(test)]
mod tests {
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
}
