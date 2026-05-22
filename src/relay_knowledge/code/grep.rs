use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use crate::domain::{CodeRepositoryRegistration, RepositoryCodeRange};

use super::{
    CodeIndexError, git::git_bytes, languages::language_id, safe_git_blob_path,
    source_line_defines_identity,
};

pub(crate) const SOURCE_GREP_CANDIDATE_FILE_LIMIT: usize = 256;
const MAX_GREP_CANDIDATE_FILES: usize = SOURCE_GREP_CANDIDATE_FILE_LIMIT;
const MAX_GREP_BYTES: usize = 8 * 1024 * 1024;
const MAX_GREP_LINE_BYTES: usize = 4096;
const MAX_DEFINITION_MATCHES_PER_FILE: usize = 64;
const RIPGREP_TIMEOUT: Duration = Duration::from_secs(3);
const RIPGREP_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    let rg_output = run_ripgrep(
        &tree.root,
        &request.query,
        ripgrep_match_limit(request.kind, request.limit),
    );
    let output = match rg_output {
        RipgrepRun::Output(output) => output,
        RipgrepRun::Degraded(reason) => {
            return Ok(SourceGrepOutcome {
                matches: Vec::new(),
                degraded_reason: Some(reason),
            });
        }
    };
    let matches = match parse_ripgrep_matches(&output.stdout, request.limit, |matched| {
        request.kind != SourceGrepKind::Definition
            || source_line_defines_identity(&matched.excerpt, &request.query)
    }) {
        Ok(matches) => matches,
        Err(error) => {
            return Ok(SourceGrepOutcome {
                matches: Vec::new(),
                degraded_reason: Some(format!("ripgrep output parse failed: {error}")),
            });
        }
    };

    Ok(SourceGrepOutcome {
        matches,
        degraded_reason,
    })
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
        degraded_reason: exhausted.then(|| "ripgrep candidate file budget exhausted".to_owned()),
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
        degraded_reason: exhausted.then(|| "ripgrep materialized byte budget exhausted".to_owned()),
    })
}

fn ripgrep_match_limit(kind: SourceGrepKind, result_limit: usize) -> usize {
    match kind {
        SourceGrepKind::Definition => result_limit.max(MAX_DEFINITION_MATCHES_PER_FILE),
        SourceGrepKind::References | SourceGrepKind::Imports | SourceGrepKind::Hybrid => {
            result_limit
        }
    }
}

fn run_ripgrep(root: &Path, query: &str, limit: usize) -> RipgrepRun {
    let max_columns = MAX_GREP_LINE_BYTES.to_string();
    let max_count = limit.to_string();
    let mut child = match Command::new("rg")
        .current_dir(root)
        .args([
            "--json",
            "--line-number",
            "--column",
            "--fixed-strings",
            "--no-heading",
            "--color",
            "never",
            "--hidden",
            "--max-columns",
            &max_columns,
            "--max-count",
            &max_count,
            "--",
            query,
            ".",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RipgrepRun::Degraded("ripgrep unavailable".to_owned());
        }
        Err(error) => return RipgrepRun::Degraded(format!("ripgrep failed to start: {error}")),
    };
    let stdout = child.stdout.take().map(spawn_pipe_reader);
    let stderr = child.stderr.take().map(spawn_pipe_reader);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= RIPGREP_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = collect_pipe_reader(stdout, "stdout");
                let _ = collect_pipe_reader(stderr, "stderr");
                return RipgrepRun::Degraded("ripgrep timeout".to_owned());
            }
            Ok(None) => std::thread::sleep(RIPGREP_POLL_INTERVAL),
            Err(error) => {
                let _ = collect_pipe_reader(stdout, "stdout");
                let _ = collect_pipe_reader(stderr, "stderr");
                return RipgrepRun::Degraded(format!("ripgrep wait failed: {error}"));
            }
        }
    };
    let stdout = match collect_pipe_reader(stdout, "stdout") {
        Ok(output) => output,
        Err(error) => return RipgrepRun::Degraded(error),
    };
    let stderr = match collect_pipe_reader(stderr, "stderr") {
        Ok(output) => output,
        Err(error) => return RipgrepRun::Degraded(error),
    };
    if status.success() || status.code() == Some(1) {
        return RipgrepRun::Output(RipgrepOutput { stdout });
    }
    let stderr = String::from_utf8_lossy(&stderr).trim().to_owned();
    RipgrepRun::Degraded(if stderr.is_empty() {
        "ripgrep exited with an error".to_owned()
    } else {
        format!("ripgrep exited with an error: {stderr}")
    })
}

