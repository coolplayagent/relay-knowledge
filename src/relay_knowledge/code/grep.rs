use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::domain::{CodeRepositoryRegistration, RepositoryCodeRange};

use super::{
    CodeIndexError,
    languages::language_id,
    safe_git_blob_path,
    scope::{
        scoped_source_snapshot_for_registration, scoped_source_snapshot_for_registration_filters,
    },
    source::{
        source_batch_bytes_after_content_verification, source_blob_sizes_after_policy_verification,
        source_bytes_after_content_verification, source_commit_is_filesystem,
    },
    source_line_defines_identity,
};

#[path = "grep_query.rs"]
mod grep_query;
use grep_query::{find_query_bytes, source_grep_queries};

pub(crate) const SOURCE_GREP_CANDIDATE_FILE_LIMIT: usize = 256;
const MAX_GREP_CANDIDATE_FILES: usize = SOURCE_GREP_CANDIDATE_FILE_LIMIT;
const MAX_GREP_BYTES: usize = 8 * 1024 * 1024;
const MAX_GREP_LINE_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceGrepKind {
    Definition,
    References,
    Imports,
    Hybrid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceGrepRequest {
    pub(crate) query: String,
    pub(crate) paths: Vec<String>,
    pub(crate) path_filters: Vec<String>,
    pub(crate) language_filters: Vec<String>,
    pub(crate) limit: usize,
    pub(crate) kind: SourceGrepKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceGrepOutcome {
    pub(crate) matches: Vec<SourceGrepMatch>,
    pub(crate) degraded_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceGrepMatch {
    pub(crate) path: String,
    pub(crate) language_id: String,
    pub(crate) excerpt: String,
    pub(crate) byte_range: RepositoryCodeRange,
    pub(crate) line_range: RepositoryCodeRange,
}

pub(crate) fn source_grep_matches(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    request: SourceGrepRequest,
) -> Result<SourceGrepOutcome, CodeIndexError> {
    if request.limit == 0 || request.query.trim().is_empty() {
        return Ok(SourceGrepOutcome {
            matches: Vec::new(),
            degraded_reason: None,
        });
    }
    let candidates = selected_candidate_paths(&request);
    if candidates.paths.is_empty() {
        return Ok(SourceGrepOutcome {
            matches: Vec::new(),
            degraded_reason: candidates.degraded_reason,
        });
    }
    let mut tree = TempSourceTree::create()?;
    let materialized = materialize_source_blobs(
        registration,
        commit,
        &candidates.paths,
        SourceMaterializationScope {
            path_filters: &request.path_filters,
            language_filters: &request.language_filters,
        },
        &mut tree,
    )?;
    let degraded_reason = candidates
        .degraded_reason
        .or(materialized.degraded_reason.clone());
    if materialized.file_count == 0 {
        return Ok(SourceGrepOutcome {
            matches: Vec::new(),
            degraded_reason,
        });
    }
    source_grep_matches_from_materialized_tree(
        &tree.root,
        &candidates.paths,
        &request,
        degraded_reason,
    )
}

fn source_grep_matches_from_materialized_tree(
    root: &Path,
    paths: &[String],
    request: &SourceGrepRequest,
    degraded_reason: Option<String>,
) -> Result<SourceGrepOutcome, CodeIndexError> {
    let matches = internal_source_grep_matches(root, paths, request, |matched| {
        source_grep_accepts(request.kind, &request.query, matched)
    })?;

    Ok(SourceGrepOutcome {
        matches,
        degraded_reason,
    })
}

fn source_grep_accepts(kind: SourceGrepKind, query: &str, matched: &SourceGrepMatch) -> bool {
    kind != SourceGrepKind::Definition
        || matched
            .excerpt
            .lines()
            .map(str::trim)
            .any(|line| source_line_defines_identity(line, query))
}

fn selected_candidate_paths(request: &SourceGrepRequest) -> CandidatePaths {
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();
    let mut exhausted = false;
    for path in &request.paths {
        if paths.len() >= MAX_GREP_CANDIDATE_FILES {
            exhausted = true;
            break;
        }
        if !safe_git_blob_path(path) || !path_filter_allows(path, &request.path_filters) {
            continue;
        }
        let language = language_id(path).unwrap_or("unknown");
        if !language_filter_allows(path, language, &request.language_filters) {
            continue;
        }
        if seen.insert(path.clone()) {
            paths.push(path.clone());
        }
    }
    CandidatePaths {
        paths,
        degraded_reason: exhausted
            .then(|| "source fallback candidate file budget exhausted".to_owned()),
    }
}

#[derive(Clone, Copy)]
struct SourceMaterializationScope<'a> {
    path_filters: &'a [String],
    language_filters: &'a [String],
}

fn materialize_source_blobs(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    paths: &[String],
    scope: SourceMaterializationScope<'_>,
    tree: &mut TempSourceTree,
) -> Result<MaterializedFiles, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    materialize_source_blobs_with_budget(
        registration,
        &root,
        commit,
        paths,
        scope,
        tree,
        MAX_GREP_BYTES,
    )
}

fn materialize_source_blobs_with_budget(
    registration: &CodeRepositoryRegistration,
    root: &Path,
    commit: &str,
    paths: &[String],
    scope: SourceMaterializationScope<'_>,
    tree: &mut TempSourceTree,
    max_bytes: usize,
) -> Result<MaterializedFiles, CodeIndexError> {
    let verified_hashes = match ensure_source_grep_commit_current(
        registration,
        commit,
        scope.path_filters,
        scope.language_filters,
    ) {
        Ok(hashes) => hashes,
        Err(error) if source_commit_is_filesystem(commit) => {
            return Ok(MaterializedFiles {
                file_count: 0,
                degraded_reason: Some(error.to_string()),
            });
        }
        Err(error) => return Err(error),
    };
    let materialized = if let Some(selection) =
        candidate_source_blob_selection(root, commit, paths, max_bytes)
    {
        materialize_selected_source_blobs(root, commit, selection, tree, verified_hashes.as_ref())?
    } else {
        materialize_source_blobs_per_path(
            root,
            commit,
            paths,
            tree,
            max_bytes,
            verified_hashes.as_ref(),
        )?
    };
    if let Err(error) = ensure_source_grep_commit_current(
        registration,
        commit,
        scope.path_filters,
        scope.language_filters,
    ) {
        if source_commit_is_filesystem(commit) {
            return Ok(MaterializedFiles {
                file_count: 0,
                degraded_reason: Some(error.to_string()),
            });
        }
        return Err(error);
    }

    Ok(materialized)
}

fn ensure_source_grep_commit_current(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<Option<BTreeMap<String, String>>, CodeIndexError> {
    if source_commit_is_filesystem(commit) {
        return filesystem_hashes_for_verified_scope(
            registration,
            commit,
            path_filters,
            language_filters,
        )
        .map(Some);
    }

    Ok(None)
}

fn filesystem_hashes_for_verified_scope(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<BTreeMap<String, String>, CodeIndexError> {
    match scoped_source_snapshot_for_registration(registration, commit) {
        Ok(snapshot) => Ok(snapshot.content_hashes),
        Err(stored_scope_error) => {
            match scoped_source_snapshot_for_registration_filters(
                registration,
                commit,
                path_filters,
                language_filters,
            ) {
                Ok(snapshot) => Ok(snapshot.content_hashes),
                Err(_) => Err(stored_scope_error),
            }
        }
    }
}

fn materialize_selected_source_blobs(
    root: &Path,
    commit: &str,
    selection: BlobCandidateSelection,
    tree: &mut TempSourceTree,
    expected_hashes: Option<&BTreeMap<String, String>>,
) -> Result<MaterializedFiles, CodeIndexError> {
    let mut file_count = 0usize;
    for (path, bytes) in selection.paths.iter().zip(candidate_source_blobs(
        root,
        commit,
        &selection.paths,
        expected_hashes,
    )) {
        let Some(bytes) = bytes else {
            continue;
        };
        tree.write(path, bytes.as_slice())?;
        file_count += 1;
    }

    Ok(MaterializedFiles {
        file_count,
        degraded_reason: selection
            .exhausted
            .then(|| "source fallback materialized byte budget exhausted".to_owned()),
    })
}

fn materialize_source_blobs_per_path(
    root: &Path,
    commit: &str,
    paths: &[String],
    tree: &mut TempSourceTree,
    max_bytes: usize,
    expected_hashes: Option<&BTreeMap<String, String>>,
) -> Result<MaterializedFiles, CodeIndexError> {
    let mut byte_count = 0usize;
    let mut file_count = 0usize;
    let mut exhausted = false;
    for path in paths {
        let Ok(bytes) =
            source_bytes_after_content_verification(root, commit, path, expected_hashes)
        else {
            continue;
        };
        if byte_count.saturating_add(bytes.len()) > max_bytes {
            exhausted = true;
            continue;
        }
        tree.write(path, &bytes)?;
        byte_count += bytes.len();
        file_count += 1;
    }

    Ok(MaterializedFiles {
        file_count,
        degraded_reason: exhausted
            .then(|| "source fallback materialized byte budget exhausted".to_owned()),
    })
}

struct BlobCandidateSelection {
    paths: Vec<String>,
    exhausted: bool,
}

fn candidate_source_blob_selection(
    root: &Path,
    commit: &str,
    paths: &[String],
    max_bytes: usize,
) -> Option<BlobCandidateSelection> {
    let sizes = source_blob_sizes_after_policy_verification(root, commit, paths).ok()?;
    if sizes.len() != paths.len() {
        return None;
    }

    let mut selected_paths = Vec::new();
    let mut byte_count = 0usize;
    let mut exhausted = false;
    for (path, size) in paths.iter().zip(sizes) {
        let Some(size) = size else {
            continue;
        };
        if byte_count.saturating_add(size) > max_bytes {
            exhausted = true;
            continue;
        }
        selected_paths.push(path.clone());
        byte_count += size;
    }

    Some(BlobCandidateSelection {
        paths: selected_paths,
        exhausted,
    })
}

fn candidate_source_blobs(
    root: &Path,
    commit: &str,
    paths: &[String],
    expected_hashes: Option<&BTreeMap<String, String>>,
) -> Vec<Option<Vec<u8>>> {
    match source_batch_bytes_after_content_verification(root, commit, paths, expected_hashes) {
        Ok(blobs) if blobs.len() == paths.len() => blobs.into_iter().map(Some).collect(),
        Err(_) if source_commit_is_filesystem(commit) => paths.iter().map(|_| None).collect(),
        _ => paths
            .iter()
            .map(|path| {
                source_bytes_after_content_verification(root, commit, path, expected_hashes).ok()
            })
            .collect(),
    }
}

fn internal_source_grep_matches(
    root: &Path,
    paths: &[String],
    request: &SourceGrepRequest,
    accepts: impl Fn(&SourceGrepMatch) -> bool,
) -> Result<Vec<SourceGrepMatch>, CodeIndexError> {
    let queries = source_grep_queries(request);
    if queries.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    for path in paths {
        if matches.len() >= request.limit {
            break;
        }
        let Ok(bytes) = fs::read(root.join(path)) else {
            continue;
        };
        if source_bytes_are_binary(&bytes) {
            continue;
        }
        push_internal_file_matches(
            path,
            &bytes,
            &queries,
            request.kind,
            request.limit,
            &accepts,
            &mut matches,
        )?;
    }

    Ok(matches)
}

fn push_internal_file_matches(
    path: &str,
    bytes: &[u8],
    queries: &[Vec<u8>],
    kind: SourceGrepKind,
    limit: usize,
    accepts: &impl Fn(&SourceGrepMatch) -> bool,
    matches: &mut Vec<SourceGrepMatch>,
) -> Result<(), CodeIndexError> {
    let mut line_start = 0usize;
    let mut line_number = 1usize;
    let mut previous_line = None;
    while line_start < bytes.len() && matches.len() < limit {
        let line_end = bytes[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |offset| line_start + offset);
        let line = &bytes[line_start..line_end];
        let mut carried_line = SourceLineContext {
            byte_start: line_start,
            byte_end: line_end,
            line_start: line_number,
        };
        if let Some((match_start, match_end)) = find_query_bytes(line, queries) {
            if line.len() > MAX_GREP_LINE_BYTES && kind == SourceGrepKind::Definition {
                line_start = if line_end < bytes.len() {
                    line_end + 1
                } else {
                    bytes.len()
                };
                line_number += 1;
                continue;
            }
            let context = source_grep_line_context(bytes, line_start, line_end, previous_line);
            if let Some(context) = context {
                carried_line = context;
            }
            let byte_range = RepositoryCodeRange::new(
                "byte_range",
                context
                    .as_ref()
                    .map_or(line_start + match_start, |context| context.byte_start),
                context
                    .as_ref()
                    .map_or(line_start + match_end, |context| context.byte_end),
            )
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
            let line_range = RepositoryCodeRange::new(
                "line_range",
                context
                    .as_ref()
                    .map_or(line_number, |context| context.line_start),
                line_number,
            )
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
            let excerpt = context.map_or_else(
                || {
                    String::from_utf8_lossy(source_line_excerpt(line, match_start, match_end))
                        .trim_end_matches('\r')
                        .trim()
                        .to_owned()
                },
                |context| {
                    String::from_utf8_lossy(&bytes[context.byte_start..context.byte_end])
                        .trim_end_matches('\r')
                        .trim()
                        .to_owned()
                },
            );
            let matched = SourceGrepMatch {
                path: path.to_owned(),
                language_id: language_id(path).unwrap_or("unknown").to_owned(),
                excerpt,
                byte_range,
                line_range,
            };
            if accepts(&matched) {
                matches.push(matched);
            }
        }
        previous_line = Some(carried_line);
        line_start = if line_end < bytes.len() {
            line_end + 1
        } else {
            bytes.len()
        };
        line_number += 1;
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct SourceLineContext {
    byte_start: usize,
    byte_end: usize,
    line_start: usize,
}

fn source_grep_line_context(
    bytes: &[u8],
    line_start: usize,
    line_end: usize,
    previous_line: Option<SourceLineContext>,
) -> Option<SourceLineContext> {
    let previous = previous_line?;
    let previous_line = std::str::from_utf8(&bytes[previous.byte_start..previous.byte_end])
        .ok()?
        .trim();
    let current_line = std::str::from_utf8(&bytes[line_start..line_end])
        .ok()?
        .trim_start();
    if previous_line.starts_with("template ")
        || (current_line.starts_with('.')
            && (previous_line.ends_with('{')
                || previous_line
                    .lines()
                    .next()
                    .is_some_and(|line| line.trim_end().ends_with('{'))))
    {
        Some(SourceLineContext {
            byte_start: previous.byte_start,
            byte_end: line_end,
            line_start: previous.line_start,
        })
    } else {
        None
    }
}

fn source_bytes_are_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

fn source_line_excerpt(line: &[u8], match_start: usize, match_end: usize) -> &[u8] {
    if line.len() <= MAX_GREP_LINE_BYTES {
        return line;
    }

    let match_len = match_end.saturating_sub(match_start);
    let budget = MAX_GREP_LINE_BYTES.max(match_len);
    let ideal_start = match_start.saturating_sub((budget.saturating_sub(match_len)) / 2);
    let max_start = line.len().saturating_sub(budget);
    let start = ideal_start.min(max_start);
    let end = start.saturating_add(budget).min(line.len());
    &line[start..end]
}

fn path_filter_allows(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            let filter = normalize_filter_path(filter);
            filter == "." || path == filter || path.starts_with(&format!("{filter}/"))
        })
}

fn language_filter_allows(path: &str, language_id: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            filter == language_id
                || cxx_header_filter_allows(path, language_id, filter)
                || unknown_filter_allows_document_path(path, language_id, filter)
        })
}

fn cxx_header_filter_allows(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "cpp" && language_id == "c" && path.to_ascii_lowercase().ends_with(".h")
}

fn unknown_filter_allows_document_path(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "unknown" && document_like_language_path(path, language_id)
}

fn document_like_language_path(path: &str, language_id: &str) -> bool {
    matches!(
        language_id,
        "markdown" | "json" | "yaml" | "toml" | "xml" | "ini" | "properties"
    ) || matches!(
        path.rsplit('.')
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("md" | "markdown" | "txt" | "rst" | "adoc")
    )
}

fn normalize_filter_path(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

struct CandidatePaths {
    paths: Vec<String>,
    degraded_reason: Option<String>,
}

struct MaterializedFiles {
    file_count: usize,
    degraded_reason: Option<String>,
}

struct TempSourceTree {
    root: PathBuf,
}

impl TempSourceTree {
    fn create() -> Result<Self, CodeIndexError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-source-grep-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;

        Ok(Self { root })
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), CodeIndexError> {
        let target = self.root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, bytes)?;

        Ok(())
    }
}

