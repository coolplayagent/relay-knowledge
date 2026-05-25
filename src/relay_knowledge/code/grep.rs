use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::domain::{CodeRepositoryRegistration, RepositoryCodeRange};

use super::{
    CodeIndexError,
    git::{git_batch_blob_sizes, git_batch_blobs, git_bytes},
    languages::language_id,
    safe_git_blob_path, source_line_defines_identity,
};

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
    super::git::validate_git_ref_arg("commit", commit)?;
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
    let root = PathBuf::from(&registration.root_path);
    let mut tree = TempSourceTree::create()?;
    let materialized = materialize_git_blobs(&root, commit, &candidates.paths, &mut tree)?;
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
    kind != SourceGrepKind::Definition || source_line_defines_identity(&matched.excerpt, query)
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
        if !language_filter_allows(language, &request.language_filters) {
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

fn materialize_git_blobs(
    root: &Path,
    commit: &str,
    paths: &[String],
    tree: &mut TempSourceTree,
) -> Result<MaterializedFiles, CodeIndexError> {
    materialize_git_blobs_with_budget(root, commit, paths, tree, MAX_GREP_BYTES)
}

fn materialize_git_blobs_with_budget(
    root: &Path,
    commit: &str,
    paths: &[String],
    tree: &mut TempSourceTree,
    max_bytes: usize,
) -> Result<MaterializedFiles, CodeIndexError> {
    if let Some(selection) = candidate_git_blob_selection(root, commit, paths, max_bytes) {
        return materialize_selected_git_blobs(root, commit, selection, tree);
    }

    materialize_git_blobs_per_path(root, commit, paths, tree, max_bytes)
}

fn materialize_selected_git_blobs(
    root: &Path,
    commit: &str,
    selection: BlobCandidateSelection,
    tree: &mut TempSourceTree,
) -> Result<MaterializedFiles, CodeIndexError> {
    let mut file_count = 0usize;
    for (path, bytes) in
        selection
            .paths
            .iter()
            .zip(candidate_git_blobs(root, commit, &selection.paths))
    {
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

fn materialize_git_blobs_per_path(
    root: &Path,
    commit: &str,
    paths: &[String],
    tree: &mut TempSourceTree,
    max_bytes: usize,
) -> Result<MaterializedFiles, CodeIndexError> {
    let mut byte_count = 0usize;
    let mut file_count = 0usize;
    let mut exhausted = false;
    for path in paths {
        let object = format!("{commit}:{path}");
        let Ok(bytes) = git_bytes(root, ["show", &object]) else {
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

fn candidate_git_blob_selection(
    root: &Path,
    commit: &str,
    paths: &[String],
    max_bytes: usize,
) -> Option<BlobCandidateSelection> {
    let sizes = git_batch_blob_sizes(root, commit, paths).ok()?;
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

fn candidate_git_blobs(root: &Path, commit: &str, paths: &[String]) -> Vec<Option<Vec<u8>>> {
    match git_batch_blobs(root, commit, paths) {
        Ok(blobs) if blobs.len() == paths.len() => blobs.into_iter().map(Some).collect(),
        _ => paths
            .iter()
            .map(|path| {
                let object = format!("{commit}:{path}");
                git_bytes(root, ["show", &object]).ok()
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
    let query = request.query.as_bytes();
    if query.is_empty() {
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
            query,
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
    query: &[u8],
    kind: SourceGrepKind,
    limit: usize,
    accepts: &impl Fn(&SourceGrepMatch) -> bool,
    matches: &mut Vec<SourceGrepMatch>,
) -> Result<(), CodeIndexError> {
    let mut line_start = 0usize;
    let mut line_number = 1usize;
    while line_start < bytes.len() && matches.len() < limit {
        let line_end = bytes[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |offset| line_start + offset);
        let line = &bytes[line_start..line_end];
        if let Some(match_start) = find_bytes(line, query) {
            if line.len() > MAX_GREP_LINE_BYTES && kind == SourceGrepKind::Definition {
                line_start = if line_end < bytes.len() {
                    line_end + 1
                } else {
                    bytes.len()
                };
                line_number += 1;
                continue;
            }
            let match_end = match_start + query.len();
            let byte_range = RepositoryCodeRange::new(
                "byte_range",
                line_start + match_start,
                line_start + match_end,
            )
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
            let line_range = RepositoryCodeRange::new("line_range", line_number, line_number)
                .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
            let excerpt =
                String::from_utf8_lossy(source_line_excerpt(line, match_start, match_end))
                    .trim_end_matches('\r')
                    .trim()
                    .to_owned();
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
        line_start = if line_end < bytes.len() {
            line_end + 1
        } else {
            bytes.len()
        };
        line_number += 1;
    }

    Ok(())
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

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn path_filter_allows(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            let filter = normalize_filter_path(filter);
            filter == "." || path == filter || path.starts_with(&format!("{filter}/"))
        })
}

fn language_filter_allows(language_id: &str, filters: &[String]) -> bool {
    filters.is_empty() || filters.iter().any(|filter| filter == language_id)
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

        let materialized =
            materialize_git_blobs_with_budget(&repo.root, "HEAD", &paths, &mut tree, 5)
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