fn spawn_pipe_reader(mut pipe: impl Read + Send + 'static) -> JoinHandle<io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut output = Vec::new();
        pipe.read_to_end(&mut output)?;
        Ok(output)
    })
}

fn collect_pipe_reader(
    reader: Option<JoinHandle<io::Result<Vec<u8>>>>,
    stream: &str,
) -> Result<Vec<u8>, String> {
    let Some(reader) = reader else {
        return Ok(Vec::new());
    };
    reader
        .join()
        .map_err(|_| format!("ripgrep {stream} reader panicked"))?
        .map_err(|error| format!("ripgrep {stream} read failed: {error}"))
}

fn parse_ripgrep_matches(
    output: &[u8],
    limit: usize,
    accepts: impl Fn(&SourceGrepMatch) -> bool,
) -> Result<Vec<SourceGrepMatch>, CodeIndexError> {
    let mut matches = Vec::new();
    for line in output.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let value = serde_json::from_slice::<Value>(line)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
        if value.get("type").and_then(Value::as_str) != Some("match") {
            continue;
        }
        let data = value.get("data").ok_or_else(|| {
            CodeIndexError::InvalidInput("ripgrep match data is missing".to_owned())
        })?;
        if let Some(matched) = match_from_json(data)?.filter(|matched| accepts(matched)) {
            matches.push(matched);
            if matches.len() >= limit {
                break;
            }
        }
    }

    Ok(matches)
}

fn match_from_json(data: &Value) -> Result<Option<SourceGrepMatch>, CodeIndexError> {
    let Some(path) = data
        .get("path")
        .map(json_text_or_bytes)
        .transpose()?
        .flatten()
        .map(|path| normalize_ripgrep_path(&path))
    else {
        return Ok(None);
    };
    let line_number = data
        .get("line_number")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(1);
    let absolute_offset = data
        .get("absolute_offset")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let excerpt = data
        .get("lines")
        .map(json_text_or_bytes)
        .transpose()?
        .flatten()
        .unwrap_or_default()
        .trim_end_matches(['\r', '\n'])
        .trim()
        .to_owned();
    let (match_start, match_end) = first_submatch_range(data).unwrap_or((0, excerpt.len() as u32));
    let byte_range = RepositoryCodeRange::new(
        "byte_range",
        absolute_offset.saturating_add(match_start) as usize,
        absolute_offset.saturating_add(match_end) as usize,
    )
    .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
    let line_range =
        RepositoryCodeRange::new("line_range", line_number as usize, line_number as usize)
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;

    Ok(Some(SourceGrepMatch {
        language_id: language_id(&path).unwrap_or("unknown").to_owned(),
        path,
        excerpt,
        byte_range,
        line_range,
    }))
}

fn json_text_or_bytes(value: &Value) -> Result<Option<String>, CodeIndexError> {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return Ok(Some(text.to_owned()));
    }
    let Some(bytes) = value.get("bytes").and_then(Value::as_str) else {
        return Ok(None);
    };
    let decoded = decode_base64(bytes)?;

    Ok(Some(String::from_utf8_lossy(&decoded).into_owned()))
}

fn decode_base64(encoded: &str) -> Result<Vec<u8>, CodeIndexError> {
    let mut sextets = Vec::new();
    let mut padding_count = 0usize;
    for byte in encoded.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if byte == b'=' {
            padding_count += 1;
            if padding_count > 2 {
                return Err(CodeIndexError::InvalidInput(
                    "ripgrep bytes field has invalid base64 padding".to_owned(),
                ));
            }
            sextets.push(0);
            continue;
        }
        if padding_count > 0 {
            return Err(CodeIndexError::InvalidInput(
                "ripgrep bytes field has non-padding data after base64 padding".to_owned(),
            ));
        }
        let Some(value) = base64_value(byte) else {
            return Err(CodeIndexError::InvalidInput(
                "ripgrep bytes field contains invalid base64 data".to_owned(),
            ));
        };
        sextets.push(value);
    }
    if sextets.is_empty() {
        return Ok(Vec::new());
    }
    if sextets.len() % 4 != 0 {
        return Err(CodeIndexError::InvalidInput(
            "ripgrep bytes field has incomplete base64 data".to_owned(),
        ));
    }

    let chunk_count = sextets.len() / 4;
    let mut decoded = Vec::with_capacity(chunk_count * 3);
    for (index, chunk) in sextets.chunks_exact(4).enumerate() {
        let packed = ((chunk[0] as u32) << 18)
            | ((chunk[1] as u32) << 12)
            | ((chunk[2] as u32) << 6)
            | (chunk[3] as u32);
        let chunk_padding = if index + 1 == chunk_count {
            padding_count
        } else {
            0
        };
        decoded.push((packed >> 16) as u8);
        if chunk_padding < 2 {
            decoded.push((packed >> 8) as u8);
        }
        if chunk_padding < 1 {
            decoded.push(packed as u8);
        }
    }

    Ok(decoded)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn first_submatch_range(data: &Value) -> Option<(u32, u32)> {
    let submatch = data
        .get("submatches")
        .and_then(Value::as_array)
        .and_then(|items| items.first())?;
    let start = submatch
        .get("start")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())?;
    let end = submatch
        .get("end")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())?;
    Some((start, end))
}

