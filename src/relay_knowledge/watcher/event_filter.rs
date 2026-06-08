use std::path::{Path, PathBuf};

const INDEXABLE_EXTENSIONS: &[&str] = &[
    "rs",
    "py",
    "js",
    "mjs",
    "ts",
    "tsx",
    "jsx",
    "go",
    "java",
    "rb",
    "c",
    "cpp",
    "cc",
    "cxx",
    "h",
    "hpp",
    "cs",
    "kt",
    "kts",
    "scala",
    "swift",
    "php",
    "sh",
    "bash",
    "yaml",
    "yml",
    "toml",
    "json",
    "xml",
    "md",
    "markdown",
    "rst",
    "adoc",
    "sql",
    "cmake",
    "make",
    "dockerfile",
    "ini",
    "properties",
];

const IGNORED_DIRECTORIES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "__pycache__",
    ".venv",
    "venv",
    ".env",
    "build",
    "dist",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".idea",
    ".vscode",
    ".cache",
];

#[derive(Debug, Clone, Default)]
pub struct WatcherEventFilter {
    watched_root: Option<PathBuf>,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
}

impl WatcherEventFilter {
    pub fn new(
        watched_root: PathBuf,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> Self {
        Self {
            watched_root: Some(watched_root),
            path_filters,
            language_filters,
        }
    }

    pub fn should_process_path(&self, path: &Path) -> bool {
        let relative = match self.strip_root(path) {
            Some(rel) => rel,
            None => return false,
        };

        if self.is_in_ignored_directory(&relative) {
            return false;
        }

        if !self.has_indexable_extension(&relative) {
            return false;
        }

        if !self.path_filters.is_empty() && !self.path_matches_scope(&relative) {
            return false;
        }

        if !self.language_filters.is_empty() && !self.language_matches_extension(&relative) {
            return false;
        }

        true
    }

    fn strip_root(&self, path: &Path) -> Option<PathBuf> {
        let root = self.watched_root.as_ref()?;
        path.strip_prefix(root).ok().map(|p| p.to_path_buf())
    }

    fn is_in_ignored_directory(&self, relative: &Path) -> bool {
        for component in relative.ancestors() {
            if let Some(name) = component.file_name() {
                if let Some(name_str) = name.to_str() {
                    if IGNORED_DIRECTORIES.contains(&name_str) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn has_indexable_extension(&self, relative: &Path) -> bool {
        let ext = match relative.extension().and_then(|e| e.to_str()) {
            Some(e) => e.to_lowercase(),
            None => {
                let name = relative.file_name().and_then(|n| n.to_str()).unwrap_or("");
                return matches!(
                    name.to_lowercase().as_str(),
                    "dockerfile"
                        | "makefile"
                        | "cmakelists.txt"
                        | "cargo.toml"
                        | "package.json"
                        | "go.mod"
                        | "requirements.txt"
                        | "pipfile"
                        | "gemfile"
                );
            }
        };
        INDEXABLE_EXTENSIONS.contains(&ext.as_str())
    }

    fn path_matches_scope(&self, relative: &Path) -> bool {
        let relative_str = relative.to_string_lossy();
        self.path_filters
            .iter()
            .any(|f| relative_str.starts_with(f.as_str()) || relative_str.contains(f.as_str()))
    }

    fn language_matches_extension(&self, relative: &Path) -> bool {
        let ext = match relative.extension().and_then(|e| e.to_str()) {
            Some(e) => e.to_lowercase(),
            None => return true,
        };
        self.language_filters
            .iter()
            .any(|lang| extension_matches_language(&ext, lang))
    }
}

fn extension_matches_language(ext: &str, language: &str) -> bool {
    match language {
        "rust" => ext == "rs",
        "python" | "py" => ext == "py",
        "javascript" | "js" => ext == "js" || ext == "jsx" || ext == "mjs",
        "typescript" | "ts" => ext == "ts" || ext == "tsx",
        "go" => ext == "go",
        "java" => ext == "java",
        "c" => ext == "c" || ext == "h",
        "cpp" | "c++" => ext == "cpp" || ext == "hpp" || ext == "cc" || ext == "cxx",
        "ruby" | "rb" => ext == "rb",
        "kotlin" | "kt" => ext == "kt" || ext == "kts",
        "scala" => ext == "scala",
        "swift" => ext == "swift",
        "csharp" | "c#" => ext == "cs",
        "php" => ext == "php",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from("/project")
    }

    #[test]
    fn allows_rust_source_file() {
        let filter = WatcherEventFilter::new(root(), vec![], vec![]);
        assert!(filter.should_process_path(&root().join("src/main.rs")));
    }

    #[test]
    fn rejects_git_directory() {
        let filter = WatcherEventFilter::new(root(), vec![], vec![]);
        assert!(!filter.should_process_path(&root().join(".git/HEAD")));
    }

    #[test]
    fn rejects_node_modules() {
        let filter = WatcherEventFilter::new(root(), vec![], vec![]);
        assert!(!filter.should_process_path(&root().join("node_modules/foo/index.js")));
    }

    #[test]
    fn rejects_binary_file() {
        let filter = WatcherEventFilter::new(root(), vec![], vec![]);
        assert!(!filter.should_process_path(&root().join("image.png")));
    }

    #[test]
    fn allows_dockerfile_without_extension() {
        let filter = WatcherEventFilter::new(root(), vec![], vec![]);
        assert!(filter.should_process_path(&root().join("Dockerfile")));
    }

    #[test]
    fn path_filter_accepts_matching_prefix() {
        let filter = WatcherEventFilter::new(root(), vec!["src/".to_owned()], vec![]);
        assert!(filter.should_process_path(&root().join("src/lib.rs")));
        assert!(!filter.should_process_path(&root().join("docs/README.md")));
    }

    #[test]
    fn rejects_path_outside_root() {
        let filter = WatcherEventFilter::new(root(), vec![], vec![]);
        assert!(!filter.should_process_path(&PathBuf::from("/other/project/main.rs")));
    }

    #[test]
    fn rejects_target_directory() {
        let filter = WatcherEventFilter::new(root(), vec![], vec![]);
        assert!(!filter.should_process_path(&root().join("target/debug/lib.rs")));
    }

    #[test]
    fn default_filter_rejects_empty_path() {
        let filter = WatcherEventFilter::default();
        assert!(!filter.should_process_path(&PathBuf::from("main.rs")));
    }

    #[test]
    fn allows_python_file() {
        let filter = WatcherEventFilter::new(root(), vec![], vec![]);
        assert!(filter.should_process_path(&root().join("app/models.py")));
    }

    #[test]
    fn rejects_pycache() {
        let filter = WatcherEventFilter::new(root(), vec![], vec![]);
        assert!(!filter.should_process_path(&root().join("__pycache__/foo.cpython-311.pyc")));
    }

    #[test]
    fn allows_known_language_alias_extensions() {
        let filter = WatcherEventFilter::new(root(), vec![], vec![]);
        assert!(filter.should_process_path(&root().join("src/lib.cc")));
        assert!(filter.should_process_path(&root().join("web/app.mjs")));
        assert!(filter.should_process_path(&root().join("build.gradle.kts")));
    }

    #[test]
    fn unknown_language_filter_does_not_match_extension_by_name() {
        let filter = WatcherEventFilter::new(root(), vec![], vec!["rs".to_owned()]);
        assert!(!filter.should_process_path(&root().join("src/main.rs")));
    }
}