impl Drop for TempSourceTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    #[test]
    fn internal_scanner_filters_definition_lines_before_enforcing_limit() {
        let mut tree = TempSourceTree::create().expect("temp tree should be created");
        tree.write("src/lib.c", b"return target();\nint target(void);\n")
            .expect("source path should be written");
        let request = SourceGrepRequest {
            query: "target".to_owned(),
            paths: vec!["src/lib.c".to_owned()],
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            limit: 1,
            kind: SourceGrepKind::Definition,
        };

        let matches =
            internal_source_grep_matches(&tree.root, &request.paths, &request, |matched| {
                source_line_defines_identity(&matched.excerpt, "target")
            })
            .expect("internal scanner should apply definition acceptance");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line_range.start, 2);
        assert_eq!(matches[0].excerpt, "int target(void);");
    }

    #[test]
    fn internal_scanner_includes_template_preamble_for_declaration_lines() {
        let mut tree = TempSourceTree::create().expect("temp tree should be created");
        tree.write(
            "include/cache.h",
            b"template <typename InstanceType>\nclass NoDestructor {};\n",
        )
        .expect("source path should be written");
        let request = SourceGrepRequest {
            query: "NoDestructor".to_owned(),
            paths: vec!["include/cache.h".to_owned()],
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            limit: 1,
            kind: SourceGrepKind::Definition,
        };

        let matches =
            internal_source_grep_matches(&tree.root, &request.paths, &request, |matched| {
                source_grep_accepts(SourceGrepKind::Definition, "NoDestructor", matched)
            })
            .expect("internal scanner should include template context");

        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].line_range,
            RepositoryCodeRange { start: 1, end: 2 }
        );
        assert!(
            matches[0]
                .excerpt
                .contains("template <typename InstanceType>")
        );
        assert!(matches[0].excerpt.contains("class NoDestructor"));
    }

    #[test]
    fn hybrid_scanner_tokenizes_query_and_keeps_initializer_header() {
        let mut tree = TempSourceTree::create().expect("temp tree should be created");
        tree.write(
            "src/generated_table.c",
            b"static const struct rk_table_row rk_rows[] = {\n  [RK_STAGE_READ] = {\n    .name = \"read\",\n    .read = rk_driver_read,\n  },\n};\n",
        )
        .expect("source path should be written");
        let request = SourceGrepRequest {
            query: "compound initializer table row read function pointer".to_owned(),
            paths: vec!["src/generated_table.c".to_owned()],
            path_filters: vec!["src/generated_table.c".to_owned()],
            language_filters: vec!["c".to_owned()],
            limit: 5,
            kind: SourceGrepKind::Hybrid,
        };

        let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |_| true)
            .expect("hybrid scanner should search query terms");

        assert!(matches.iter().any(|matched| {
            matched.excerpt.contains("[RK_STAGE_READ]")
                && matched.excerpt.contains(".read = rk_driver_read")
        }));
    }

    #[test]
    fn unknown_language_filter_allows_document_source_fallback_candidates() {
        assert!(language_filter_allows(
            "docs/operations.md",
            "markdown",
            &["unknown".to_owned()]
        ));
        assert!(!language_filter_allows(
            "src/service.py",
            "python",
            &["unknown".to_owned()]
        ));
    }

    #[test]
    fn internal_scanner_searches_materialized_paths_without_ripgrep() {
        let mut tree = TempSourceTree::create().expect("temp tree should be created");
        tree.write(
            ".github/workflows/ci.yml",
            b"# RK_INTERNAL_SCANNER_REFERENCE\nname: ci\n",
        )
        .expect("hidden path should be written");
        let request = SourceGrepRequest {
            query: "RK_INTERNAL_SCANNER_REFERENCE".to_owned(),
            paths: vec![".github/workflows/ci.yml".to_owned()],
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            limit: 5,
            kind: SourceGrepKind::References,
        };

        let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |_| true)
            .expect("internal scanner should read materialized files");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, ".github/workflows/ci.yml");
        assert_eq!(matches[0].line_range.start, 1);
        assert_eq!(matches[0].byte_range.start, 2);
        assert!(matches[0].excerpt.contains("RK_INTERNAL_SCANNER_REFERENCE"));
    }

    #[test]
    fn internal_scanner_returns_bounded_excerpts_for_long_non_definition_lines() {
        let mut tree = TempSourceTree::create().expect("temp tree should be created");
        let prefix = "x".repeat(MAX_GREP_LINE_BYTES + 64);
        let suffix = "y".repeat(MAX_GREP_LINE_BYTES + 64);
        let source = format!("{prefix}RK_LONG_REFERENCE{suffix}\n");
        tree.write("dist/bundle.js", source.as_bytes())
            .expect("long source path should be written");
        let request = SourceGrepRequest {
            query: "RK_LONG_REFERENCE".to_owned(),
            paths: vec!["dist/bundle.js".to_owned()],
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            limit: 5,
            kind: SourceGrepKind::References,
        };

        let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |_| true)
            .expect("internal scanner should return long-line matches");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line_range.start, 1);
        assert_eq!(matches[0].byte_range.start, prefix.len() as u32);
        assert!(matches[0].excerpt.contains("RK_LONG_REFERENCE"));
        assert!(matches[0].excerpt.len() <= MAX_GREP_LINE_BYTES);
    }

    #[test]
    fn internal_scanner_skips_binary_blobs() {
        let mut tree = TempSourceTree::create().expect("temp tree should be created");
        tree.write("assets/blob.bin", b"prefix RK_BINARY_REFERENCE\0suffix\n")
            .expect("binary path should be written");
        let request = SourceGrepRequest {
            query: "RK_BINARY_REFERENCE".to_owned(),
            paths: vec!["assets/blob.bin".to_owned()],
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            limit: 5,
            kind: SourceGrepKind::References,
        };

        let matches = internal_source_grep_matches(&tree.root, &request.paths, &request, |_| true)
            .expect("internal scanner should skip binary blobs without failing");

        assert!(matches.is_empty());
    }

    #[test]
    fn internal_scanner_primary_path_does_not_report_ripgrep_unavailable() {
        let mut tree = TempSourceTree::create().expect("temp tree should be created");
        tree.write("src/component.tsx", b"import React from \"react\";\n")
            .expect("source path should be written");
        let request = SourceGrepRequest {
            query: "react".to_owned(),
            paths: vec!["src/component.tsx".to_owned()],
            path_filters: Vec::new(),
            language_filters: vec!["tsx".to_owned()],
            limit: 10,
            kind: SourceGrepKind::Imports,
        };

        let outcome =
            source_grep_matches_from_materialized_tree(&tree.root, &request.paths, &request, None)
                .expect("internal scanner should search materialized source");

        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].path, "src/component.tsx");
        assert_eq!(outcome.matches[0].language_id, "tsx");
        assert_eq!(outcome.matches[0].excerpt, "import React from \"react\";");
        assert!(outcome.degraded_reason.is_none());
    }

    #[test]
    fn materialization_skips_oversized_blob_and_keeps_later_candidates() {
        let repo = TestRepo::create("grep-materialization-budget");
        repo.write("large.txt", "abcdef");
        repo.write("small.txt", "xy");
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "budget fixture"]);
        let mut tree = TempSourceTree::create().expect("temp tree should be created");
        let paths = vec!["large.txt".to_owned(), "small.txt".to_owned()];
        let registration = CodeRepositoryRegistration::new(
            "repo",
            "alias",
            repo.root.display().to_string(),
            Vec::new(),
            Vec::new(),
        )
        .expect("registration should validate");

        let materialized = materialize_source_blobs_with_budget(
            &registration,
            &repo.root,
            "HEAD",
            &paths,
            SourceMaterializationScope {
                path_filters: &[],
                language_filters: &[],
            },
            &mut tree,
            5,
        )
        .expect("materialization should succeed");

        assert_eq!(materialized.file_count, 1);
        assert!(materialized.degraded_reason.is_some());
        assert!(!tree.root.join("large.txt").exists());
        assert_eq!(
            fs::read_to_string(tree.root.join("small.txt")).expect("small blob should exist"),
            "xy"
        );
    }

    #[test]
    fn candidate_paths_apply_scope_filters_and_budget() {
        let request = SourceGrepRequest {
            query: "target".to_owned(),
            paths: vec![
                "src/lib.rs".to_owned(),
                "../bad.rs".to_owned(),
                "tests/lib.rs".to_owned(),
                "src/app.py".to_owned(),
            ],
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            limit: 5,
            kind: SourceGrepKind::Hybrid,
        };

        let candidates = selected_candidate_paths(&request);

        assert_eq!(candidates.paths, ["src/lib.rs"]);
    }

    struct TestRepo {
        root: PathBuf,
    }

    impl TestRepo {
        fn create(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let root = std::env::temp_dir().join(format!(
                "relay-knowledge-{name}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("repo directory should be created");
            let repo = Self { root };
            repo.git(["init"]);
            repo.git(["config", "user.email", "relay@example.invalid"]);
            repo.git(["config", "user.name", "Relay Test"]);
            repo
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent directory should exist");
            }
            fs::write(path, content).expect("fixture file should be written");
        }

        fn git<const N: usize>(&self, args: [&str; N]) {
            let output = Command::new("git")
                .current_dir(&self.root)
                .args(args)
                .output()
                .expect("git should run");
            assert!(
                output.status.success(),
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