fn normalize_ripgrep_path(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/")
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

enum RipgrepRun {
    Output(RipgrepOutput),
    Degraded(String),
}

struct RipgrepOutput {
    stdout: Vec<u8>,
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
        let root =
            std::env::temp_dir().join(format!("relay-knowledge-rg-{}-{nanos}", std::process::id()));
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
    use super::*;

    #[test]
    fn parses_ripgrep_json_match() {
        let output = br#"{"type":"match","data":{"path":{"text":"./src/lib.rs"},"lines":{"text":"pub fn target() {}\n"},"line_number":7,"absolute_offset":42,"submatches":[{"match":{"text":"target"},"start":7,"end":13}]}}"#;

        let matches = parse_ripgrep_matches(output, 10, |_| true).expect("json should parse");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "src/lib.rs");
        assert_eq!(matches[0].language_id, "rust");
        assert_eq!(matches[0].excerpt, "pub fn target() {}");
        assert_eq!(matches[0].line_range.start, 7);
        assert_eq!(matches[0].byte_range.start, 49);
        assert_eq!(matches[0].byte_range.end, 55);
    }

    #[test]
    fn parses_ripgrep_json_bytes_fields() {
        let output = br#"{"type":"match","data":{"path":{"bytes":"Li9zcmMvbGliLnJz"},"lines":{"bytes":"cHViIGZuIHRhcmdldCgpIHt9Cg=="},"line_number":7,"absolute_offset":42,"submatches":[{"match":{"text":"target"},"start":7,"end":13}]}}"#;

        let matches = parse_ripgrep_matches(output, 10, |_| true).expect("json should parse");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "src/lib.rs");
        assert_eq!(matches[0].excerpt, "pub fn target() {}");
        assert_eq!(matches[0].byte_range.start, 49);
        assert_eq!(matches[0].byte_range.end, 55);
    }

    #[test]
    fn filters_ripgrep_matches_before_enforcing_limit() {
        let output = br#"{"type":"match","data":{"path":{"text":"./src/lib.rs"},"lines":{"text":"return target();\n"},"line_number":3,"absolute_offset":20,"submatches":[{"match":{"text":"target"},"start":7,"end":13}]}}
{"type":"match","data":{"path":{"text":"./src/lib.rs"},"lines":{"text":"int target(void);\n"},"line_number":9,"absolute_offset":80,"submatches":[{"match":{"text":"target"},"start":4,"end":10}]}}"#;

        let matches = parse_ripgrep_matches(output, 1, |matched| {
            source_line_defines_identity(&matched.excerpt, "target")
        })
        .expect("json should parse");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line_range.start, 9);
        assert_eq!(matches[0].excerpt, "int target(void);");
    }

    #[test]
    fn ripgrep_searches_hidden_materialized_paths() {
        if Command::new("rg").arg("--version").output().is_err() {
            return;
        }
        let mut tree = TempSourceTree::create().expect("temp tree should be created");
        tree.write(
            ".github/workflows/ci.yml",
            b"# RK_HIDDEN_WORKFLOW_REFERENCE\nname: ci\n",
        )
        .expect("hidden path should be written");

        let output = match run_ripgrep(&tree.root, "RK_HIDDEN_WORKFLOW_REFERENCE", 5) {
            RipgrepRun::Output(output) => output,
            RipgrepRun::Degraded(reason) => panic!("ripgrep should search hidden paths: {reason}"),
        };
        let matches = parse_ripgrep_matches(&output.stdout, 5, |_| true)
            .expect("ripgrep output should parse");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, ".github/workflows/ci.yml");
        assert!(matches[0].excerpt.contains("RK_HIDDEN_WORKFLOW_REFERENCE"));
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
