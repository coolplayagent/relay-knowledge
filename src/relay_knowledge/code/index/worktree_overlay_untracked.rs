use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

const WORKTREE_UNTRACKED_BROAD_SEGMENTS: &[&str] = &[
    ".cache",
    ".next",
    ".nuxt",
    ".parcel-cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    ".venv",
    "__pycache__",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "out",
    "target",
    "third_party",
    "vendor",
    "venv",
];

pub(super) fn allowed(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    !path_contains_broad_segment(path) || explicit_path_filter_covers(path, registration, selector)
}

fn path_contains_broad_segment(path: &str) -> bool {
    normalize_path(path)
        .split('/')
        .any(|segment| WORKTREE_UNTRACKED_BROAD_SEGMENTS.contains(&segment))
}

fn explicit_path_filter_covers(
    path: &str,
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> bool {
    registration
        .path_filters
        .iter()
        .chain(selector.path_filters.iter())
        .any(|filter| explicit_filter_matches_path(path, filter))
}

fn explicit_filter_matches_path(path: &str, filter: &str) -> bool {
    let path = normalize_path(path);
    let filter = normalize_path(filter);
    if filter.is_empty() || filter == "." {
        return false;
    }

    path == filter
        || path.starts_with(&format!("{filter}/"))
        || filter.starts_with(&format!("{path}/"))
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_owned()
}
